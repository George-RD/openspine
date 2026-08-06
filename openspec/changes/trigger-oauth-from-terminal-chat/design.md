## Context

`openspine chat` intercepts only `/exit`, `/quit`, `/help`, and `/status` before the governed pipeline (`crates/openspine-kernel/src/main.rs:955-1038`). OAuth login exists as a separate top-level command (`cli::login::run_provider_login`, `main.rs:234-240`) that acquires its own exclusive data-root lifetime lock through `wizard::open_vault`. Chat startup already holds that lock (`overlay_export_restore::acquire`, `main.rs:247-264`) and keeps two live owners in `run`: the local `overlay_operations` and `secrets` Arcs plus their clones inside `AppState`. D-014 requires setup secrets to bypass shell and model context; the archived `2026-08-05-ground-terminal-setup-guidance` change grounded the prompt but cannot make model output a safe trigger for account authorization.

## Goals / Non-Goals

Goals: give the owner one explicit local gesture (`/login`, optional provider id) that starts the existing PKCE flow; guarantee the flow runs only after chat and its HTTP server are fully torn down and the lifetime lock is released; keep `--once` output byte-exact; keep the authorization code and tokens on the existing vault path.

Non-goals: model-triggered login; concurrent login while the kernel serves requests; hot-reloading `AppState.provider_pool` (immutable by design, `pipeline/mod.rs:127-135`); new provider transports; device-code polling.

## Decisions

- **Teardown-then-exec over in-process login.** In-chat login would race the OAuth refresher (`oauth/refresher.rs` writes the vault on 401) and would leave the immutable provider pool stale, making "logged in" a lie for the live process. Child-process login (spawn `openspine provider login`) fails at `open_vault` because the parent holds the lifetime lock. The chosen path: `run_terminal_chat` returns a requested provider, `run` drops every remaining lock owner in order (`state` already dropped inside the chat function; then local `secrets`, then `overlay_operations`), then calls the existing `run_provider_login` and exits. Rejected alternatives: runtime provider-pool reload plus shared vault write serialization (large, not needed at n=1); print-only guidance (leaves the owner's expectation unmet).
- **Exit after login rather than restart chat in-process.** Re-entering chat would require rebuilding the entire startup sequence inside `run` a second time (stores, overlay reconciliation, registry). Process exit makes "next `openspine` start uses the new binding" the observable contract, matching how `provider login` already behaves from the shell.
- **Parser lives in `cli::onboarding` beside `help_text`.** The chat loop stays a thin match; a pure `parse_local_command` function gives the no-event/no-grant contract a deterministic unit-test seam without spawning a kernel.
- **`/login` defaults to Anthropic.** `OAUTH_PROVIDER_IDS` currently contains exactly one entry; passing the id through to `run_provider_login` preserves the existing refusal messages for `openai-codex` and `google-antigravity` (login-unsupported transports) and for unknown ids.

## Risks / Trade-offs

- [Risk] A future startup path spawns a task capturing `AppState` before chat returns, resurrecting a hidden lock owner → the regression test acquires a fresh `OverlayOperations` lock after the teardown helper runs, so any survivor fails the suite.
- [Risk] Owner types `/login` mid-conversation and loses chat context → acceptable; the command prints what is happening before teardown, and conversation history is persisted per grant, not per process.
- [Risk] Login stores a credential but verification fails → existing `complete_login` behavior already keeps the credential, skips role binding, and names the retry (`openspine setup`).
- Audit: local commands are kernel-owned UI, mint no events, and never reach the audit chain; the OAuth flow itself keeps its existing audit-free CLI semantics, and the vault write path is unchanged.

## Migration Plan

Pure addition to the chat loop and help surfaces; no data or config migration. Rollback is reverting the commit. Deployment follows the standard flake-input bump on gascity.

## Open Questions

None.
- [Risk] axum serves each connection on a detached task holding a clone of
  the chat state; joining only the accept loop would let a trailing shell
  request carry the lock into the handoff → on `/login` the chat server
  shuts down gracefully and drains until the serve future resolves, which
  is bounded by construction because every kernel handler carries its own
  timeout (model gateway 60 seconds, connectors 30 seconds). Other exits
  keep the old immediate teardown, since process exit reclaims the tasks.

# Change: Implement first-run onboarding and a working setup wizard

## Layer

OpenSpine core. The change touches the kernel CLI, provider credential intake,
and terminal chat startup. It adds no new runtime authority.

## Authority sensitivity

Authority-sensitive. Onboarding writes model-provider OAuth credentials into the
encrypted `SecretStore` vault and rebinds provider auth mode in
`openspine.yaml`. It does not widen task-grant authority, connector access, or
private-data reach.

## Dependencies

- `model-provider-oauth-onboarding`: already requires `openspine setup` and
  `openspine provider login` to run a real OAuth PKCE flow with a headless
  fallback and a pre-flight verification ping before role binding.
- `direct-terminal-chat`: defines the governed `openspine chat` lane that a
  first-run owner lands in.
- `day-2-operations`: defines the first-run and restart sequence.
- D-027: interactive OAuth PKCE login for model providers.

## Why

The shipped binary contradicts the archived `model-provider-oauth-onboarding`
spec. `openspine setup` prints a three-line banner and exits
(`crates/openspine-kernel/src/main.rs:203-209`). `openspine provider login`
prints one line and exits (`main.rs:210-217`). The real PKCE, vault, ping, and
role-binding functions in `crates/openspine-kernel/src/cli/setup.rs` are marked
`#[allow(dead_code)]` and are never called from any command path.

A new owner therefore has no working path from install to a configured
assistant. Running `openspine` drops straight into the chat prompt with no
indication of what is configured, what is missing, or what to do next. When
startup does fail, the owner sees a raw error such as `data root is already
locked` or `Address already in use (os error 98)` with no remedy.

Onboarding is the first thing a self-hosting owner touches. A substrate whose
safety story depends on the owner configuring it correctly cannot leave
configuration undiscoverable.

## What Changes

- Add a deterministic readiness assessment that reports whether an install can
  serve a governed turn, naming each blocking gap and its remedy.
- Replace the `openspine setup` placeholder with a real wizard that reports
  readiness, writes a starter configuration and key file when absent, runs
  provider login, verifies the provider, and binds model roles only after a
  successful verification.
- Add `openspine setup --check`: a non-interactive readiness report with a
  process exit code, usable from scripts and smoke tests.
- Replace the `openspine provider login` placeholder with the real OAuth PKCE
  flow, including the headless and remote-SSH fallback the spec already
  requires.
- Recognize first start in `openspine chat`: print an orientation notice and the
  readiness checklist when onboarding has never completed, and record
  completion so a ready install shows the notice once.
- Add `/help` and `/status` to the terminal chat loop.
- Map the known startup failures (held data-root lock, bound listener address,
  missing key material, missing configuration) to messages that name the
  remedy.
- Resolve the starter configuration's package directory from the running
  executable so an installed binary does not depend on the working directory or
  on a store path captured at install time.

## Acceptance Criteria

- `openspine setup --check` on an unconfigured install exits non-zero and lists
  every blocking gap with a remedy.
- `openspine setup --check` on the configured gascity install exits zero.
- `openspine provider login anthropic` performs a real PKCE authorization,
  falling back to printed URL and pasted code without a browser.
- Provider credentials reach the vault only through `SecretStore`, and no
  onboarding output contains secret material.
- Model roles are bound only after a successful verification request.
- A first `openspine chat` on an unconfigured install prints the blocking gaps
  instead of an unexplained prompt.
- A second `openspine chat` on a ready install prints no orientation notice.
- Starting a second instance names the running instance instead of printing a
  raw lock path.
- `./scripts/check.sh 2026-08-04-implement-first-run-onboarding` passes.

## Prerequisite this change surfaces

The three OAuth provider specs shipped hardcoded placeholder client ids, so a
login would have been rejected by the provider on arrival. Neither Anthropic nor
OpenAI offers self-service registration for subscription OAuth, so an
owner-suppliable client id does not exist; the providers' public first-party ids
are the only values that authorize, and a PKCE public client carries no secret.
Those ids are now embedded.

Reaching a real provider also required the rest of the wire contract, which had
never been exercised: the `user:inference` scope, `code=true`, a JSON token
exchange carrying `state`, and the client headers, user agents, and system block
on both inference and refresh.

Only Anthropic is offered. Codex and Antigravity authorize correctly and produce
credentials this build cannot spend, because their grants require provider
transports the model gateway does not implement.

## Out of Scope

- A vault-backed API key intake path. Readiness names the environment variable a
  provider expects; the model gateway's key resolution is unchanged.
- OAuth device authorization code polling. The manual-paste fallback covers the
  headless case this change verifies.
- Starting the background OAuth refresher from `main`. Refresh stays demand
  triggered on provider HTTP 401.
- Telegram and Gmail connector onboarding, which keep their existing documented
  setup flows.
- The native package installer tracked by `2026-07-28-productize-lyra`.

# OpenSpine

Most agent frameworks answer "what can my agent do?" OpenSpine answers "what
is my agent *allowed* to do — and can I prove it?"

OpenSpine is a self-hostable runtime substrate for governed agents, written
in Rust. Events are verified, identity is resolved, and authority is
*composed* — deny-by-default, from declarative artifacts — into a
short-lived task grant. Every effect passes one gate. Every decision is
hash-chain audited. Every security claim in this README maps to a test you
can run.

```
event → source verification → identity → route → authority composition → task grant → agent/workflow → gated effects → audit/memory
```

**Lyra** is the first product built on OpenSpine: a governed personal
assistant controlled through a verified Telegram owner channel, starting
with selected-thread Gmail reply drafting and digest-bound draft approval.

> OpenClaw gives an assistant claws. OpenSpine gives it a backbone.

## If you're tired of…

Capability frameworks — OpenClaw-style assistants, LangGraph-style
orchestration, most agent SDKs — optimise what an agent *can* do: more
tools, more connectors, more autonomy. Their failure mode is an authority
failure, not a capability one: a prompt-injected email turns into an
outbound action, a tool call nobody scoped, an agent that widened its own
permissions because nothing was watching. Bolting a policy layer on top of
a capability-first design after the fact is exactly how those failures
happen.

OpenSpine's bet is the substrate owns authority, not the model and not the
agent framework. Identity alone never grants trust. A route, agent,
workflow, capability pack, or policy is only ever a *candidate* input —
the task grant that authority composition issues is the one live authority
object a running agent holds, and it can never be widened except through
the same owner-approval mechanism as every other effect.

What OpenSpine does **not** do, on purpose: there is no artifact
marketplace, no autonomy ladder that lets an agent earn more trust over
time, and `email.send` is a hard `Deny` regardless of grant or approval
state — Lyra drafts, it never sends. The restraint is the pitch.

## Claims and proof

`./scripts/check.sh` runs every test below on every change. The full
register, including claims not assertable inside `cargo test` (e.g. Docker
network topology), lives in [`docs/threat-claims.md`](docs/threat-claims.md).

| Claim | Proof |
| --- | --- |
| Telegram owner messages are verified against the configured owner ID | `configured_owner_text_message_is_verified` |
| Identity is not authority — a spoofed owner ID without a verified source is denied | `spoofed_owner_id_without_verified_source_is_denied` |
| Connector authentication and account role grant no trust by themselves | `gmail_connector_authenticated_alone_does_not_match_the_selected_thread_route` |
| External content is data, never instruction | `email_reply_drafter_template_wraps_untrusted_context_on_the_wire` |
| The shell receives no raw connector credentials | `process_driver_clears_env_and_sets_only_two_vars` |
| Private-context model calls are mediated by the model gateway, untrusted context always wrapped | `generate_sends_untrusted_context_in_body` |
| User-selected targets are proven with selection tokens, single-use | `email_read_selected_thread_rejects_foreign_grant`, `email_read_selected_thread_rejects_second_use` |
| Authority composes by deterministic intersection — no candidate allow means no grant | `no_candidate_allow_means_action_is_not_granted` |
| Explicit deny wins over any allow; approval-required overrides a plain allow | `explicit_deny_overrides_allow`, `approval_required_overrides_plain_allow` |
| Every effectful action is mediated by `gate()` before dispatch | `approval_required_action_stops_before_dispatch` |
| Audit records reference encrypted artifacts, never plaintext | `audit_metadata_records_action_grant_and_refs_not_plaintext` |
| The shell cannot widen its own authority without explicit owner approval | `widening_via_a_proposed_pack_requires_approval_first` |
| LLMs may not resolve authority-affecting route conflicts | `priority_tie_with_equal_specificity_is_ambiguous` |
| Email send is denied regardless of grant or approval state | `global_policy_round_trips_and_denies_send` |
| Kernel replies are channel-bound to the grant-bound owner chat | `lyra_ui_preview_sends_telegram_reply_to_grant_bound_chat` |
| System-operations actions (host filesystem, raw network egress) are denied by default | `host_filesystem_read_and_write_are_denied_for_owner_control_grant` |

## Pointer map

| Document | What it covers |
| --- | --- |
| [`.raw/openspine-prd-v9.md`](.raw/openspine-prd-v9.md) | The product/architecture spec: envelope shapes, artifact examples, phase exit criteria. |
| [`.raw/openspine-decision-log.md`](.raw/openspine-decision-log.md) | Why the architecture is shaped the way it is (48 decisions, D-001–D-049), and closed open questions (O-001–O-008). |
| [`docs/threat-claims.md`](docs/threat-claims.md) | Every security claim mapped to the test (or documented manual justification) that proves it. |
| [`openspec/`](openspec/) | The OpenSpec-driven development process: 11 applied capability specs, in-flight changes, and the implementation sequence in [`openspec/openspine-change-sequence.md`](openspec/openspine-change-sequence.md). |
| [`openspec/conventions.md`](openspec/conventions.md) | Per-change ceremony: proposal → spec → design → tasks → archive. |

## Workspace layout

Five Rust crates, workspace-managed dependencies (see root `Cargo.toml`):

- `openspine-schemas` — versioned, `deny_unknown_fields` object kinds for every runtime concept (event, identity, route, grant, action, approval, artifact, audit) plus the canonical-JSON digest functions. Pure data, no I/O.
- `openspine-authority` — `resolve_route` and `compose_authority`: pure functions that merge route/workflow/agent/pack/policy inputs into a task grant or a denial.
- `openspine-gate` — the `gate()` mediation boundary every effectful action passes through before a connector runs it.
- `openspine-kernel` (bin `openspine`) — the trusted process: storage, artifact store, connectors (Telegram, Gmail), model gateway, audit chain, and the kernel HTTP API.
- `openspine-shell` (bin `openspine-shell`) — the contained per-task worker that runs agent/workflow logic; its only I/O is the kernel API.

## Quickstart

```sh
cargo build --workspace
./scripts/check.sh          # fmt, clippy -D warnings, tests, file-size gate, claims gate, openspec validate --all --strict
```

Running a real kernel instance requires bootstrap secrets as environment
variables (never committed, see `.env.example`):
`OPENSPINE_TELEGRAM_BOT_TOKEN`, `OPENSPINE_ARTIFACT_KEY` (32-byte hex,
`openssl rand -hex 32`), a configured model-provider API key, and —
optionally, only if Gmail is configured — `OPENSPINE_GMAIL_CLIENT_SECRET` /
`OPENSPINE_GMAIL_REFRESH_TOKEN`. See `openspine.yaml` (created in
[`docs/telegram-setup.md`](docs/telegram-setup.md)) for the rest of the
configuration surface, and [`docs/gmail-setup.md`](docs/gmail-setup.md) for
the Gmail connector.

## Lyra today

Owner-control channel: DM the configured Telegram bot from the owner
account. `/status` reads kernel status; `/draft <thread_id>` fetches a
Gmail thread (found via Gmail's own web UI URL) and drafts a reply through
the model gateway, previewed over Telegram — no draft is created and no
email is sent by the preview itself. Tapping "Approve" on a preview creates
the exact reviewed Gmail draft, bound to the exact payload and target the
owner saw (digest-bound approval); anything else replies through a
deterministic command layer or a kernel-mediated model call. `/propose
<kind>` lets an agent propose a new route, agent, workflow, capability
pack, or policy — it stays inert until the owner approves the exact
YAML via the same digest-bound approval mechanism.

## Threat notes

- The Gmail connector requests `gmail.readonly` + `gmail.compose`. There is
  no draft-only Google scope — `gmail.compose` technically permits send at
  the OAuth layer. The actual boundary is that the OAuth token never leaves
  the kernel, and `email.send` is a hard `Deny` in `gate()` regardless of
  grant or approval state (D-015, D-029).
- Bootstrap secrets (bot token, artifact key, provider keys) are read from
  environment variables in phases 1–3; this is a documented, temporary
  deferral, not a final secret-management story (D-014, D-025). A
  secret-intake flow is a future change.
- The shell process is contained (`SandboxDriver`: `ProcessDriver` is
  dev-only and unsafe for real private data; `DockerDriver` is the first
  driver that provides real network isolation) — see D-005, D-026. The
  kernel itself needs the Docker socket to spawn shell containers;
  `compose.yaml` includes a commented-out `docker-socket-proxy` option that
  narrows the kernel's Docker API access to only container
  create/start/stop/remove.
- The kernel↔shell link is plain HTTP over the Docker-Compose-internal
  network, not TLS — a deliberate, documented trust boundary for phases
  1–3, not an oversight. See
  [`docs/kernel-http-contract.md`](docs/kernel-http-contract.md).

## Status

Alpha. Phases 1–3 of the PRD are implemented: authority composition, the
gate-mediated action API, the Telegram owner channel, selected-thread Gmail
preview, digest-bound draft approval, and a minimal artifact-lifecycle
slice (propose → owner-approve → activate). See
[`openspec/openspine-change-sequence.md`](openspec/openspine-change-sequence.md)
for the applied sequence and deferred work (secret intake, a real thread
picker, per-kind activation policies).

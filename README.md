# OpenSpine

OpenSpine is a self-hostable runtime substrate for governed agents. It accepts
events, verifies their source, resolves identity, chooses a route, composes
authority from all relevant policy artifacts, issues a bounded task grant,
runs an agent/workflow, mediates every effect through a single gate, records
audit events, and updates memory only through policy:

```
event → source verification → identity → route → authority composition → task grant → agent/workflow → gated effects → audit/memory
```

**Lyra** is the first product built on OpenSpine: a governed personal
assistant controlled through a verified Telegram owner channel, starting with
selected-thread Gmail reply drafting.

> OpenClaw gives an assistant claws. OpenSpine gives it a backbone.

## Why this exists

Identity is not authority (a channel/sender match alone grants nothing).
Routes, agents, workflows, and capability packs are *candidate* inputs to
authority, not authority itself — the task grant issued by authority
composition is the only live authority object a running agent ever holds.
Every effectful action — reads, model calls, connector writes — is mediated
by `gate()`. External content (email, web pages, inbound messages) is always
data, never instruction. See `.raw/openspine-decision-log.md` for the full
reasoning behind each of these choices.

## Pointer map

| Document | What it covers |
| --- | --- |
| [`.raw/openspine-prd-v9.md`](.raw/openspine-prd-v9.md) | The product/architecture spec: envelope shapes, artifact examples, phase exit criteria. |
| [`.raw/openspine-decision-log.md`](.raw/openspine-decision-log.md) | Why the architecture is shaped the way it is (D-001–D-033), and closed open questions (O-001–O-008). |
| [`openspec/`](openspec/) | The OpenSpec-driven development process: applied specs, in-flight changes, and the fixed implementation sequence in [`openspec/openspine-change-sequence.md`](openspec/openspine-change-sequence.md). |
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
./scripts/check.sh          # fmt, clippy -D warnings, tests, file-size gate, openspec validate --all --strict
```

Running a real kernel instance requires bootstrap secrets as environment
variables (never committed): `OPENSPINE_TELEGRAM_BOT_TOKEN`,
`OPENSPINE_ARTIFACT_KEY` (32-byte hex, `openssl rand -hex 32`), and any
configured model-provider API keys. See `openspine.yaml` (created in Step 4)
for the rest of the configuration surface.

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
  driver that provides real network isolation) — see D-005, D-026.

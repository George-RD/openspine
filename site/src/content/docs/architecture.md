---
title: Architecture
description: The event-to-audit pipeline, the crate map, and the kernel/shell trust boundary.
---

This page explains how OpenSpine is built and how data flows through it.

## The pipeline

Every inbound event runs through the same fixed sequence before an agent
ever does anything:

```mermaid
flowchart LR
    A[event] --> B[source verification]
    B --> C[identity]
    C --> D[route]
    D --> E[authority composition]
    E --> F[task grant]
    F --> G[agent / workflow]
    G --> H[gated effects]
    H --> I[audit / memory]
```

- **Source verification** — is this event's claimed origin real? (e.g. a
  Telegram message's sender ID matched against the configured owner.)
- **Identity** — who is this, structurally — never "what can they do."
- **Route** — which workflow/agent pairing handles this event, resolved
  declaratively, never by an LLM (route-conflict resolution is
  authority-affecting and LLMs are never trusted with it).
- **Authority composition** — deterministic, deny-by-default intersection
  across every relevant route, agent manifest, workflow, capability pack,
  and policy, producing one task grant or an explicit denial.
- **Task grant** — the one live authority object an agent holds: a
  short-lived, scoped bearer token plus the allowed/approval-required
  action lists, budgets, and any selection tokens.
- **Agent / workflow** — runs in a contained shell process, with no I/O
  except the kernel API.
- **Gated effects** — every effectful action passes through `gate()`
  before a connector runs it.
- **Audit / memory** — every decision is appended to a hash-chained audit
  log; memory only ever updates through policy, never freely.

## Crate map

- `openspine-schemas` — versioned, `deny_unknown_fields` object kinds for every runtime concept (event, identity, route, grant, action, approval, artifact, audit) plus the canonical-JSON digest functions. Pure data, no I/O.
- `openspine-authority` — `resolve_route` and `compose_authority`: pure functions that merge route/workflow/agent/pack/policy inputs into a task grant or a denial.
- `openspine-gate` — the `gate()` mediation boundary every effectful action passes through before a connector runs it.
- `openspine-kernel` (bin `openspine`) — the trusted process: storage, artifact store, connectors (Telegram, Gmail), model gateway, audit chain, and the kernel HTTP API.
- `openspine-shell` (bin `openspine-shell`) — the contained per-task worker that runs agent/workflow logic; its only I/O is the kernel API.

## The kernel/shell trust boundary

The shell is never trusted with anything the kernel itself needs to stay
secret. Concretely (full contract in
[`docs/kernel-http-contract.md`](https://github.com/George-RD/openspine/blob/main/docs/kernel-http-contract.md)):

- The shell process/container receives exactly two environment variables:
  `KERNEL_ENDPOINT` and `TASK_TOKEN`. No provider API keys, no artifact
  encryption key, no Telegram bot token.
- The shell never computes digests or encrypts anything itself — it sends
  raw JSON payloads over `POST /v1/actions` and `POST /v1/model/generate`,
  and the kernel builds the real, digested, artifact-referenced request
  server-side.
- The kernel↔shell link is plain HTTP (no TLS) over a Docker-Compose
  internal network with no route to the public internet — a deliberate,
  documented trust boundary for the current phases, not an oversight.
- Under the `docker` sandbox driver, shell containers get no public-internet
  egress at all; under the dev-only `process` driver, an explicit
  `unsafe_allow_uncontained_private_data` flag is required before the
  kernel will even route an `external_communication`-lane event to it.

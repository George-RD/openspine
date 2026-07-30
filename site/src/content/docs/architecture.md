---
title: Architecture
description: The runtime underneath Lyra: event-to-audit pipeline, crate map, and kernel/shell boundary.
---

OpenSpine is the self-hosted system. Lyra is the default assistant package. This page covers the governed runtime underneath it: the part that verifies requests, creates task permissions, holds credentials, checks actions, and records outcomes.

The runtime is small on purpose: one fixed pipeline, five crates, one trust boundary. This page walks the spine from an inbound event to its audit record.

## The pipeline

Every inbound event runs through the same fixed sequence before an agent ever does anything:

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

- **Source verification**: is this event's claimed origin real? For example, did a Telegram message's sender ID arrive through the verified owner channel?
- **Identity**: who is this, structurally, never “what can they do.”
- **Route**: which workflow and agent pairing handles this event, resolved declaratively rather than by an LLM. Route conflict resolution affects authority, so the model does not own it.
- **Authority composition**: deterministic, deny-by-default intersection across every relevant route, agent manifest, workflow, capability pack, policy, caveat, approval, and runtime limit.
- **Task grant**: the one live authority object a worker holds. It contains a short-lived token, scoped actions, approval requirements, budgets, and any selection tokens.
- **Agent / workflow**: runs in a contained shell process with no I/O except the kernel API.
- **Gated effects**: every effectful action passes through `gate()` before a connector runs it.
- **Audit / memory**: every decision is appended to a hash-chained audit log. Memory and learned behaviour update through governed lifecycles rather than free mutation.

The order is the security model. Authority is settled before model-driven worker code runs, so nothing the model generates can reach back and renegotiate the task grant.

## Crate map

- `openspine-schemas`: versioned, `deny_unknown_fields` object kinds for every runtime concept, plus canonical-JSON digest functions. Pure data, no I/O.
- `openspine-authority`: route resolution and authority composition as pure functions that merge route, workflow, agent, pack, policy, caveat, and runtime inputs into a task grant or denial.
- `openspine-gate`: the `gate()` mediation boundary every effectful action passes through before a connector runs it.
- `openspine-kernel` (bin `openspine`): the trusted process. It owns storage, the artifact store, connectors, model gateway, audit chain, and kernel HTTP API.
- `openspine-shell` (bin `openspine-shell`): the contained per-task worker that runs agent and workflow logic. Its only I/O is the kernel API.

The split mirrors the trust argument: schemas and authority are pure functions; the kernel is the only process that holds secrets; the shell is the place model-driven code runs, and it holds no connector credential.

## The kernel/shell trust boundary

The shell is never trusted with anything the kernel needs to keep secret. The full contract lives in [`docs/kernel-http-contract.md`](https://github.com/George-RD/openspine/blob/main/docs/kernel-http-contract.md).

- The shell process or container receives exactly two environment variables: `KERNEL_ENDPOINT` and `TASK_TOKEN`. It receives no provider API key, artifact encryption key, Gmail credential, or Telegram bot token.
- The shell does not compute trusted digests or encrypt artifacts. It submits intents to the kernel, which constructs the real digested and artifact-referenced request.
- The kernel and shell communicate over the Compose internal network. The contained shell has no route to the public internet.
- Under the `docker` sandbox driver, task workers run in ephemeral containers. Under the development-only `process` driver, an explicit `unsafe_allow_uncontained_private_data` flag is required before the kernel routes a private external-communication task.

## Why this is a personal AI system, not only a library

The runtime is reusable, but a user does not interact with `compose_authority()` or `gate()` directly. They talk to Lyra. The assistant package declares the conversation, workflows, skills, persona, and memory scope. The runtime turns those declarations into enforceable task boundaries.

That separation is the product:

```text
Lyra decides what work to propose.
OpenSpine decides what work may happen.
```
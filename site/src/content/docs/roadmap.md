---
title: Roadmap
description: What works through Lyra, what has landed in the runtime, and what still needs a product surface.
---

OpenSpine has two kinds of progress:

1. **Runtime capability**: kernel machinery implemented, specified, and tested.
2. **Product capability**: something a user can discover and complete through Lyra.

Those are not the same. A worker runtime, standing-rule engine, or skill lifecycle may be real without yet being a useful default assistant workflow.

The canonical implementation ledger is [`openspec/openspine-change-sequence.md`](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md). The public security evidence is [`docs/threat-claims.md`](https://github.com/George-RD/openspine/blob/main/docs/threat-claims.md). This page translates those sources into product status.

## Available through Lyra today

The current owner-facing alpha proves one guarded workflow end to end:

- verified Telegram owner control;
- `/draft <thread_id>` for one Gmail thread selected by the owner;
- a live, attachment-free Gmail read bound to a single-use selection token;
- reply drafting through the model gateway;
- a Telegram preview of the exact proposed draft;
- digest-bound approval of the payload and target;
- Gmail draft creation after approval;
- email sending denied by global runtime policy;
- encrypted artifacts, task-grant records, gate decisions, and audit receipts.

This is a working trust proof. It is not yet a broad personal-assistant experience.

## Landed in the OpenSpine runtime

The change ledger records substantially more machinery than the public Gmail flow exposes.

### Authority and containment

- deterministic authority composition with deny by default and explicit-deny precedence;
- task grants as the only live worker authority;
- one gate for effectful actions, with trusted-path carve-outs enumerated and audited;
- contained process and Docker worker drivers;
- kernel-held connector and model credentials;
- parameter, selection-token, and digest binding;
- hash-chained audit and encrypted artifact storage;
- global spend caps and per-task budgets.

### Durable work and delegation

- an event bus with idempotent subscribers;
- durable workflow replay and kernel timers;
- task-board objects and deterministic task slices;
- declarative workflow state machines;
- caveat-chain worker grants, worker result chokepoints, supervision, restart limits, and failure escalation;
- deterministic briefcase packing for task context.

### Governed learning and growth

- base and user-owned overlay artifacts with provenance;
- persona artifacts and deterministic persona binding;
- versioned skill artifacts with a separate install and promotion lifecycle;
- standing rules with budgets, expiry, drift handling, and dark-window defaults;
- a reflection miner that proposes changes through the normal lifecycle;
- authority-equivalence matching that cannot cross permission classes;
- disclosure policies and typed egress classes;
- overlay export and restore.

### Operations and connectors

- connector rate limits, timeouts, refresh, idempotency, and circuit breakers;
- failure taxonomy, owner notifications, dead letters, and secure digest detail retrieval;
- versioned migrations, backup and restore guidance, disk-full and clock-regression handling;
- native model-provider OAuth onboarding and token refresh;
- secret intake and rotation through the kernel vault.

A runtime item appears here because the canonical change is archived. It should not be advertised as a complete Lyra feature until an owner-facing path proves it end to end.

## Product work now raised

The positioning audit exposed three product gaps that the architecture roadmap did not make visible enough.

### [Install OpenSpine as an assistant system](https://github.com/George-RD/openspine/issues/117)

Ship the intended package model: a transactional package store, an explicit selected package, and a coherent `install lyra` / `use lyra` / `run` flow. This resolves the current ambiguity between OpenSpine the installed system, Lyra the default assistant, and OpenSpine the reusable runtime.

### [Ship a first-run trust loop](https://github.com/George-RD/openspine/issues/118)

Treat onboarding as product work now, not as polish deferred until a second deployment exists. Preflight the full setup, replace copied thread IDs with a minimal selector, show the task boundary, and finish with a compact receipt.

### [Separate runtime-landed from wired-into-Lyra](https://github.com/George-RD/openspine/issues/119)

Create one generated capability map and require an owner-path test before describing a runtime primitive as available through Lyra. Select the next starter workflows from machinery that has already landed.

## Deliberate current limits

- **No email send.** The current Lyra policy denies it in every grant and approval state.
- **No arbitrary third-party assistant compatibility.** OpenSpine loads Lyra as its default package; a general adapter surface is not shipped.
- **No broad inbox selection.** The owner currently supplies a Gmail thread ID.
- **No consumer-grade setup.** The supported path is for technical self-hosters.
- **No silent capability growth.** New authority remains a reviewed lifecycle event.
- **No claim that every archived kernel component is a finished Lyra workflow.**

## Product direction

The north star remains a chief-of-staff-style personal AI:

- do internal work without interrupting the owner;
- ask at a real effect or disclosure boundary;
- turn repeated decisions into revocable standing rules after explicit confirmation;
- commission contained workers with only the context and authority they need;
- learn preferences without turning memory into permission;
- show concise receipts and exceptions rather than constant approval prompts.

The next product work should make that experience visible without weakening the runtime invariants. Breadth is useful only when the boundary remains understandable.
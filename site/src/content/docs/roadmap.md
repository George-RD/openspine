---
title: Roadmap
description: What works through Lyra, what has landed in the runtime, and what still needs a product surface.
---

OpenSpine has two kinds of progress:

1. **Runtime capability**: kernel machinery implemented, specified, and tested.
2. **Product capability**: something a user can discover and complete through Lyra.

Those are not the same. A worker runtime, standing-rule engine, reflection miner, or skill lifecycle may be real without yet being a useful default assistant workflow.

The canonical implementation ledger is [`openspec/openspine-change-sequence.md`](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md). The public security evidence is [`docs/threat-claims.md`](https://github.com/George-RD/openspine/blob/main/docs/threat-claims.md). This page translates those sources into product status.

<!-- capability-map:start -->
## Capability map

This table is generated from [`capabilities/capability-map.json`](https://github.com/George-RD/openspine/blob/main/capabilities/capability-map.json). CI checks its runtime change IDs against the archived implementation ledger, verifies every evidence path, and requires each **Wired into Lyra** claim to name real owner-path tests. Issue numbers are blockers, never repository proof.

**Current count:** 2 wired into Lyra · 3 known product surfaces missing · 2 runtime-only capabilities · 0 proof in progress

| Owner outcome | State | Repository proof | Current limit |
|---|---|---|---|
| Talk to Lyra from the local terminal through the governed task path. | **Wired into Lyra** | [2026-07-28-add-terminal-chat-onyx-lfm](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)<br />[terminal_owner_status_reaches_device_through_real_shell](https://github.com/George-RD/openspine/blob/main/crates/openspine-kernel/src/pipeline/tests/terminal_e2e.rs) | The owner can converse locally, but this path does not yet expose recurring account work or responsibility growth. |
| Choose one Gmail thread, review the exact reply, and create only the approved draft. | **Wired into Lyra** | [implement-selected-thread-email-preview-slice](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)<br />[implement-digest-bound-draft-approval](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)<br />[draft_command_for_a_real_thread_composes_a_bound_selection_grant](https://github.com/George-RD/openspine/blob/main/crates/openspine-kernel/src/pipeline/tests/draft.rs)<br />[a_double_tap_on_approve_creates_only_one_gmail_draft](https://github.com/George-RD/openspine/blob/main/crates/openspine-kernel/src/pipeline/tests/approval.rs) | The owner still supplies a thread ID and approves each draft; email sending remains denied. |
| Let Lyra grow through real work by reviewing and delegating reusable, protocol-neutral responsibility for narrowly bounded recurring work. | **Product surface missing** | [implement-standing-rules](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)<br />[implement-reflection-miner](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)<br />[implement-overlay-eval-gate](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)<br />[implement-artifact-lifecycle-slice](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)<br />[harden-approval-and-budgets](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)<br />[define-grant-chain-and-modes](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)<br />[implement-spend-kill-switch](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)<br />[backfill-implemented-capability-specs](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md) | The runtime can propose, evaluate, budget, expire, match, revoke, and audit reusable authority, but Lyra has no plain-language owner loop that joins those pieces into a protocol-neutral responsibility. |
| Give Lyra a commitment and have it return at the right time with durable task state. | **Product surface missing** | [implement-task-board](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)<br />[implement-durable-workflow-replay](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)<br />[implement-workflow-state-machines](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)<br />[implement-seed-workflows](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md) | Task objects, timers, replay, and seed workflows exist in the runtime, but Lyra has no supported owner-facing commitment flow. |
| Ask Lyra to research and return a brief without silently disclosing private context. | **Product surface missing** | [implement-disclosure-policy](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)<br />[implement-briefcase-packing](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)<br />[implement-worker-runtime](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md) | Disclosure classes, bounded context, and contained workers have landed, but no complete research-and-brief workflow is wired into Lyra. |
| Commission contained workers with bounded context and receive structured results. | **Runtime landed** | [implement-worker-runtime](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)<br />[implement-worker-supervision](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md) | This is kernel machinery, not a workflow that should be exposed to the owner as a worker-management primitive. |
| Use versioned skills without allowing a skill or model to install or promote itself. | **Runtime landed** | [implement-skill-artifact-class](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md) | The lifecycle and containment tests exist, but Lyra does not yet present a useful owner outcome around installing and applying a skill. |

### Let Lyra grow through real work by reviewing and delegating reusable, protocol-neutral responsibility for narrowly bounded recurring work.

**State:** Product surface missing (generic capability)

**Landed substrate:**

- [standing-rules](https://github.com/George-RD/openspine/blob/main/openspec/specs/standing-rules/spec.md)
- [reflection-miner](https://github.com/George-RD/openspine/blob/main/openspec/specs/reflection-miner/spec.md)
- [artifact-lifecycle](https://github.com/George-RD/openspine/blob/main/openspec/specs/artifact-lifecycle/spec.md)
- [audit-artifact-store](https://github.com/George-RD/openspine/blob/main/openspec/specs/audit-artifact-store/spec.md)
- [spend-kill-switch](https://github.com/George-RD/openspine/blob/main/openspec/specs/spend-kill-switch/spec.md)
- [authority-composition](https://github.com/George-RD/openspine/blob/main/openspec/specs/authority-composition/spec.md)

**Blockers:**

- **Execution/review foundations:** [#129](https://github.com/George-RD/openspine/issues/129)
- **Proposal-specific evaluation:** [#133](https://github.com/George-RD/openspine/issues/133)
- **Scoped evidence/matching:** [#128](https://github.com/George-RD/openspine/issues/128)

**Selected proof:** [recurring Gmail drafts (#130)](https://github.com/George-RD/openspine/issues/130)

**Portability proof:** [second communication shape (#131)](https://github.com/George-RD/openspine/issues/131)

**Whole-responsibility progression:** [#132](https://github.com/George-RD/openspine/issues/132)

### Selected proof

**Let Lyra prepare recurring drafts for one known relationship without asking the same approval every time.**

This is the smallest useful progression because it reuses the working Gmail proof, adds no new connector, and demonstrates the product's defining promise: repeated approved work can become narrow, budgeted, expiring, and revocable responsibility without permitting email send.

**Boundary:** One configured mailbox, one counterparty key, draft creation only, a small weekly volume budget, and a fixed expiry; recipient changes and email sending remain approval-required or denied.

**Proof sequence:**

1. The owner selects a Gmail thread and approves an exact draft through the existing flow.
2. After a repeated matching approval, Lyra proposes the responsibility in ordinary language, including mailbox, relationship, action, budget, expiry, and what stays blocked.
3. The owner narrows or approves the proposal; activation still uses the governed artifact lifecycle.
4. The next matching draft is created with a concise receipt instead of another approval prompt.
5. A changed recipient, exhausted budget, drift trigger, pause, expiry, or revocation returns the action for review or denies it.

Implementation is tracked in [issue #130](https://github.com/George-RD/openspine/issues/130).

<!-- capability-map:end -->

## Available through Lyra today

The current owner-facing alpha proves two bounded paths end to end.

### Direct local terminal conversation

- `openspine chat` provides a line-oriented local REPL, with `--once` for smoke tests and scripts;
- each message becomes a kernel-minted `cli.owner.message` verified with `local_cli_auth` on an owner-device channel;
- every turn receives a signed task grant and runs through the contained shell, model gateway, action gate, artifact store, and audit trail;
- the supplied Onyx configuration uses `LiquidAI/LFM2.5-1.2B-Instruct` first and registers `LiquidAI/LFM2.5-350M` as a smaller alternative;
- the Onyx PAT remains in the kernel, outside YAML and the worker environment;
- terminal grants permit only status, setup, approved model generation, and `terminal.reply:owner_device`;
- no Telegram bot token is required for terminal mode.

### Selected Gmail draft

- verified Telegram owner control;
- `/draft <thread_id>` for one Gmail thread selected by the owner;
- a live, attachment-free Gmail read bound to a single-use selection token;
- reply drafting through the model gateway;
- a Telegram preview of the exact proposed draft;
- digest-bound approval of the payload and target;
- Gmail draft creation after approval;
- email sending denied by global runtime policy;
- encrypted artifacts, task-grant records, gate decisions, and audit receipts.

These are working first-rung trust proofs. They are not yet the employee-like personal-assistant experience OpenSpine is designed to become.

## Landed in the OpenSpine runtime

The change ledger records substantially more machinery than the public owner paths expose.

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

The capability map closes the status ambiguity between landed machinery and usable owner paths. Three owner-facing gaps remain.

### [Install OpenSpine as an assistant system](https://github.com/George-RD/openspine/issues/117)

Ship the intended package model: a transactional package store, an explicit selected package, and a coherent `install lyra` / `use lyra` / `run` flow. This resolves the current ambiguity between OpenSpine the installed system, Lyra the default assistant, and OpenSpine the reusable runtime.

### [Ship a first-run trust loop](https://github.com/George-RD/openspine/issues/118)

Treat onboarding as product work now, not as polish deferred until a second deployment exists. Preflight the full setup, replace copied thread IDs with a minimal selector, show the task boundary, and finish with a compact receipt.

### [Ship the progressive delegation loop](https://github.com/George-RD/openspine/issues/123)

Join the landed runtime pieces into the product's defining experience:

1. the owner delegates one bounded job;
2. Lyra completes internal work and asks only at a real boundary;
3. repeated approvals, corrections, or preferences produce a plain-language proposal;
4. the owner approves, narrows, edits, rejects, pauses, expires, or revokes the reusable responsibility;
5. future matching work needs less interruption;
6. drift, changed context, or exhausted budgets return the responsibility for review.

This is the missing longitudinal product surface. It should feel like managing a good employee, not editing YAML or learning the runtime ontology.

## Deliberate current limits

- **No email send.** The current Lyra policy denies it in every grant and approval state.
- **No arbitrary third-party assistant compatibility.** OpenSpine loads Lyra as its default package; a general adapter surface is not shipped.
- **No broad inbox selection.** The owner currently supplies a Gmail thread ID.
- **No consumer-grade setup.** The supported path is for technical self-hosters.
- **No complete progressive-delegation owner loop.** Runtime support exists, but the seamless interaction is still product work.
- **No silent capability growth.** Learning and proposals do not activate authority by themselves.
- **No claim that every archived kernel component is a finished Lyra workflow.**

## Product direction

The north star remains a chief-of-staff-style personal AI that grows through reviewed delegation:

- start with one clear, bounded job;
- do internal work without interrupting the owner;
- ask at a real effect or disclosure boundary;
- turn repeated decisions into reviewable, revocable responsibility after explicit confirmation;
- commission contained workers with only the context and authority they need;
- learn preferences without turning memory into permission;
- use budgets, expiry, drift review, pause, and revocation to keep autonomy understandable;
- show concise receipts and exceptions rather than constant approval prompts.

The next product work should make that working relationship visible without weakening the runtime invariants. Breadth is useful only when responsibility remains clear and reversible.

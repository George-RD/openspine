# Proposal: Implement durable workflow replay

## Dependencies

- `implement-event-bus-subscriptions` (archived): audit-ledger aggregate coordinates and validated replay.
- AD-104: deterministic workflow state machines replay recorded outside-world results.
- AD-012: dark-window grants consume the kernel timer substrate.

## Problem

Workflow execution needs durable step identity across crashes and concurrent contexts. A crash or stale context must not silently re-run a recorded effect, create a second timer, or return a live failure that replay cannot reproduce.

## Proposed Solution

Add a kernel `WorkflowCtx` backed by the `workflow_run:{run_id}` audit aggregate. `begin_step` creates an exact `StepHandle` for the persisted `Pending` row. Completion, receipts, and replay claims use that handle's stored sequence rather than kind/order lookups. Timer scheduling uses one SQLite immediate transaction to CAS the exact step registry row, append the canonical `Completed` event, and insert the timer registry row; all callers receive the winner's canonical `TimerSpec`. Timer firing uses the trusted kernel clock and `fires_at` predicate with a status CAS.

Outside-world failures append terminal `Outcome::Err` values before returning. A completion append error propagates as a typed failure; a successful terminal failure returns the same closed/non-sensitive code that recovery replays, and recovery never redispatches a Pending gated effect without a receipt. Gated replay identity binds action, grant, bound chat, actual payload, and step-input digest. Raw dispatch is private and reachable only through the mediator. Approval steps use a typed adapter requiring action, target digest, and payload digest; generic approval-kind rows are rejected. Private errors use a closed non-sensitive code; private success values are encrypted artifact references and inline payload types are sealed.

Audit verification and workflow replay share one locked snapshot and the event-bus row/event consistency validator. Recovery tests cover durable Pending and Resuming states, and timer tests cover pre-deadline no-op, trusted time, and single firing.

## Acceptance Criteria

- Kill/crash recovery reaches the same final state without duplicate effects or redispatch.
- Recorded model/connector/approval steps and failures are replayed without invoking providers again.
- Time and randomness replay identically.
- Timer scheduling converges to one canonical spec across contexts; pre-deadline polls are not durable and due firing is emitted once.
- All workflow facts remain audit-ledger events replayable through `store::event_bus`.

## Ratified decisions

The parent adjudicated this change's candidate decision text as **D-073** (durable workflow steps persist intent before effect; recovery replays recorded outcomes and fails closed on receiptless pending non-idempotent effects; sealed inline payload set) and **D-074** (kernel-owned workflow timers fire at most once via trusted-clock atomic claims), recorded in `.raw/openspine-decision-log.md`.
## Out of Scope

Task-board and standing-rule consumers, distributed multi-process workflow ownership, and a projection framework remain deferred.

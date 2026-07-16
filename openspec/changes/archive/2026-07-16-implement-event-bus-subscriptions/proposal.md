# Proposal: Implement event bus subscriptions

## Dependencies

None. This is a leaf of the Event substrate group (AD-105) and is itself a
prerequisite of `implement-durable-workflow-replay`, the task board, nerves, and
master-agent changes.

## Problem/Context

AD-105 settles the event bus as **the existing event-sourced audit store with
typed subscriptions** — not a separate broker:

> No separate broker: events append to the ledger BEFORE consumers act;
> consumers (master, advisor, miner, gate feedback) subscribe to filtered
> streams. Requirements: unique event IDs + per-aggregate sequence numbers
> (idempotent consumers). Rebuildable CQRS-style projections: principle noted
> (don't design state that CAN'T be rebuilt from the stream), machinery
> deferred — no projection framework at n=1. (settled, scale-parts deferred)

Today the kernel has a hash-chained `audit_log` (`AuditEvent`) that is
append-only and verified on startup, and an inbound `EventEnvelope` used for
channel-activity normalization. There is no typed filter, no ordered replay
API, no per-aggregate sequence, and no consumer checkpoint/ack contract.
Downstream work (durable workflow replay, nerves, task board) needs that
substrate before it can build on it.

## Proposed Solution

Build subscription/replay **on top of the existing `audit_log` ledger** — no
parallel event store, no live pub/sub broker, no projection framework.

1. **Ledger is the bus.** Every durable runtime fact continues to enter via
   `append_audit` / `append_audit_conn`. Append is synchronous under the store
   lock: the row is durable before the call returns. Consumers never observe an
   event that is not already in the ledger, because the only delivery path is
   ordered replay of ledger rows.
2. **Unique IDs + per-aggregate sequence.** `AuditEvent` already carries a ULID
   `id`. Extend it (and the `audit_log` row) with `aggregate_id` and
   `aggregate_seq`. Sequence is assigned atomically inside the same connection
   lock as the insert (`MAX(aggregate_seq)+1` for that aggregate). Default
   aggregate policy: `task_grant:{id}` when a task-grant is present on the
   append, otherwise `"system"`.
3. **Typed filtered subscriptions.** A pure filter type
   (`EventSubscriptionFilter`: optional kind allowlist + optional aggregate_id)
   and a store `replay_audit(filter, after_global_seq)` that returns matching
   rows ordered by global `audit_log.seq`.
4. **Idempotent-consumer contract.** An `IdempotentConsumer` holds a
   `consumer_id`, a filter, and a checkpoint (`last_acked_global_seq`). Replay
   walks matching rows after the checkpoint; the handler runs; the checkpoint
   advances **only after successful handling** (never on publish, never before
   the handler returns). Double-replay of the same stream yields the same
   terminal consumer state. Checkpoint is optionally persisted in
   `consumer_checkpoints` so a process restart resumes correctly.

## Acceptance Criteria

- Events are durable in `audit_log` before any consumer can act on them (the
  only delivery path is post-append ledger replay).
- `AuditEvent` has unique IDs and monotonic per-aggregate sequence numbers;
  sequences for distinct aggregates are independent.
- A typed filter selects by kind and/or aggregate; replay is deterministic
  (global `seq` order).
- An idempotent consumer, under test, replays a filtered stream twice and
  reaches the same terminal state (second pass is a pure no-op).
- No projection framework, no live broker/channel fan-out, no second event
  store.

## Out of Scope

- CQRS projection framework / rebuildable read models machinery (AD-105 scale
  note: principle only at n=1).
- Live push subscriptions, multi-process brokers, or out-of-process message
  queues.
- Wiring specific consumers (master, advisor, miner, nerves) — they arrive in
  later changes that *require* this one.
- Changing the inbound channel `EventEnvelope` shape or the pipeline's
  identity/routing path.
- Altering hash-chain verification semantics beyond folding the new aggregate
  fields into the hashed meta pre-image for *new* rows.

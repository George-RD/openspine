# event-bus Specification

## Purpose
TBD - created by archiving change implement-event-bus-subscriptions. Update Purpose after archive.
## Requirements
### Requirement: The event bus MUST be the append-only audit ledger with no parallel store

The kernel event bus MUST be the existing hash-chained `audit_log` ledger
(`AuditEvent` rows). The kernel MUST NOT introduce a second durable event store
or an out-of-process message broker for bus delivery.

Every bus event MUST be durable in `audit_log` before any consumer can observe
it. The only delivery path for consumers MUST be ordered replay of ledger rows
that have already been appended.

#### Scenario: Append is durable before consumer observation

Given a caller invokes the audit append path
When the append call returns successfully
Then the resulting `AuditEvent` MUST be readable from the ledger via filtered
replay
And no consumer delivery path MUST exist that can observe the event before the
append returns.

### Requirement: Bus events MUST carry unique IDs and per-aggregate sequence numbers

Every `AuditEvent` MUST have a unique event ID.

Every `AuditEvent` MUST carry an `aggregate_id` and a per-aggregate
`aggregate_seq`.

For a given `aggregate_id`, `aggregate_seq` values MUST be strictly monotonic
positive integers assigned at append time under the same store lock as the
insert (no gaps under single-writer store semantics).

When a task grant is associated with the append, the default `aggregate_id`
MUST be derived from that grant; otherwise the default MUST be the system
aggregate.

Distinct aggregates MUST maintain independent sequence counters.

#### Scenario: Two aggregates sequence independently

Given audit events are appended for aggregate A and aggregate B
When the ledger is inspected
Then events for A MUST have `aggregate_seq` 1, 2, 3, … in append order for A
And events for B MUST have `aggregate_seq` 1, 2, 3, … in append order for B
And every event ID MUST be unique across both aggregates.

### Requirement: Consumers MUST subscribe via typed filters and ordered ledger replay

A subscription MUST be expressed as a typed filter over audit kind and/or
`aggregate_id`.

Filtered replay MUST return matching ledger rows in deterministic global
ledger order (ascending global sequence).

A filter with no kind constraint MUST match all kinds; a filter with no
aggregate constraint MUST match all aggregates.

#### Scenario: Kind filter selects a subset in order

Given the ledger contains events of kinds `authority.granted`,
`action.gated`, and `artifact.activated` in that global order
When a consumer replays with a kind filter of `action.gated` only
Then the consumer MUST observe only the `action.gated` event
And observation order MUST match global ledger order among matches.

### Requirement: Consumers MUST be idempotent and ack only after successful handling

An idempotent consumer MUST track a checkpoint of the last successfully
handled global ledger sequence.

Replay MUST invoke the consumer handler only for matching events after the
checkpoint.

The checkpoint MUST advance for an event only after the handler returns
success for that event. The checkpoint MUST NOT advance at append/publish
time, and MUST NOT advance when the handler fails.

Replaying the same filtered stream twice through an idempotent consumer MUST
yield the same terminal consumer state (the second pass is a no-op for already
acked events).

#### Scenario: Double filtered replay is a pure no-op

Given a ledger with multiple events matching a consumer's filter
And the consumer has already completed one successful replay over that filter
When the consumer replays the same filter a second time without new events
Then the consumer's terminal state MUST equal the state after the first replay
And the handler MUST NOT be applied to any already-acked event.

#### Scenario: Failed handling does not advance the checkpoint

Given a matching event after the consumer's checkpoint
When the handler returns failure for that event
Then the consumer checkpoint MUST remain unchanged
And a subsequent replay MUST present the same event again.

The v1 store posture is single-process/single-Store-writer; callers MUST serialize audit appends through one Store instance.

Delivery contract: the event bus provides **AT-LEAST-ONCE** delivery. Durable consumers own crash-safe idempotence for their handler state; the watermark prevents successful replay from repeating already acknowledged rows.


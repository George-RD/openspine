# Spec: Audit artifact store (event bus ledger extensions)

## ADDED Requirements

### Requirement: Audit append MUST assign per-aggregate sequence under the store lock

When an audit row is appended, the store MUST assign `aggregate_seq` as one
greater than the current maximum `aggregate_seq` for that row's
`aggregate_id`, using the same connection/lock that performs the insert.

The assigned `aggregate_id` and `aggregate_seq` MUST be stored as columns on
`audit_log` and MUST be included in the hash-chain meta pre-image for the new
row.

#### Scenario: Sequential appends on one aggregate

Given no prior rows for aggregate `system`
When two audit rows are appended without a task grant
Then both MUST use `aggregate_id = "system"`
And their `aggregate_seq` values MUST be 1 then 2.

### Requirement: The store MUST support filtered ordered replay of the audit ledger

The store MUST expose a filtered replay API over `audit_log` that returns
matching rows in ascending global sequence order, optionally starting after a
caller-supplied global sequence watermark.

The store MUST support durable consumer checkpoints keyed by consumer id,
recording the last successfully acked global sequence.

#### Scenario: Replay after watermark skips earlier rows

Given three audit rows with global sequences 1, 2, and 3
When replay is requested with watermark 2 and no filter constraints
Then only the row with global sequence 3 MUST be returned.

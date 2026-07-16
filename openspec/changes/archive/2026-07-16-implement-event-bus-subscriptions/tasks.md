# Tasks: Implement event bus subscriptions

## 1. Schemas

- [x] Extend `AuditEvent` with `aggregate_id: String` and `aggregate_seq: u64`
  (`#[serde(default)]` so older rows still deserialize).
- [x] Add `openspine-schemas/src/event_bus.rs`: `EventSubscriptionFilter`,
  `ConsumerCheckpoint`. Register in `lib.rs`.
- [x] Update in-crate `AuditEvent` serde tests for the new fields.

## 2. Ledger write path

- [x] `SCHEMA_SQL`: add `aggregate_id`, `aggregate_seq` on `audit_log`, index
  `idx_audit_aggregate`, and `consumer_checkpoints` table.
- [x] `migrations.rs`: `add_column_if_missing` for both new columns on existing
  DBs (duplicate-column = success).
- [x] `audit_support.rs`: resolve default aggregate, assign
  `MAX(aggregate_seq)+1` under the connection lock, fold fields into hashed
  meta + `AuditEvent` + INSERT columns.

## 3. Replay + idempotent consumer

- [x] `store/event_bus.rs` (new, registered): `replay_audit`,
  `load_consumer_checkpoint`, `save_consumer_checkpoint`, `IdempotentConsumer`
  with `replay` that advances the checkpoint **only after** the handler
  returns `Ok`.
- [x] Single-line `mod event_bus;` in `store/mod.rs`.

## 4. Tests

- [x] Ledger-append-before-consume: after `append_audit` returns, the row is
  visible via `replay_audit` before any consumer runs.
- [x] Filtered replay is idempotent: mixed kinds/aggregates, filtered
  consumer, double `replay` → identical terminal state; second pass is a no-op.
- [x] Unique event IDs + per-aggregate sequences: two aggregates, independent
  1..N sequences, all IDs unique; system aggregate for grant-less appends.
- [x] Failed handler does not advance the checkpoint (event is retried).

## 5. Validation

- [x] `cargo fmt && cargo fmt --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`
- [x] `bash scripts/check-file-sizes.sh` (all `crates/**/*.rs` ≤ 500 lines)
- [x] `openspec validate implement-event-bus-subscriptions --strict`

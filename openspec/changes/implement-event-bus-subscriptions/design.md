# Design: Implement event bus subscriptions

## Approach

AD-105 is explicit: the event bus **is** the event-sourced audit store with
typed subscriptions. This change adds the missing read-side substrate on the
existing ledger and the minimum write-side fields (aggregate identity +
sequence) needed for idempotent consumers. No second store. No live broker.

### 1. Extend `AuditEvent` — not a parallel envelope

`AuditEvent` already has a unique ULID `id` and is the persisted ledger row
shape. Add:

- `aggregate_id: String` — logical stream key. Default policy at append time:
  - `task_grant:{ulid}` when `task_grant_id` is `Some`
  - `"system"` otherwise
- `aggregate_seq: u64` — monotonic per `aggregate_id`, assigned inside the
  append path under the existing connection lock.

Both fields are folded into the hashed `meta` pre-image so they cannot be
silently rewritten without breaking `verify_audit_chain`. They carry
`#[serde(default)]` so older on-disk `event_json` rows still deserialize
(defaults: `"system"` / `0`). Sequence `0` is a **legacy sentinel only** —
the positive, gap-free per-aggregate guarantee applies to rows written by
the post-AD-105 append path (`aggregate_seq >= 1`). Hash-chained historical
payloads are never rewritten.

`kind` is a nominal [`AuditKind`] newtype (serde-transparent string,
non-empty): the vocabulary stays open (D-013) but subscription filters take
`AuditKind`, not free text.

`EventEnvelope` is intentionally **not** the bus event. It is the inbound
channel-activity shape used before identity/routing; the durable facts that
consumers care about (gate decisions, grants, activations, …) are already
`AuditEvent`s. Building a parallel `UnifiedEvent` wrapper would invent a second
ledger and fight AD-105.

### 2. Schema: columns on `audit_log`, not a new events table

`SCHEMA_SQL` gains:

```sql
-- on audit_log:
aggregate_id  TEXT    NOT NULL DEFAULT 'system',
aggregate_seq INTEGER NOT NULL DEFAULT 0
CREATE INDEX IF NOT EXISTS idx_audit_aggregate
  ON audit_log (aggregate_id, aggregate_seq);
CREATE TABLE IF NOT EXISTS consumer_checkpoints (
    consumer_id TEXT PRIMARY KEY,
    last_acked_global_seq INTEGER NOT NULL DEFAULT 0,
    checkpoint_json TEXT NOT NULL
);
```

Existing on-disk DBs pick up the new columns via the established ad-hoc
migration helper `add_column_if_missing` in `store/migrations.rs` (SQLite
`ALTER TABLE ... ADD COLUMN` with "duplicate column name" treated as success).
No `information_schema`, no `IF NOT EXISTS` on `ADD COLUMN` (not portable
SQLite), no `PRAGMA user_version` (owned by a later day-2 ops change).

### 3. Append path — sequence under the same lock as the insert

`append_audit_conn` already holds the caller's connection (and
`append_audit` holds the store mutex). Inside that critical section:

1. Resolve `aggregate_id` from the default policy (or an explicit override
   when a later caller needs one — v1 uses the default only).
2. `SELECT MAX(aggregate_seq) FROM audit_log WHERE aggregate_id = ?` → `+ 1`
   (NULL → 1).
3. Build meta (including `aggregate_id` / `aggregate_seq`), hash, insert the
   full row, return the `AuditEvent`.

Because the mutex serializes all store access, the max+insert pair cannot
race. Append returns only after the `INSERT` succeeds — that is the
"ledger-before-consume" guarantee: there is no in-memory fan-out that could
deliver an event before the row exists.

Existing `append_audit` call sites keep their signature; they inherit the
default aggregate policy with zero churn.

### 4. Typed filter + ordered replay (read path)

New pure types in `openspine-schemas` (`event_bus` module):

```rust
pub struct EventSubscriptionFilter {
    /// None = all kinds; Some(list) = match any of these `AuditEvent.kind`s.
    pub kinds: Option<Vec<String>>,
    /// None = all aggregates; Some(id) = that aggregate only.
    pub aggregate_id: Option<String>,
}

pub struct ConsumerCheckpoint {
    pub last_acked_global_seq: u64, // 0 = nothing acked
}
```

New store module `store/event_bus.rs`:

- `Store::replay_audit(filter, after_global_seq) -> Vec<(u64 /*global seq*/, AuditEvent)>`
  reads `audit_log` ordered by `seq ASC`, applies the filter, returns only rows
  with `seq > after_global_seq`. Filtering by multi-kind is done in Rust after
  a `seq`/`aggregate_id`-scoped query so the SQL stays simple and index-friendly.
- `Store::load_consumer_checkpoint` / `save_consumer_checkpoint` for the
  optional durable checkpoint table.

No subscriptions table. No channels. No `context.Context`. A "subscription" is
a filter value plus a consumer that replays against it.

### 5. Idempotent consumer — ack only after successful handling

```rust
pub struct IdempotentConsumer {
    pub consumer_id: String,
    pub filter: EventSubscriptionFilter,
    checkpoint: ConsumerCheckpoint,
}

impl IdempotentConsumer {
    /// Replay matching ledger rows after the checkpoint through `handler`.
    /// The checkpoint advances for an event ONLY after `handler` returns Ok.
    /// A failed handler leaves the checkpoint unmoved so the event is retried.
    pub fn replay<F, S, E>(&mut self, store: &Store, state: &mut S, handler: F)
        -> Result<(), ConsumerError>
    where F: FnMut(&mut S, &AuditEvent) -> Result<(), E>, E: Display;
}
```

Contract:

- Delivery order = global `audit_log.seq` among rows matching the filter.
- Primary dedup = global sequence watermark (`last_acked_global_seq`).
- Defense in depth = in-process `seen_event_ids` set (unique event IDs); a
  duplicate id already handled in this process does not re-invoke the handler.
- A durable `consumer_id` is **bound to a fixed filter** for the lifetime of
  its checkpoint; loading the same id with a different filter fails closed
  (avoids silently skipping earlier matching events after a filter change).
- **Never** advance the checkpoint on publish / at append time. Doing so would
  mark work processed before the consumer acts and let a crash skip it.
- Double-replay: first pass applies handler to each new event and advances;
  second pass sees `seq <= last_acked` and is a pure no-op → identical terminal
  `state`.

Persistence of the checkpoint is optional for pure in-process tests; the
store helpers exist so a restarted kernel can resume.

### 6. File layout (rebase-friendly)

| File | Role |
|------|------|
| `openspine-schemas/src/audit.rs` | add `aggregate_id` / `aggregate_seq` |
| `openspine-schemas/src/event_bus.rs` | filter + checkpoint types (new) |
| `openspine-kernel/src/store/event_bus.rs` | replay, checkpoint, consumer (new) |
| `openspine-kernel/src/store/audit_support.rs` | sequence assignment in append |
| `openspine-kernel/src/store/mod.rs` | `SCHEMA_SQL` columns + `mod event_bus` |
| `openspine-kernel/src/store/migrations.rs` | `add_column_if_missing` for the two cols |

Single-line registrations only in shared files. No action-catalog or pipeline
wiring — consumers arrive later.

## Key decisions

- **Ledger = bus.** AD-105 forbids a separate broker; reusing `audit_log`
  keeps one source of truth for "why did Lyra do X" and for consumer replay.
- **Ack after success, never on publish.** Checkpoint-on-publish is a silent
  correctness bug under crash; the acceptance test is double-replay identity.
- **Default aggregate from `task_grant_id`.** Avoids churning every
  `append_audit` call site while still giving domain events a real aggregate
  stream. Explicit override can be added later without a schema change.
- **No live push.** Nerves and workflow recovery are pull/replay-shaped
  (AD-104/AD-130); push can be layered later without changing the ledger.
- **Projection framework deferred.** AD-105's scale note is binding; this
  change only makes state *rebuildable in principle* (filter + ordered replay).

## Alternatives considered

- **Parallel `events` table / `UnifiedEvent` wrapper:** rejected — second
  ledger, fights AD-105, duplicates the hash-chain problem.
- **Checkpoint advanced at append/publish time:** rejected — skips unprocessed
  work after a consumer crash.
- **Live channel/broker fan-out:** rejected — not required at n=1; complicates
  "ledger before consume" and multi-process semantics.
- **Put aggregate fields only in DB columns, not `AuditEvent`:** rejected —
  consumers must see sequence numbers; hash-chain integrity requires them in
  the hashed meta.
- **`PRAGMA user_version` migrations:** deferred to day-2 ops; use the
  existing `add_column_if_missing` helper.

## Authority sensitivity

Authority-sensitive (audit / recovery). Invariants preserved:

- **D-004 deny-by-default:** the bus grants no authority; it is a read-side
  view of already-audited facts. Subscriptions cannot authorize effects.
- **D-005/D-010 kernel/shell split:** all ledger I/O stays in the kernel store;
  shells never write the audit chain.
- **D-006 identity ≠ authority:** no identity/principal fields on the bus.
- **D-007 grant is the only live authority:** unchanged.
- **D-008 deterministic routing:** replay order is deterministic (global seq).
- **D-011 digest-bound approval:** unchanged; approvals remain on their own
  path and continue to append audit rows as today.
- **D-012 audit chain:** new fields are inside the hashed meta for new rows;
  verification walk is unchanged.

## What does NOT move

- `EventEnvelope` and the inbound pipeline (telegram/gmail → envelope →
  identity → route → grant).
- Gate semantics, action catalog, capability packs.
- Hash-chain verification algorithm and genesis digest.
- Existing `append_audit` call sites (signature preserved).

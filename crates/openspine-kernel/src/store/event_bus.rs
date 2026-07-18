//! Event-bus read path over the audit ledger (AD-105).
//!
//! The bus is the `audit_log` table: append stays in `audit_support`, and this
//! module provides typed filtered replay plus the idempotent-consumer contract.
//! No live broker, no parallel event store, no projection framework.
//!
//! Kernel-internal event-bus API for future consumers (nerves, workflow
//! recovery, task board). Domain projections are out of scope for AD-105;
//! this module exports the typed filter + ordered replay + idempotent
//! consumer substrate those later changes require.

use super::{Store, StoreError};
use openspine_schemas::audit::AuditEvent;
use openspine_schemas::event_bus::{ConsumerCheckpoint, EventSubscriptionFilter};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use std::fmt;
use ulid::Ulid;

/// One ledger row as seen by a bus consumer: global sequence + event.
#[derive(Debug, Clone, PartialEq, Eq)]
// AD-105 substrate type; re-exported for kernel consumers.
#[allow(dead_code)]
pub struct LedgerEntry {
    /// Global `audit_log.seq` (append order across all aggregates).
    pub global_seq: u64,
    pub event: AuditEvent,
}
type ReplayRow = (
    i64,
    String,
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
);

/// Errors from idempotent consumer replay.
#[derive(Debug, thiserror::Error)]
// AD-105 substrate type; re-exported for kernel consumers.
#[allow(dead_code)]
pub enum ConsumerError {
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("handler failed at global_seq {global_seq}: {message}")]
    Handler { global_seq: u64, message: String },
    #[error(
        "consumer_id {consumer_id:?} was checkpointed with a different filter; \
         consumer_id is bound to a fixed filter for the lifetime of its checkpoint"
    )]
    FilterMismatch { consumer_id: String },
}

/// Durable checkpoint payload: watermark + the filter the consumer is bound to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedConsumerState {
    pub(crate) schema_version: u32,
    pub(crate) checkpoint: ConsumerCheckpoint,
    pub(crate) filter: EventSubscriptionFilter,
}

impl Store {
    /// Ordered filtered replay of the audit ledger.
    ///
    /// Returns rows with `global_seq > after_global_seq` that match `filter`,
    /// in ascending global sequence. Matching is applied in Rust after a
    /// `seq`-scoped (and optional aggregate-scoped) query so the SQL stays
    /// index-friendly for multi-kind filters.
    // AD-105: substrate entry point; domain consumers land in later changes.
    #[allow(dead_code)]
    pub(crate) fn replay_audit(
        &self,
        filter: &EventSubscriptionFilter,
        after_global_seq: u64,
    ) -> Result<Vec<LedgerEntry>, StoreError> {
        let after_global_seq_i64 =
            i64::try_from(after_global_seq).map_err(|_| StoreError::NumericRange)?;
        let conn = self.conn.lock();
        Self::replay_audit_conn(&conn, filter, after_global_seq_i64)
    }

    /// Replay the audit ledger over an already-locked `Connection`, applying
    /// the same row/event coordinate and consistency validation as
    /// [`Self::replay_audit`]. Exposed so callers that already hold the lock
    /// (e.g. an atomic verify+replay snapshot) reuse the exact same path.
    // AD-105: substrate entry point; domain consumers land in later changes.
    #[allow(dead_code)]
    pub(crate) fn replay_audit_conn(
        conn: &Connection,
        filter: &EventSubscriptionFilter,
        after_global_seq: i64,
    ) -> Result<Vec<LedgerEntry>, StoreError> {
        let (sql, bind_agg): (&str, Option<&str>) = match filter.aggregate_id.as_deref() {
            Some(agg) => (
                "SELECT seq, event_json, meta_json, id, kind, aggregate_id, aggregate_seq, prev_hash, hash FROM audit_log \
                 WHERE seq > ?1 AND aggregate_id = ?2 \
                 ORDER BY seq ASC",
                Some(agg),
            ),
            None => (
                "SELECT seq, event_json, meta_json, id, kind, aggregate_id, aggregate_seq, prev_hash, hash FROM audit_log \
                 WHERE seq > ?1 \
                 ORDER BY seq ASC",
                None,
            ),
        };

        let mut stmt = conn.prepare(sql)?;
        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<ReplayRow> {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
            ))
        };
        let rows: Vec<ReplayRow> = match bind_agg {
            Some(agg) => {
                let mapped = stmt.query_map(params![after_global_seq, agg], map_row)?;
                mapped.collect::<Result<Vec<_>, _>>()?
            }
            None => {
                let mapped = stmt.query_map(params![after_global_seq], map_row)?;
                mapped.collect::<Result<Vec<_>, _>>()?
            }
        };

        let mut out = Vec::with_capacity(rows.len());
        for (
            seq,
            event_json,
            meta_json,
            row_id,
            row_kind,
            row_aggregate,
            row_aggregate_seq,
            row_prev_hash,
            row_hash,
        ) in rows
        {
            let meta: serde_json::Value = serde_json::from_str(&meta_json)?;
            let meta_id = meta.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            let meta_kind = meta
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let meta_aggregate = meta
                .get("aggregate_id")
                .and_then(|v| v.as_str())
                .unwrap_or("system");
            let meta_seq = meta
                .get("aggregate_seq")
                .and_then(|v| v.as_u64())
                .unwrap_or_default();
            if meta_id != row_id
                || meta_kind != row_kind
                || meta_aggregate != row_aggregate
                || meta_seq != row_aggregate_seq as u64
            {
                return Err(StoreError::BadLedgerMeta(format!(
                    "ledger row {seq} metadata mismatch"
                )));
            }
            let event: AuditEvent = serde_json::from_str(&event_json)?;
            if event.schema_version != 1
                || event.id.to_string() != row_id
                || event.kind.as_str() != row_kind
                || event.aggregate_id != row_aggregate
                || event.aggregate_seq != row_aggregate_seq as u64
                || event.prev_hash.as_str() != row_prev_hash
                || event.hash.as_str() != row_hash
            {
                return Err(StoreError::BadLedgerMeta(format!(
                    "ledger row {seq} event_json mismatch"
                )));
            }
            // Every delivered field that exists in hashed metadata must agree;
            // event_json is a redundant cache, never an authority.
            let event_value = serde_json::to_value(&event)?;
            for field in [
                "id",
                "ts",
                "kind",
                "action",
                "decision",
                "reason",
                "task_grant_id",
                "target_refs",
                "payload_refs",
                "aggregate_id",
                "aggregate_seq",
                "payload_json",
            ] {
                let normalized = match field {
                    "aggregate_id" => meta
                        .get(field)
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!("system")),
                    "aggregate_seq" => meta
                        .get(field)
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!(0)),
                    _ => meta.get(field).cloned().unwrap_or(serde_json::Value::Null),
                };
                if event_value.get(field) != Some(&normalized) {
                    return Err(StoreError::BadLedgerMeta(format!(
                        "ledger row {seq} event_json field {field} mismatch"
                    )));
                }
            }
            if filter.matches(&event.kind, &event.aggregate_id) {
                out.push(LedgerEntry {
                    global_seq: seq as u64,
                    event,
                });
            }
        }
        Ok(out)
    }

    // AD-105: substrate entry point; domain consumers land in later changes.
    #[allow(dead_code)]
    pub(crate) fn load_consumer_checkpoint(
        &self,
        consumer_id: &str,
    ) -> Result<Option<PersistedConsumerState>, StoreError> {
        let conn = self.conn.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT checkpoint_json FROM consumer_checkpoints WHERE consumer_id = ?1",
                params![consumer_id],
                |row| row.get(0),
            )
            .optional()?;
        match json {
            Some(j) => Ok(Some(serde_json::from_str(&j)?)),
            None => Ok(None),
        }
    }

    // AD-105: substrate entry point; domain consumers land in later changes.
    #[allow(dead_code)]
    pub(crate) fn save_consumer_checkpoint(
        &self,
        consumer_id: &str,
        state: &PersistedConsumerState,
    ) -> Result<(), StoreError> {
        let checkpoint_i64 = i64::try_from(state.checkpoint.last_acked_global_seq)
            .map_err(|_| StoreError::NumericRange)?;
        let conn = self.conn.lock();
        let existing: Option<(i64, String)> = conn.query_row(
            "SELECT last_acked_global_seq, checkpoint_json FROM consumer_checkpoints WHERE consumer_id = ?1",
            params![consumer_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        if let Some((old_seq, old_json)) = existing {
            let old: PersistedConsumerState = serde_json::from_str(&old_json)?;
            if old.filter != state.filter {
                return Err(StoreError::CheckpointFilterMismatch(
                    consumer_id.to_string(),
                ));
            }
            if old_seq as u64 > state.checkpoint.last_acked_global_seq {
                return Err(StoreError::CheckpointRegression(consumer_id.to_string()));
            }
            let json = serde_json::to_string(state)?;
            let changed = conn.execute(
                "UPDATE consumer_checkpoints SET last_acked_global_seq = ?1, checkpoint_json = ?2
                 WHERE consumer_id = ?3 AND checkpoint_json = ?4 AND last_acked_global_seq <= ?1",
                params![
                    state.checkpoint.last_acked_global_seq as i64,
                    json,
                    consumer_id,
                    old_json
                ],
            )?;
            if changed != 1 {
                return Err(StoreError::CheckpointRegression(consumer_id.to_string()));
            }
        } else {
            let json = serde_json::to_string(state)?;
            conn.execute("INSERT INTO consumer_checkpoints (consumer_id, last_acked_global_seq, checkpoint_json) VALUES (?1, ?2, ?3)", params![consumer_id, checkpoint_i64, json])?;
        }
        Ok(())
    }
}

#[allow(dead_code)]
/// Bind a consumer id to a filter before delivery begins. Nerve registration
/// uses this on the same SQLite transaction as its declaration row, so every
/// nerve rides the existing audit-ledger checkpoint substrate.
pub(crate) fn bind_consumer_conn(
    conn: &Connection,
    consumer_id: &str,
    filter: &EventSubscriptionFilter,
) -> Result<(), StoreError> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT checkpoint_json FROM consumer_checkpoints WHERE consumer_id = ?1",
            params![consumer_id],
            |row| row.get(0),
        )
        .optional()?;
    if existing.is_some() {
        return Err(StoreError::CheckpointRegression(consumer_id.to_string()));
    }
    let state = PersistedConsumerState {
        schema_version: 1,
        checkpoint: ConsumerCheckpoint::default(),
        filter: filter.clone(),
    };
    conn.execute(
        "INSERT INTO consumer_checkpoints
         (consumer_id, last_acked_global_seq, checkpoint_json)
         VALUES (?1, 0, ?2)",
        params![consumer_id, serde_json::to_string(&state)?],
    )?;
    Ok(())
}

/// Idempotent consumer over a typed filter (AD-105).
///
/// Primary dedup is the global-sequence watermark (`last_acked_global_seq`).
/// Within a process, a `seen_event_ids` set is defense-in-depth against a
/// duplicate row with a new global seq (should not happen under the ledger
/// invariants, but keeps handlers pure-idempotent if it ever does).
///
/// Checkpoint advances **only after** the handler returns success for an
/// event. Double-replay of the same filtered stream is a pure no-op.
///
/// A `consumer_id` is bound to a fixed filter for the lifetime of its durable
/// checkpoint: loading the same id with a different filter fails closed.
#[derive(Debug, Clone)]
// AD-105: substrate entry point; domain consumers land in later changes.
#[allow(dead_code)]
pub struct IdempotentConsumer {
    pub consumer_id: String,
    pub filter: EventSubscriptionFilter,
    checkpoint: ConsumerCheckpoint,
    /// Event IDs successfully handled in this process (defense in depth).
    seen_event_ids: HashSet<Ulid>,
    /// When true, checkpoint is also written through
    /// [`Store::save_consumer_checkpoint`] after each successful ack.
    persist: bool,
}

impl IdempotentConsumer {
    #[allow(dead_code)] // AD-105 substrate API
    pub fn new(consumer_id: impl Into<String>, filter: EventSubscriptionFilter) -> Self {
        Self {
            consumer_id: consumer_id.into(),
            filter,
            checkpoint: ConsumerCheckpoint::default(),
            seen_event_ids: HashSet::new(),
            persist: false,
        }
    }

    /// Load a durable checkpoint from the store (if any) and enable
    /// persistence of subsequent acks. Fails if a checkpoint exists for this
    /// consumer_id with a different filter.
    #[allow(dead_code)] // AD-105 substrate API
    pub fn with_persisted_checkpoint(
        store: &Store,
        consumer_id: impl Into<String>,
        filter: EventSubscriptionFilter,
    ) -> Result<Self, ConsumerError> {
        let consumer_id = consumer_id.into();
        let (checkpoint, seen_event_ids) = match store.load_consumer_checkpoint(&consumer_id)? {
            Some(state) => {
                if state.filter != filter {
                    return Err(ConsumerError::FilterMismatch {
                        consumer_id: consumer_id.clone(),
                    });
                }
                (state.checkpoint, HashSet::new())
            }
            None => (ConsumerCheckpoint::default(), HashSet::new()),
        };
        Ok(Self {
            consumer_id,
            filter,
            checkpoint,
            seen_event_ids,
            persist: true,
        })
    }

    #[allow(dead_code)] // AD-105 substrate API
    pub fn checkpoint(&self) -> &ConsumerCheckpoint {
        &self.checkpoint
    }

    /// Replay matching ledger rows after the checkpoint through `handler`.
    ///
    /// For each entry in global-seq order:
    /// 1. Skip if `event.id` was already handled in this process (defense in depth).
    /// 2. Invoke `handler(state, &event)`.
    /// 3. On `Ok`, advance `last_acked_global_seq` and record the event id
    ///    (and optionally persist the watermark + filter).
    /// 4. On `Err`, leave the checkpoint unmoved and return
    ///    [`ConsumerError::Handler`] so a later call retries the same event.
    #[allow(dead_code)] // AD-105 substrate API
    pub fn replay<F, S, E>(
        &mut self,
        store: &Store,
        state: &mut S,
        mut handler: F,
    ) -> Result<(), ConsumerError>
    where
        F: FnMut(&mut S, &AuditEvent) -> Result<(), E>,
        E: fmt::Display,
    {
        let entries = store.replay_audit(&self.filter, self.checkpoint.last_acked_global_seq)?;
        for entry in entries {
            if self.seen_event_ids.contains(&entry.event.id) {
                // Defense in depth: already handled this id in-process.
                // Still advance the watermark past it so we do not stall.
                self.checkpoint.last_acked_global_seq = entry.global_seq;
                if self.persist {
                    self.persist_state(store)?;
                }
                continue;
            }
            if let Err(err) = handler(state, &entry.event) {
                return Err(ConsumerError::Handler {
                    global_seq: entry.global_seq,
                    message: err.to_string(),
                });
            }
            self.seen_event_ids.insert(entry.event.id);
            self.checkpoint.last_acked_global_seq = entry.global_seq;
            if self.persist {
                self.persist_state(store)?;
            }
        }
        Ok(())
    }

    fn persist_state(&self, store: &Store) -> Result<(), StoreError> {
        store.save_consumer_checkpoint(
            &self.consumer_id,
            &PersistedConsumerState {
                schema_version: 1,
                checkpoint: self.checkpoint.clone(),
                filter: self.filter.clone(),
            },
        )
    }
}

#[cfg(test)]
#[path = "event_bus_tests.rs"]
mod tests;

// Delivery is AT-LEAST-ONCE; durable consumers own crash-safe idempotence for handler state.

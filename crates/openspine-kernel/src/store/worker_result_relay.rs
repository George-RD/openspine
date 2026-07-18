//! Durable worker-result relay markers, retries, and checkpoint fencing.

use super::event_bus::PersistedConsumerState;
use super::{Store, StoreError};
use jiff::Timestamp;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use ulid::Ulid;

const MAX_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkerRelayClaim {
    /// A send should be performed for this attempt.
    Send { attempt: u32 },
    /// The event already reached a terminal marker (delivered, skipped, or
    /// dead-lettered) — the checkpoint was advanced on a prior run, so the
    /// caller must not re-send or re-advance.
    Terminal,
}

impl Store {
    /// Claim a relay attempt under BEGIN IMMEDIATE. The marker is durable
    /// before the provider call, so a restarted consumer reuses the same
    /// event-id idempotency key rather than creating an uncoordinated send.
    pub(crate) fn claim_worker_result_relay(
        &self,
        event_id: Ulid,
        global_seq: u64,
        task_grant_id: Option<Ulid>,
    ) -> Result<WorkerRelayClaim, StoreError> {
        let seq = i64::try_from(global_seq).map_err(|_| StoreError::NumericRange)?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(String, i64)> = tx
            .query_row(
                "SELECT state, attempts FROM worker_result_relays WHERE event_id = ?1",
                params![event_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let now = Timestamp::now().to_string();
        let claim = match row {
            None => {
                tx.execute(
                    "INSERT INTO worker_result_relays
                     (event_id, global_seq, task_grant_id, state, attempts, last_error, created_at, updated_at)
                     VALUES (?1, ?2, ?3, 'attempting', 1, NULL, ?4, ?4)",
                    params![
                        event_id.to_string(),
                        seq,
                        task_grant_id.map(|id| id.to_string()),
                        now,
                    ],
                )?;
                WorkerRelayClaim::Send { attempt: 1 }
            }
            Some((state, _attempts))
                if state == "delivered" || state == "skipped" || state == "dead_letter" =>
            {
                WorkerRelayClaim::Terminal
            }
            // A pre-existing `attempting`/`pending` marker (from a crashed
            // prior run) is RETRYABLE: re-send with an incremented attempt.
            // It is never treated as `Delivered` — a crash between claiming
            // and provider acceptance must not skip an unconfirmed handoff
            // (D-071 delivery-unknown; advisory: claim != delivered).
            Some((_state, attempts)) => {
                let next = attempts.saturating_add(1);
                tx.execute(
                    "UPDATE worker_result_relays
                     SET state='attempting', attempts=?2, updated_at=?3
                     WHERE event_id=?1",
                    params![event_id.to_string(), next, now],
                )?;
                WorkerRelayClaim::Send {
                    attempt: next as u32,
                }
            }
        };
        tx.commit()?;
        Ok(claim)
    }

    /// Mark a confirmed relay delivered and advance its checkpoint in the
    /// same BEGIN IMMEDIATE transaction. The event-id marker is the durable
    /// receiver handoff key used by retries and restart recovery.
    pub(crate) fn complete_worker_result_relay(
        &self,
        event_id: Ulid,
        global_seq: u64,
        state: &PersistedConsumerState,
    ) -> Result<(), StoreError> {
        self.finish_worker_result_relay(event_id, global_seq, "delivered", state)
    }

    /// Advance over a structural denial or malformed event, with an explicit
    /// durable skip marker and checkpoint commit in one transaction.
    pub(crate) fn skip_worker_result_relay(
        &self,
        event_id: Ulid,
        global_seq: u64,
        state: &PersistedConsumerState,
    ) -> Result<(), StoreError> {
        self.finish_worker_result_relay(event_id, global_seq, "skipped", state)
    }

    fn finish_worker_result_relay(
        &self,
        event_id: Ulid,
        global_seq: u64,
        relay_state: &str,
        state: &PersistedConsumerState,
    ) -> Result<(), StoreError> {
        let seq = i64::try_from(global_seq).map_err(|_| StoreError::NumericRange)?;
        let checkpoint_seq = i64::try_from(state.checkpoint.last_acked_global_seq)
            .map_err(|_| StoreError::NumericRange)?;
        let json = serde_json::to_string(state)?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(i64, String)> = tx
            .query_row(
                "SELECT last_acked_global_seq, checkpoint_json FROM consumer_checkpoints WHERE consumer_id = ?1",
                params!["worker_result_consumer"],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((old_seq, old_json)) = existing {
            let old: PersistedConsumerState = serde_json::from_str(&old_json)?;
            if old.filter != state.filter {
                return Err(StoreError::CheckpointFilterMismatch(
                    "worker_result_consumer".to_string(),
                ));
            }
            if old_seq > checkpoint_seq {
                return Err(StoreError::CheckpointRegression(
                    "worker_result_consumer".to_string(),
                ));
            }
            tx.execute(
                "UPDATE consumer_checkpoints SET last_acked_global_seq=?1, checkpoint_json=?2
                 WHERE consumer_id=?3 AND checkpoint_json=?4 AND last_acked_global_seq <= ?1",
                params![checkpoint_seq, json, "worker_result_consumer", old_json],
            )?;
        } else {
            tx.execute(
                "INSERT INTO consumer_checkpoints
                 (consumer_id, last_acked_global_seq, checkpoint_json) VALUES (?1, ?2, ?3)",
                params!["worker_result_consumer", checkpoint_seq, json],
            )?;
        }
        tx.execute(
            "INSERT INTO worker_result_relays
             (event_id, global_seq, task_grant_id, state, attempts, last_error, created_at, updated_at)
             VALUES (?1, ?2, NULL, ?3, 0, NULL, ?4, ?4)
             ON CONFLICT(event_id) DO UPDATE SET state=excluded.state, updated_at=excluded.updated_at",
            params![event_id.to_string(), seq, relay_state, Timestamp::now().to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Persist a transient failure. Attempts one through four remain pending;
    /// attempt five becomes a dead-letter row and advances the checkpoint only
    /// after its audit receipt is committed. The owner notification is
    /// enqueued in the SAME transaction as the dead-letter commit, so a crash
    /// between dead-letter and notification cannot skip the event permanently.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fail_worker_result_relay(
        &self,
        event_id: Ulid,
        global_seq: u64,
        attempt: u32,
        task_grant_id: Option<Ulid>,
        chat_id: i64,
        text_ref: &str,
        state: &PersistedConsumerState,
    ) -> Result<bool, StoreError> {
        if attempt >= MAX_ATTEMPTS && (chat_id == 0 || text_ref.is_empty()) {
            return Err(StoreError::OwnerNotificationFailed(
                "dead-letter requires a resolvable owner notification artifact".to_string(),
            ));
        }
        let seq = i64::try_from(global_seq).map_err(|_| StoreError::NumericRange)?;
        let now = Timestamp::now().to_string();
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if attempt < MAX_ATTEMPTS {
            tx.execute(
                "UPDATE worker_result_relays SET state='pending', last_error=?2, updated_at=?3
                 WHERE event_id=?1 AND state='attempting' AND attempts=?4",
                params![
                    event_id.to_string(),
                    "worker result relay failed",
                    now,
                    attempt as i64
                ],
            )?;
            Store::append_audit_conn(
                &tx,
                "worker.result.relay_failed",
                None,
                None,
                Some("worker result relay failed (retry pending)"),
                task_grant_id,
                &[],
                &[],
            )?;
            tx.commit()?;
            return Ok(false);
        }
        tx.execute(
            "UPDATE worker_result_relays SET state='dead_letter', last_error=?2, updated_at=?3
             WHERE event_id=?1 AND state='attempting' AND attempts=?4",
            params![
                event_id.to_string(),
                "worker result relay dead-lettered",
                now,
                attempt as i64
            ],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO worker_result_dead_letters
             (event_id, global_seq, task_grant_id, attempts, reason, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                event_id.to_string(),
                seq,
                task_grant_id.map(|id| id.to_string()),
                attempt,
                "worker result relay exhausted retries",
                now
            ],
        )?;
        // Enqueue the owner notification in the SAME transaction as the
        // dead-letter commit, so a crash after dead-letter but before
        // notification cannot skip the event permanently on restart.
        if chat_id != 0 && !text_ref.is_empty() {
            let ids = String::new();
            tx.execute(
                "INSERT INTO notify_dead_letters \
                 (id, enqueued_at, chat_id, text_ref, task_grant_id, digest_item_ids, attempts, next_attempt_at, state) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, 'pending')",
                params![
                    Ulid::new().to_string(),
                    now,
                    chat_id,
                    text_ref,
                    task_grant_id.map(|id| id.to_string()),
                    ids,
                    now,
                ],
            )?;
        }
        Store::append_audit_conn(
            &tx,
            "worker.result.relay_dead_letter",
            None,
            None,
            Some("worker result relay exhausted retries; dead-lettered"),
            task_grant_id,
            &[],
            &[],
        )?;
        let checkpoint_seq = i64::try_from(state.checkpoint.last_acked_global_seq)
            .map_err(|_| StoreError::NumericRange)?;
        let json = serde_json::to_string(state)?;
        let existing: Option<(i64, String)> = tx
            .query_row(
                "SELECT last_acked_global_seq, checkpoint_json FROM consumer_checkpoints WHERE consumer_id='worker_result_consumer'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match existing {
            Some((old_seq, old_json)) => {
                let old: PersistedConsumerState = serde_json::from_str(&old_json)?;
                if old.filter != state.filter {
                    return Err(StoreError::CheckpointFilterMismatch(
                        "worker_result_consumer".to_string(),
                    ));
                }
                if old_seq > checkpoint_seq {
                    return Err(StoreError::CheckpointRegression(
                        "worker_result_consumer".to_string(),
                    ));
                }
                tx.execute(
                    "UPDATE consumer_checkpoints SET last_acked_global_seq=?1, checkpoint_json=?2
                     WHERE consumer_id='worker_result_consumer' AND checkpoint_json=?3 AND last_acked_global_seq <= ?1",
                    params![checkpoint_seq, json, old_json],
                )?;
            }
            None => {
                tx.execute(
                    "INSERT INTO consumer_checkpoints
                     (consumer_id, last_acked_global_seq, checkpoint_json) VALUES ('worker_result_consumer', ?1, ?2)",
                    params![checkpoint_seq, json],
                )?;
            }
        }
        tx.commit()?;
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn worker_result_dead_letters(&self) -> Result<Vec<(Ulid, u32)>, StoreError> {
        let conn = self.conn.lock();
        let rows = conn
            .prepare("SELECT event_id, attempts FROM worker_result_dead_letters ORDER BY event_id")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)?;
        rows.into_iter()
            .map(|(id, attempts)| {
                Ok((
                    Ulid::from_string(&id)
                        .map_err(|_| StoreError::BadUlid("worker_result_dead_letter.id".into()))?,
                    u32::try_from(attempts).map_err(|_| StoreError::NumericRange)?,
                ))
            })
            .collect()
    }
}

pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS worker_result_relays (
            event_id TEXT PRIMARY KEY,
            global_seq INTEGER NOT NULL,
            task_grant_id TEXT,
            state TEXT NOT NULL CHECK(state IN ('attempting','pending','delivered','skipped','dead_letter')),
            attempts INTEGER NOT NULL DEFAULT 0,
            last_error TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS worker_result_dead_letters (
            event_id TEXT PRIMARY KEY,
            global_seq INTEGER NOT NULL,
            task_grant_id TEXT,
            attempts INTEGER NOT NULL,
            reason TEXT NOT NULL,
            created_at TEXT NOT NULL
        );",
    )?;
    Ok(())
}

//! Durable timer registry (`workflow_timers`) — the kernel-owned pending-timer
//! index backing AD-012 dark-window timers and the AD-104 workflow timer
//! substrate. The table is rebuildable from the audit ledger: every schedule
//! appends a `workflow.timer_scheduled` event in the same transaction as its
//! registry insert; every fire appends a `workflow.timer_fired` event in the
//! same transaction as its registry transition. The registry's job is to give
//! the due-timer driver a cheap, ledger-independent index and to make firing
//! at-most-once a DB-level guarantee (`status='pending'` compare-and-swap),
//! not a parallel source of truth.

use jiff::Timestamp;
use rusqlite::{params, OptionalExtension};

use super::{Store, StoreError};

pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS workflow_timers (\n\
         \x20   timer_id TEXT PRIMARY KEY,\n\
         \x20   run_id TEXT NOT NULL,\n\
         \x20   fires_at INTEGER NOT NULL,\n\
         \x20   status TEXT NOT NULL CHECK (status IN ('pending', 'fired')),\n\
         \x20   fired_event_id TEXT\n\
         );\n\
         CREATE INDEX IF NOT EXISTS idx_workflow_timers_due\n\
         \x20   ON workflow_timers (status, fires_at)\n\
         \x20   WHERE status = 'pending';",
    )?;
    Ok(())
}

fn timestamp_to_epoch_nanos(timestamp: Timestamp) -> Result<i64, StoreError> {
    i64::try_from(timestamp.as_nanosecond()).map_err(|_| {
        StoreError::TimestampRange(format!(
            "epoch nanoseconds {} do not fit SQLite INTEGER",
            timestamp.as_nanosecond()
        ))
    })
}

fn epoch_nanos_to_timestamp(nanos: i64) -> Result<Timestamp, StoreError> {
    Timestamp::from_nanosecond(i128::from(nanos)).map_err(|err| {
        StoreError::TimestampRange(format!("invalid epoch nanoseconds {nanos}: {err}"))
    })
}

impl Store {
    /// Atomically claim one exact workflow step and persist its canonical
    /// `Completed` record together with the timer registry row. The registry
    /// CAS makes concurrent contexts converge on the same timer spec.
    pub(crate) fn schedule_workflow_timer_step(
        &self,
        run_id: &str,
        step_id: &str,
        pending_seq: u64,
        kind: &str,
        input_digest: &str,
        fires_at: Timestamp,
    ) -> Result<(openspine_schemas::audit::AuditEvent, bool), StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let registry: (i64, Option<i64>) = tx.query_row(
            "SELECT pending_seq, completed_seq FROM workflow_step_registry
             WHERE run_id = ?1 AND step_id = ?2",
            params![run_id, step_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if registry.0 != i64::try_from(pending_seq).map_err(|_| StoreError::NumericRange)? {
            return Err(StoreError::InconsistentLineage(format!(
                "workflow step handle {step_id} does not match its pending sequence"
            )));
        }
        let aggregate = format!("workflow_run:{run_id}");
        let pending_json: String = tx.query_row(
            "SELECT event_json FROM audit_log
             WHERE aggregate_id = ?1 AND aggregate_seq = ?2",
            params![aggregate, registry.0],
            |row| row.get(0),
        )?;
        let pending_event: openspine_schemas::audit::AuditEvent =
            serde_json::from_str(&pending_json)?;
        let pending_payload: serde_json::Value = pending_event
            .payload_json
            .as_deref()
            .ok_or_else(|| {
                StoreError::InconsistentLineage(format!(
                    "workflow step {step_id} pending event has no payload"
                ))
            })
            .and_then(|raw| serde_json::from_str(raw).map_err(StoreError::from))?;
        let pending_matches = pending_event.kind.as_str() == kind
            && pending_payload.get("phase").and_then(|v| v.as_str()) == Some("Pending")
            && pending_payload.get("step_id").and_then(|v| v.as_str()) == Some(step_id)
            && pending_payload.get("input_digest").and_then(|v| v.as_str()) == Some(input_digest);
        if !pending_matches {
            return Err(StoreError::InconsistentLineage(format!(
                "workflow step handle {step_id} does not match its pending record"
            )));
        }
        let existing_event =
            |seq: i64| -> Result<openspine_schemas::audit::AuditEvent, StoreError> {
                let json: String = tx.query_row(
                "SELECT event_json FROM audit_log WHERE aggregate_id = ?1 AND aggregate_seq = ?2",
                params![aggregate, seq],
                |row| row.get(0),
            )?;
                Ok(serde_json::from_str(&json)?)
            };
        if let Some(seq) = registry.1.filter(|seq| *seq >= 0) {
            let event = existing_event(seq)?;
            tx.commit()?;
            return Ok((event, false));
        }
        let claimed = tx.execute(
            "UPDATE workflow_step_registry SET completed_seq = -1
             WHERE run_id = ?1 AND step_id = ?2 AND completed_seq IS NULL",
            params![run_id, step_id],
        )? == 1;
        if !claimed {
            let seq: i64 = tx.query_row(
                "SELECT completed_seq FROM workflow_step_registry
                 WHERE run_id = ?1 AND step_id = ?2",
                params![run_id, step_id],
                |row| row.get(0),
            )?;
            let event = existing_event(seq)?;
            tx.commit()?;
            return Ok((event, false));
        }
        let timer_id = ulid::Ulid::new().to_string();
        let spec = serde_json::json!({
            "timer_id": timer_id,
            "fires_at": fires_at,
        });
        let payload = serde_json::json!({
            "phase": "Completed",
            "step_id": step_id,
            "input_digest": input_digest,
            "outcome": {"Ok": spec},
        });
        let event = Self::append_audit_conn_with_options(
            &tx,
            kind,
            None,
            None,
            None,
            None,
            &[],
            &[],
            Some(&aggregate),
            Some(&serde_json::to_string(&payload)?),
        )?;
        tx.execute(
            "INSERT INTO workflow_timers
             (timer_id, run_id, fires_at, status, fired_event_id)
             VALUES (?1, ?2, ?3, 'pending', NULL)",
            params![timer_id, run_id, timestamp_to_epoch_nanos(fires_at)?],
        )?;
        tx.execute(
            "UPDATE workflow_step_registry SET completed_seq = ?3
             WHERE run_id = ?1 AND step_id = ?2 AND completed_seq = -1",
            params![run_id, step_id, event.aggregate_seq as i64],
        )?;
        tx.commit()?;
        Ok((event, true))
    }

    /// DB-enforced at-most-once timer firing. The `status='pending'` filter on
    /// the UPDATE is a compare-and-swap: only the first caller for a given
    /// `timer_id` affects a row, and the unique `timer_id` PRIMARY KEY
    /// guarantees that row is unique. Appends the terminal
    /// `workflow.timer_fired` event and records its ledger id, all in the same
    /// transaction. Returns the (possibly already-existing) terminal event.
    pub(crate) fn fire_workflow_timer(
        &self,
        timer_id: &str,
        at: Timestamp,
    ) -> Result<openspine_schemas::audit::AuditEvent, StoreError> {
        use openspine_schemas::audit::AuditEvent;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let claimed: Option<()> = tx
            .query_row(
                "UPDATE workflow_timers \
                 SET status = 'fired', fired_event_id = ?1 \
                 WHERE timer_id = ?2 AND status = 'pending' AND fires_at <= ?3 \
                 RETURNING 1",
                params!["", timer_id, timestamp_to_epoch_nanos(at)?],
                |_| Ok(()),
            )
            .optional()?;
        let event = if claimed.is_some() {
            // Fetch the run_id + fires_at from the registry we just updated so
            // the terminal ledger event is self-describing and rebuildable.
            let (run_id, fires_at_nanos): (String, i64) = tx.query_row(
                "SELECT run_id, fires_at FROM workflow_timers WHERE timer_id = ?1",
                params![timer_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let fires_at = epoch_nanos_to_timestamp(fires_at_nanos)?;
            let payload = serde_json::json!({
                "timer_id": timer_id,
                "fires_at": fires_at.to_string(),
            });
            let payload_str = serde_json::to_string(&payload)?;
            Self::append_audit_conn_with_options(
                &tx,
                "workflow.timer_fired",
                None,
                None,
                None,
                None,
                &[],
                &[],
                Some(&format!("workflow_run:{run_id}:timer:{timer_id}")),
                Some(&payload_str),
            )?
        } else {
            // Either already fired in this transaction (find the stored event
            // id) or never registered. Both surface as a typed failure rather
            // than silently dropping the call.
            let fired_event_id: Option<String> = tx
                .query_row(
                    "SELECT fired_event_id FROM workflow_timers WHERE timer_id = ?1",
                    params![timer_id],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            let Some(fired_id) = fired_event_id.filter(|id| !id.is_empty()) else {
                return Err(StoreError::WorkflowTimerUnknown(timer_id.to_string()));
            };
            let event_json: String = tx.query_row(
                "SELECT event_json FROM audit_log WHERE id = ?1",
                params![fired_id],
                |row| row.get(0),
            )?;
            serde_json::from_str::<AuditEvent>(&event_json)?
        };
        // Persist the freshly-minted terminal event's id back onto the row we
        // claimed, so a later repeat call finds it via the cheap lookup above.
        tx.execute(
            "UPDATE workflow_timers SET fired_event_id = ?1 WHERE timer_id = ?2",
            params![event.id.to_string(), timer_id],
        )?;
        tx.commit()?;
        Ok(event)
    }

    /// Cheap status check for `poll_timer`. True once the registry row has
    /// durably transitioned to `fired`.
    pub(crate) fn workflow_timer_fired(&self, timer_id: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock();
        let status: Option<String> = conn
            .query_row(
                "SELECT status FROM workflow_timers WHERE timer_id = ?1",
                params![timer_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(status.as_deref() == Some("fired"))
    }

    /// All pending timers whose `fires_at` is at or before `at`. Drives the
    /// kernel-owned [`crate::workflow::run_timer_driver`].
    pub(crate) fn due_timers(&self, at: Timestamp) -> Result<Vec<(String, String)>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT timer_id, run_id FROM workflow_timers \
             WHERE status = 'pending' AND fires_at <= ?1 ORDER BY fires_at ASC",
        )?;
        let rows = stmt.query_map(params![timestamp_to_epoch_nanos(at)?], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<_, _>>().map_err(StoreError::from)
    }

    /// Convenience driver: fire every timer due at `at`. Returns the terminal
    /// events appended. Safe to call from a polling loop or a recovery sweep.
    pub(crate) fn fire_due_timers(
        &self,
        at: Timestamp,
    ) -> Result<Vec<openspine_schemas::audit::AuditEvent>, StoreError> {
        let mut fired = Vec::new();
        for (timer_id, _run_id) in self.due_timers(at)? {
            fired.push(self.fire_workflow_timer(&timer_id, at)?);
        }
        Ok(fired)
    }

    /// Earliest pending-timer deadline, for sleep-until-deadline scheduling.
    pub(crate) fn next_timer_deadline(&self) -> Result<Option<Timestamp>, StoreError> {
        let conn = self.conn.lock();
        let nanos: Option<i64> = conn
            .query_row(
                "SELECT MIN(fires_at) FROM workflow_timers WHERE status = 'pending'",
                [],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        nanos.map(epoch_nanos_to_timestamp).transpose()
    }
}

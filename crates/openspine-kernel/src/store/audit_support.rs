//! SQLite storage audit log chaining and verification (PRD §18, D-012).
//!
//! Separated from `store/mod.rs` to keep that file under the 500-line gate.
//! AD-105: per-aggregate sequence assignment lives here so the ledger *is*
//! the event bus — no parallel store.
use openspine_schemas::event_bus::EventSubscriptionFilter;

use super::{genesis_digest, Store, StoreError};
use openspine_schemas::audit::AuditEvent;
use openspine_schemas::digest::digest_matches_hash;
use rusqlite::TransactionBehavior;
use sha2::{Digest as _, Sha256};

impl Store {
    /// Walk the chain from genesis, recomputing each hash. Returns `Ok(true)`
    /// if every row's stored hash matches, `Ok(false)` at the first break
    /// (a broken chain is not an I/O error — it's the thing this function
    /// exists to detect).
    pub fn verify_audit_chain(&self) -> Result<bool, StoreError> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT prev_hash, hash, meta_json, aggregate_id, aggregate_seq FROM audit_log ORDER BY seq ASC")?;
        let rows = stmt.query_map([], |row| {
            let prev_hash: String = row.get(0)?;
            let hash: String = row.get(1)?;
            let meta_json: String = row.get(2)?;
            let aggregate_id: String = row.get(3)?;
            let aggregate_seq: i64 = row.get(4)?;
            Ok((prev_hash, hash, meta_json, aggregate_id, aggregate_seq))
        })?;

        let mut expected_prev = genesis_digest().as_str().to_string();
        let mut hasher = Sha256::new();
        for row in rows {
            let (prev_hash, hash, meta_json, aggregate_id, aggregate_seq) = row?;
            if prev_hash != expected_prev {
                return Ok(false);
            }
            hasher.update(prev_hash.as_bytes());
            hasher.update(meta_json.as_bytes());
            let result = hasher.finalize_reset();
            if !digest_matches_hash(&hash, &result.into()) {
                return Ok(false);
            }
            let meta: serde_json::Value = match serde_json::from_str(&meta_json) {
                Ok(v) => v,
                Err(_) => return Ok(false),
            };
            let meta_aggregate = meta
                .get("aggregate_id")
                .and_then(|v| v.as_str())
                .unwrap_or("system");
            let meta_seq = meta
                .get("aggregate_seq")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            if meta_aggregate != aggregate_id || meta_seq != aggregate_seq {
                return Ok(false);
            }
            expected_prev = hash;
        }
        Ok(true)
    }
    /// Verify the full audit chain and replay one aggregate's events under a
    /// single connection lock, so verification and replay observe one
    /// snapshot (no concurrent append can interleave between them — required
    /// by D-012 replay integrity). Returns `StoreError::LedgerCorrupted` if
    /// the chain does not verify.
    pub(crate) fn verify_and_replay_aggregate(
        &self,
        aggregate: &str,
    ) -> Result<Vec<AuditEvent>, StoreError> {
        self.with_deferred_read(|tx| {
            let mut stmt = tx.prepare(
                "SELECT prev_hash, hash, meta_json, aggregate_id, aggregate_seq FROM audit_log ORDER BY seq ASC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?;
            let mut expected_prev = genesis_digest().as_str().to_string();
            let mut hasher = Sha256::new();
            for row in rows {
                let (prev_hash, hash, meta_json, aggregate_id, aggregate_seq) = row?;
                if prev_hash != expected_prev {
                    return Err(StoreError::LedgerCorrupted);
                }
                hasher.update(prev_hash.as_bytes());
                hasher.update(meta_json.as_bytes());
                let result = hasher.finalize_reset();
                if !digest_matches_hash(&hash, &result.into()) {
                    return Err(StoreError::LedgerCorrupted);
                }
                let meta: serde_json::Value =
                    serde_json::from_str(&meta_json).map_err(|_| StoreError::LedgerCorrupted)?;
                let meta_aggregate = meta
                    .get("aggregate_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("system");
                let meta_seq = meta
                    .get("aggregate_seq")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                if meta_aggregate != aggregate_id || meta_seq != aggregate_seq {
                    return Err(StoreError::LedgerCorrupted);
                }
                expected_prev = hash;
            }
            drop(stmt);
            let entries =
                Store::replay_audit_conn(tx, &EventSubscriptionFilter::aggregate(aggregate), 0)?;
            let events: Vec<AuditEvent> = entries.into_iter().map(|e| e.event).collect();
            Ok(events)
        })
    }

    #[allow(dead_code)]
    pub(crate) fn append_workflow_step(
        &self,
        run_id: &str,
        kind: &str,
        payload_json: &str,
    ) -> Result<AuditEvent, StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let event = Self::append_audit_conn_with_options(
            &tx,
            kind,
            None,
            None,
            None,
            None,
            &[],
            &[],
            Some(&format!("workflow_run:{run_id}")),
            Some(payload_json),
        )?;
        tx.commit()?;
        Ok(event)
    }
    pub(crate) fn append_workflow_step_if_absent(
        &self,
        run_id: &str,
        kind: &str,
        payload_json: &str,
        step_id: &str,
    ) -> Result<(AuditEvent, bool), StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO workflow_step_registry
             (run_id, step_id, pending_seq) VALUES (?1, ?2, -1)",
            rusqlite::params![run_id, step_id],
        )? == 1;
        let aggregate = format!("workflow_run:{run_id}");
        if !inserted {
            let pending_seq: i64 = tx.query_row(
                "SELECT pending_seq FROM workflow_step_registry
                 WHERE run_id = ?1 AND step_id = ?2",
                rusqlite::params![run_id, step_id],
                |row| row.get(0),
            )?;
            let json: String = tx.query_row(
                "SELECT event_json FROM audit_log
                 WHERE aggregate_id = ?1 AND aggregate_seq = ?2",
                rusqlite::params![aggregate, pending_seq],
                |row| row.get(0),
            )?;
            let event = serde_json::from_str(&json)?;
            tx.commit()?;
            return Ok((event, false));
        }
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
            Some(payload_json),
        )?;
        tx.execute(
            "UPDATE workflow_step_registry SET pending_seq = ?3
             WHERE run_id = ?1 AND step_id = ?2 AND pending_seq = -1",
            rusqlite::params![run_id, step_id, event.aggregate_seq as i64],
        )?;
        tx.commit()?;
        Ok((event, true))
    }
    pub(crate) fn append_workflow_receipt(
        &self,
        run_id: &str,
        kind: &str,
        payload_json: &str,
        step_id: &str,
    ) -> Result<(AuditEvent, bool), StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let claimed = tx.execute(
            "UPDATE workflow_step_registry SET receipt_seq = -1
             WHERE run_id = ?1 AND step_id = ?2 AND receipt_seq IS NULL",
            rusqlite::params![run_id, step_id],
        )? == 1;
        let aggregate = format!("workflow_run:{run_id}");
        if !claimed {
            let seq: i64 = tx.query_row(
                "SELECT receipt_seq FROM workflow_step_registry
                 WHERE run_id = ?1 AND step_id = ?2",
                rusqlite::params![run_id, step_id],
                |row| row.get(0),
            )?;
            let json: String = tx.query_row(
                "SELECT event_json FROM audit_log
                 WHERE aggregate_id = ?1 AND aggregate_seq = ?2",
                rusqlite::params![aggregate, seq],
                |row| row.get(0),
            )?;
            let event = serde_json::from_str(&json)?;
            tx.commit()?;
            return Ok((event, false));
        }
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
            Some(payload_json),
        )?;
        tx.execute(
            "UPDATE workflow_step_registry SET receipt_seq = ?3
             WHERE run_id = ?1 AND step_id = ?2 AND receipt_seq = -1",
            rusqlite::params![run_id, step_id, event.aggregate_seq as i64],
        )?;
        tx.commit()?;
        Ok((event, true))
    }

    pub(crate) fn append_workflow_completion(
        &self,
        run_id: &str,
        kind: &str,
        payload_json: &str,
        step_id: &str,
    ) -> Result<(AuditEvent, bool), StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let claimed = tx.execute(
            "UPDATE workflow_step_registry SET completed_seq = -1
             WHERE run_id = ?1 AND step_id = ?2 AND completed_seq IS NULL",
            rusqlite::params![run_id, step_id],
        )? == 1;
        let aggregate = format!("workflow_run:{run_id}");
        if !claimed {
            let seq: i64 = tx.query_row(
                "SELECT completed_seq FROM workflow_step_registry
                 WHERE run_id = ?1 AND step_id = ?2",
                rusqlite::params![run_id, step_id],
                |row| row.get(0),
            )?;
            let json: String = tx.query_row(
                "SELECT event_json FROM audit_log
                 WHERE aggregate_id = ?1 AND aggregate_seq = ?2",
                rusqlite::params![aggregate, seq],
                |row| row.get(0),
            )?;
            let event = serde_json::from_str(&json)?;
            tx.commit()?;
            return Ok((event, false));
        }
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
            Some(payload_json),
        )?;
        tx.execute(
            "UPDATE workflow_step_registry SET completed_seq = ?3
             WHERE run_id = ?1 AND step_id = ?2 AND completed_seq = -1",
            rusqlite::params![run_id, step_id, event.aggregate_seq as i64],
        )?;
        tx.commit()?;
        Ok((event, true))
    }
}
#[cfg(test)]
#[path = "audit_support_tests.rs"]
mod tests;

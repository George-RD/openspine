//! SQLite storage audit log chaining and verification (PRD §18, D-012).
//!
//! Separated from `store/mod.rs` to keep that file under the 500-line gate.
//! AD-105: per-aggregate sequence assignment lives here so the ledger *is*
//! the event bus — no parallel store.
use openspine_schemas::event_bus::EventSubscriptionFilter;

use super::{genesis_digest, Store, StoreError};
use jiff::Timestamp;
use openspine_schemas::action::{ActionId, GateDecision};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::audit::{default_aggregate_id, AuditEvent, AuditKind};
use openspine_schemas::digest::{canonical_json, digest_from_hash, digest_matches_hash, Digest};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest as _, Sha256};
use ulid::Ulid;

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
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
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
            Store::replay_audit_conn(&tx, &EventSubscriptionFilter::aggregate(aggregate), 0)?;
        let events: Vec<AuditEvent> = entries.into_iter().map(|e| e.event).collect();
        Ok(events)
    }

    /// Append one audit row, chaining it to the previous hash. Never
    /// mutates or removes an existing row (append-only, PRD §18). `id` and
    /// `ts` are folded into the hashed pre-image (not just stored
    /// alongside it) so neither can be silently rewritten without breaking
    /// [`Self::verify_audit_chain`].
    ///
    /// AD-105: also assigns `aggregate_id` (default policy) and the next
    /// per-aggregate `aggregate_seq` under the same connection lock as the
    /// insert. The row is durable before this call returns — that is the
    /// ledger-before-consume guarantee.
    #[allow(clippy::too_many_arguments)]
    pub fn append_audit(
        &self,
        kind: &str,
        action: Option<&ActionId>,
        decision: Option<&GateDecision>,
        reason: Option<&str>,
        task_grant_id: Option<Ulid>,
        target_refs: &[ArtifactRef],
        payload_refs: &[ArtifactRef],
    ) -> Result<AuditEvent, StoreError> {
        // Test-only one-shot failure: when armed, the next effective-Allow
        // `action.gated` audit append fails so a regression can prove a failed
        // effective-Allow audit cancels the reserved budget rather than
        // leaking it. The initial (ApprovalRequired) gate audit is never
        // targeted — only an effective Allow carries budget. The swap is
        // gated behind the kind/decision predicates so a prior non-Allow
        // `action.gated` audit cannot consume the one-shot flag.
        if kind == "action.gated"
            && matches!(decision, Some(GateDecision::Allow))
            && self
                .fail_next_effective_allow_audit
                .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
        }
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let event = Self::append_audit_conn(
            &tx,
            kind,
            action,
            decision,
            reason,
            task_grant_id,
            target_refs,
            payload_refs,
        )?;
        tx.commit()?;
        Ok(event)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn append_audit_conn(
        conn: &Connection,
        kind: &str,
        action: Option<&ActionId>,
        decision: Option<&GateDecision>,
        reason: Option<&str>,
        task_grant_id: Option<Ulid>,
        target_refs: &[ArtifactRef],
        payload_refs: &[ArtifactRef],
    ) -> Result<AuditEvent, StoreError> {
        Self::append_audit_conn_with_options(
            conn,
            kind,
            action,
            decision,
            reason,
            task_grant_id,
            target_refs,
            payload_refs,
            None,
            None,
        )
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

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn append_audit_conn_with_options(
        conn: &Connection,
        kind: &str,
        action: Option<&ActionId>,
        decision: Option<&GateDecision>,
        reason: Option<&str>,
        task_grant_id: Option<Ulid>,
        target_refs: &[ArtifactRef],
        payload_refs: &[ArtifactRef],
        aggregate_override: Option<&str>,
        payload_json: Option<&str>,
    ) -> Result<AuditEvent, StoreError> {
        let prev_hash = Self::last_hash(conn)?;
        let id = Ulid::new();
        let ts = Timestamp::now();
        let audit_kind =
            AuditKind::new(kind).map_err(|e| StoreError::BadAuditKind(e.to_string()))?;
        let aggregate_id = aggregate_override.map(str::to_string).unwrap_or_else(|| {
            task_grant_id.map_or_else(default_aggregate_id, |gid| format!("task_grant:{gid}"))
        });
        let aggregate_seq = Self::next_aggregate_seq(conn, &aggregate_id)?;
        let aggregate_seq_i64 =
            i64::try_from(aggregate_seq).map_err(|_| StoreError::NumericRange)?;
        let meta = serde_json::json!({
            "id": id.to_string(), "ts": ts.to_string(), "kind": audit_kind.as_str(),
            "action": action, "decision": decision, "reason": reason,
            "task_grant_id": task_grant_id.map(|u| u.to_string()),
            "target_refs": target_refs, "payload_refs": payload_refs,
            "aggregate_id": aggregate_id, "aggregate_seq": aggregate_seq,
            "payload_json": payload_json,
        });
        let canonical = canonical_json(&meta);
        let mut hasher = Sha256::new();
        hasher.update(prev_hash.as_str().as_bytes());
        hasher.update(canonical.as_bytes());
        let hash = digest_from_hash(hasher.finalize().into());
        let event = AuditEvent {
            id,
            schema_version: 1,
            ts,
            kind: audit_kind,
            action: action.cloned(),
            decision: decision.cloned(),
            reason: reason.map(str::to_string),
            task_grant_id,
            target_refs: target_refs.to_vec(),
            payload_refs: payload_refs.to_vec(),
            aggregate_id: aggregate_id.clone(),
            aggregate_seq,
            payload_json: payload_json.map(str::to_string),
            prev_hash,
            hash,
        };
        conn.execute("INSERT INTO audit_log (id, ts, kind, prev_hash, hash, meta_json, event_json, aggregate_id, aggregate_seq) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)", params![
            event.id.to_string(), event.ts.to_string(), event.kind.as_str(),
            event.prev_hash.as_str(), event.hash.as_str(), canonical,
            serde_json::to_string(&event)?, aggregate_id, aggregate_seq_i64,
        ])?;
        Ok(event)
    }

    /// Next positive sequence for `aggregate_id` (1-based). Called under the
    /// caller's connection lock so max+insert cannot race.
    fn next_aggregate_seq(conn: &Connection, aggregate_id: &str) -> Result<u64, StoreError> {
        // MAX always returns a row; NULL when the aggregate has no prior rows.
        let max: Option<i64> = conn.query_row(
            "SELECT MAX(aggregate_seq) FROM audit_log WHERE aggregate_id = ?1",
            params![aggregate_id],
            |row| row.get(0),
        )?;
        let current = max.unwrap_or(0);
        let current = u64::try_from(current).map_err(|_| StoreError::NumericRange)?;
        current.checked_add(1).ok_or(StoreError::NumericRange)
    }

    fn last_hash(conn: &Connection) -> Result<Digest, StoreError> {
        let hash: Option<String> = conn
            .query_row(
                "SELECT hash FROM audit_log ORDER BY seq DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        match hash {
            Some(h) => Digest::parse(h.clone()).map_err(|_| StoreError::BadDigest(h)),
            None => Ok(genesis_digest()),
        }
    }
    /// Install a per-store, kind-targeted SQLite fault for deterministic
    /// action-level tests. Each in-memory Store owns its connection, so this
    /// does not use process-global mutable state or cross-test coordination.
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn install_audit_append_failure_for_kind(&self, kind: &str) -> Result<(), StoreError> {
        let escaped = kind.replace('\'', "''");
        let conn = self.conn.lock();
        conn.execute_batch(&format!(
            "CREATE TRIGGER fail_audit_append_{suffix} \
             BEFORE INSERT ON audit_log WHEN NEW.kind = '{escaped}' \
             BEGIN SELECT RAISE(FAIL, 'injected audit append failure'); END;",
            suffix = escaped.replace(|c: char| !c.is_ascii_alphanumeric(), "_"),
        ))?;
        Ok(())
    }
}

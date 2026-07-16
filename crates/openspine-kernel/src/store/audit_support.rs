//! SQLite storage audit log chaining and verification (PRD §18, D-012).
//!
//! Separated from `store/mod.rs` to keep that file under the 500-line gate.

use super::{genesis_digest, Store, StoreError};
use jiff::Timestamp;
use openspine_schemas::action::{ActionId, GateDecision};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::audit::AuditEvent;
use openspine_schemas::digest::{canonical_json, digest_from_hash, digest_matches_hash, Digest};
use rusqlite::{params, Connection, OptionalExtension};
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
            conn.prepare("SELECT prev_hash, hash, meta_json FROM audit_log ORDER BY seq ASC")?;
        let rows = stmt.query_map([], |row| {
            let prev_hash: String = row.get(0)?;
            let hash: String = row.get(1)?;
            let meta_json: String = row.get(2)?;
            Ok((prev_hash, hash, meta_json))
        })?;

        let mut expected_prev = genesis_digest().as_str().to_string();
        let mut hasher = Sha256::new();
        for row in rows {
            let (prev_hash, hash, meta_json) = row?;
            if prev_hash != expected_prev {
                return Ok(false);
            }
            hasher.update(prev_hash.as_bytes());
            hasher.update(meta_json.as_bytes());
            let result = hasher.finalize_reset();
            if !digest_matches_hash(&hash, &result.into()) {
                return Ok(false);
            }
            expected_prev = hash;
        }
        Ok(true)
    }

    /// Append one audit row, chaining it to the previous hash. Never
    /// mutates or removes an existing row (append-only, PRD §18). `id` and
    /// `ts` are folded into the hashed pre-image (not just stored
    /// alongside it) so neither can be silently rewritten without breaking
    /// [`Self::verify_audit_chain`].
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
        let conn = self.conn.lock();
        Self::append_audit_conn(
            &conn,
            kind,
            action,
            decision,
            reason,
            task_grant_id,
            target_refs,
            payload_refs,
        )
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
        let prev_hash = Self::last_hash(conn)?;

        let id = Ulid::new();
        let ts = Timestamp::now();
        let meta = serde_json::json!({
            "id": id.to_string(),
            "ts": ts.to_string(),
            "kind": kind,
            "action": action,
            "decision": decision,
            "reason": reason,
            "task_grant_id": task_grant_id.map(|u| u.to_string()),
            "target_refs": target_refs,
            "payload_refs": payload_refs,
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
            kind: kind.to_string(),
            action: action.cloned(),
            decision: decision.cloned(),
            reason: reason.map(str::to_string),
            task_grant_id,
            target_refs: target_refs.to_vec(),
            payload_refs: payload_refs.to_vec(),
            prev_hash,
            hash,
        };

        conn.execute(
            "INSERT INTO audit_log (id, ts, kind, prev_hash, hash, meta_json, event_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                event.id.to_string(),
                event.ts.to_string(),
                event.kind,
                event.prev_hash.as_str(),
                event.hash.as_str(),
                canonical,
                serde_json::to_string(&event)?,
            ],
        )?;

        Ok(event)
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
}

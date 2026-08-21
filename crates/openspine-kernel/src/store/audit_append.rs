//! Audit append persistence, including optional typed metadata.
//!
//! Kept separate from audit verification/replay so the audit support module's
//! production boundary remains below the repository's 500-line gate.

use super::{genesis_digest, Store, StoreError};
use jiff::Timestamp;
use openspine_schemas::action::{ActionId, GateDecision};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::audit::{default_aggregate_id, AuditEvent, AuditKind};
use openspine_schemas::digest::{canonical_json, digest_from_hash, Digest};
use openspine_schemas::ids::PrincipalId;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest as _, Sha256};
use ulid::Ulid;

impl Store {
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
        self.append_audit_with_payload_json(
            kind,
            action,
            decision,
            reason,
            task_grant_id,
            target_refs,
            payload_refs,
            None,
        )
    }

    /// Append an audit row with an optional typed payload reference. The
    /// payload is hashed as part of the existing audit pre-image and remains
    /// metadata-only; callers must never place plaintext effect bytes here.
    #[allow(clippy::too_many_arguments)]
    pub fn append_audit_with_payload_json(
        &self,
        kind: &str,
        action: Option<&ActionId>,
        decision: Option<&GateDecision>,
        reason: Option<&str>,
        task_grant_id: Option<Ulid>,
        target_refs: &[ArtifactRef],
        payload_refs: &[ArtifactRef],
        payload_json: Option<&str>,
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
        self.with_immediate_tx(|tx| {
            let event = Self::append_audit_conn_with_options(
                tx,
                kind,
                action,
                decision,
                reason,
                task_grant_id,
                target_refs,
                payload_refs,
                None,
                payload_json,
            )?;
            Ok(event)
        })
    }

    /// Append one audit row carrying an optional typed `actor` (spec #197,
    /// D-003): the principal that authored the event. Used by owner-authored
    /// paths (approval, review, escalation); all other callers use
    /// [`Self::append_audit`] / [`Self::append_audit_with_payload_json`],
    /// which record `actor = None`. The actor is folded into the hashed
    /// pre-image, so it cannot be silently rewritten.
    #[allow(clippy::too_many_arguments)]
    pub fn append_audit_with_actor(
        &self,
        kind: &str,
        action: Option<&ActionId>,
        decision: Option<&GateDecision>,
        reason: Option<&str>,
        task_grant_id: Option<Ulid>,
        target_refs: &[ArtifactRef],
        payload_refs: &[ArtifactRef],
        actor: Option<&PrincipalId>,
    ) -> Result<AuditEvent, StoreError> {
        self.with_immediate_tx(|tx| {
            Self::append_audit_conn_with_actor(
                tx,
                kind,
                action,
                decision,
                reason,
                task_grant_id,
                target_refs,
                payload_refs,
                None,
                None,
                actor,
            )
        })
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
        Self::append_audit_conn_with_actor(
            conn,
            kind,
            action,
            decision,
            reason,
            task_grant_id,
            target_refs,
            payload_refs,
            aggregate_override,
            payload_json,
            None,
        )
    }

    /// As [`Self::append_audit_conn_with_options`], but also folds an optional
    /// typed `actor` (spec #197, D-003) into the hashed pre-image. New rows
    /// carry `actor` in their `meta` object unconditionally (null when absent),
    /// so the identity of the event author cannot be silently rewritten;
    /// historical rows re-verify from their stored `meta_json` verbatim and are
    /// unaffected by this addition.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn append_audit_conn_with_actor(
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
        actor: Option<&PrincipalId>,
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
            "actor": actor.map(|a| a.to_string()),
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
            actor: actor.copied(),
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
}

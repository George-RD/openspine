//! Kernel-packed audit slice for the reflection miner (AD-053/135).
//!
//! The miner never receives a caller-supplied briefcase: this module reads the
//! verified audit ledger and returns encrypted-reference-only entries. Rows
//! without a target ref (no real provenance) are skipped — they cannot anchor
//! miner observations. Every returned entry is stamped with the grant's
//! authoritative classification ceiling (never a per-row synthesis), because
//! the audit ledger does not persist a per-row classification and the ceiling
//! is the trust boundary the gateway already enforced for this grant.

use openspine_schemas::action::GateDecision;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::audit::AuditEvent;
use openspine_schemas::digest::Digest;
use openspine_schemas::event::DataClassification;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::ids::PrincipalId;
use openspine_schemas::reflection_miner::AuditTrailEntry;
use openspine_schemas::resolved_context::ResolvedActionContext;
use openspine_schemas::reviewed_scope::ReviewedActionScope;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use super::Store;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OwnerApprovalAuditMetadata {
    pub(crate) schema_version: u32,
    pub(crate) context_class_digest: Digest,
    pub(crate) reviewed_scope_digest: Digest,
    pub(crate) request_digest: Digest,
    pub(crate) target_digest: Digest,
    pub(crate) payload_digest: Digest,
    pub(crate) compatibility_digest: Digest,
    pub(crate) counterparty_scope_id: Ulid,
    pub(crate) reviewed_scope_ref: ArtifactRef,
}

impl OwnerApprovalAuditMetadata {
    pub(crate) fn from_context(
        context: &ResolvedActionContext,
        reviewed_scope_ref: ArtifactRef,
    ) -> Option<Self> {
        let scope = ReviewedActionScope::derive(context).ok()?;
        Some(Self {
            schema_version: 1,
            context_class_digest: scope.context_class_digest().clone(),
            reviewed_scope_digest: context.reviewed_scope_digest()?,
            request_digest: context.task_shape_digest()?.clone(),
            target_digest: context.target_digest()?.clone(),
            payload_digest: context.payload_digest()?.clone(),
            compatibility_digest: context.compatibility_digest().clone(),
            counterparty_scope_id: context.counterparty_identity_id()?,
            reviewed_scope_ref,
        })
    }
    pub(crate) fn from_payload_json(payload_json: Option<&str>) -> Option<Self> {
        let metadata: Self = serde_json::from_str(payload_json?).ok()?;
        (metadata.schema_version == 1).then_some(metadata)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OwnerApprovalScopeArtifact {
    pub(crate) scope: ReviewedActionScope,
    pub(crate) compatibility_digest: Digest,
}

impl OwnerApprovalScopeArtifact {
    pub(crate) fn from_context(context: &ResolvedActionContext) -> Option<Self> {
        Some(Self {
            scope: ReviewedActionScope::derive(context).ok()?,
            compatibility_digest: context.compatibility_digest().clone(),
        })
    }
}

impl Store {
    /// Load allowed, provenance-bearing audit events for one owner principal
    /// and stamp them into the scheduled miner's read-only scope. Source
    /// events retain their original task-grant identity; the scope argument
    /// controls only what the miner can see, never event ownership.
    pub fn load_owner_miner_audit_slice(
        &self,
        owner_principal_id: PrincipalId,
        grant_hmac_key: &[u8],
        scope: &str,
        ceiling: DataClassification,
    ) -> Result<Vec<AuditTrailEntry>, crate::store::StoreError> {
        if !self.verify_audit_chain()? {
            return Err(crate::store::StoreError::LedgerCorrupted);
        }
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT a.event_json, g.grant_json
             FROM audit_log AS a
             JOIN task_grants AS g
               ON g.id = json_extract(a.event_json, '$.task_grant_id')
             WHERE json_extract(g.grant_json, '$.user') = ?1
             ORDER BY a.seq ASC",
        )?;
        let rows = stmt.query_map(params![owner_principal_id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (event_json, grant_json) = row?;
            let event: AuditEvent = serde_json::from_str(&event_json)?;
            let source_grant: TaskGrant = serde_json::from_str(&grant_json)?;
            if source_grant.user != owner_principal_id || !source_grant.verify_mac(grant_hmac_key) {
                return Err(crate::store::StoreError::LedgerCorrupted);
            }
            if event.decision != Some(GateDecision::Allow)
                || event.reason.as_deref() != Some(super::OWNER_APPROVAL_GATE_REASON)
            {
                continue;
            }
            let Some(exchange) = event
                .target_refs
                .first()
                .or_else(|| event.payload_refs.first())
                .cloned()
            else {
                continue;
            };
            out.push(AuditTrailEntry {
                scope: scope.to_string(),
                artifact_id: exchange.digest.as_str().to_string(),
                event_id: event.id,
                exchange,
                classification: ceiling,
            });
        }
        Ok(out)
    }

    /// Fetch one audit event by id. Used to include an owner-correction anchor
    /// event in the miner briefcase so a correction observation's provenance
    /// is verifiable against the ledger (AD-135).
    pub fn audit_event_by_id(
        &self,
        id: Ulid,
    ) -> Result<Option<AuditEvent>, crate::store::StoreError> {
        if !self.verify_audit_chain()? {
            return Err(crate::store::StoreError::LedgerCorrupted);
        }
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT event_json FROM audit_log WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![id.to_string()], |row| row.get::<_, String>(0))?;
        match rows.next() {
            Some(Ok(json)) => Ok(Some(serde_json::from_str(&json)?)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }
}

//! Audit-before-effect pairing combinator (spec #208 D-002, expand step).
//!
//! [`Store::with_audited_effect`] is the single effect-path entry that may
//! write an effect row: it folds the caller's effect writes and their audit
//! append into one `Immediate` transaction and commits both atomically. Its
//! signature forces every effect-path caller to supply audit metadata (an
//! [`AuditDescriptor`]), so an effect-only write cannot be expressed here — the
//! Auditor's "effect with no prior audit row" failure becomes unrepresentable
//! on this path (AD-105 ledger-before-consume).
//!
//! Audit-only writes and non-effect internal-maintenance writes use separately
//! named entries (`append_audit`, `append_audit_conn`, or dedicated `Store`
//! methods); they never route through this combinator.

use super::{Store, StoreError};
use openspine_schemas::action::{ActionId, GateDecision};
use openspine_schemas::artifact::ArtifactRef;
use ulid::Ulid;

/// Owned audit-row inputs for [`Store::with_audited_effect`], mirroring the
/// arguments of `append_audit_conn`. Carried as plain data so no caller supplies
/// a raw `Connection`; deliberately decoupled from `Store` and `rusqlite`.
#[derive(Debug, Clone)]
pub struct AuditDescriptor {
    /// Audit kind (e.g. `"identity.bound"`).
    pub kind: String,
    /// Optional action this effect settles.
    pub action: Option<ActionId>,
    /// Optional gate decision that authorized the effect.
    pub decision: Option<GateDecision>,
    /// Optional human-readable reason string.
    pub reason: Option<String>,
    /// Optional task grant the effect is charged against.
    pub task_grant_id: Option<Ulid>,
    /// Artifact references the effect targets.
    pub target_refs: Vec<ArtifactRef>,
    /// Artifact references carried as payload metadata.
    pub payload_refs: Vec<ArtifactRef>,
}

impl AuditDescriptor {
    /// Build a descriptor for the common case: a `kind` with every optional
    /// field left empty. Chain [`AuditDescriptor::with_reason`] for the
    /// kind-plus-reason shape most pilot sites need; set the remaining public
    /// fields directly when required.
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            action: None,
            decision: None,
            reason: None,
            task_grant_id: None,
            target_refs: Vec::new(),
            payload_refs: Vec::new(),
        }
    }

    /// Attach a reason string, returning the descriptor for chaining.
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

impl Store {
    /// Pair an effect closure with its audit append inside one `Immediate`
    /// transaction, committing both atomically (spec #208 D-002).
    ///
    /// This is the **only** effect-path entry allowed to write an effect row.
    /// The `descriptor` forces audit metadata for every effect, so an
    /// effect-only write cannot be expressed on this path (AD-105
    /// ledger-before-consume). Audit-only and non-effect internal-maintenance
    /// writes must use separately named entries (`append_audit`,
    /// `append_audit_conn`, or dedicated `Store` methods).
    ///
    /// `effect` runs first and performs the effect-row writes; the audit row is
    /// then appended in the same transaction. If either step errors the
    /// transaction rolls back, so a failed audit append leaves no orphan effect
    /// row. Routes through [`Store::with_immediate_tx`] so the D-050
    /// write-serialization discipline is not restated here.
    pub fn with_audited_effect<T>(
        &self,
        descriptor: AuditDescriptor,
        effect: impl FnOnce(&rusqlite::Transaction) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        self.with_immediate_tx(|tx| {
            let value = effect(tx)?;
            Self::append_audit_conn(
                tx,
                descriptor.kind.as_str(),
                descriptor.action.as_ref(),
                descriptor.decision.as_ref(),
                descriptor.reason.as_deref(),
                descriptor.task_grant_id,
                &descriptor.target_refs,
                &descriptor.payload_refs,
            )?;
            Ok(value)
        })
    }
}

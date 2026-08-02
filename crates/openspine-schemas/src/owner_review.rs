//! Channel-neutral owner review contract for reusable delegation proposals.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::delegation_evidence::{DelegationEvidence, DelegationEvidenceKind};
use crate::digest::{digest_of, Digest};
use crate::reviewed_scope::ReviewedActionScope;
use crate::standing_rule::BudgetWindow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalKind {
    Responsibility,
    StandingRule,
    WorkflowCorrection,
    ManualArtifact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalProvenance {
    pub schema_version: u32,
    pub kind: DelegationEvidenceKind,
    pub summary: String,
    pub evidence_digest: Digest,
    pub evidence_count: u32,
}

impl ProposalProvenance {
    pub fn from_evidence(evidence: &DelegationEvidence, summary: String) -> Self {
        Self {
            schema_version: 1,
            kind: evidence.kind(),
            summary,
            evidence_digest: evidence.provenance_digest().clone(),
            evidence_count: evidence.evidence_count(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewLimits {
    pub quota: BudgetWindow,
    pub rate: BudgetWindow,
    pub expires_after_secs: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryBehavior {
    RequireApproval,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewFallbackBehavior {
    pub scope_mismatch: BoundaryBehavior,
    pub compatibility_drift: BoundaryBehavior,
    pub budget_exhaustion: BoundaryBehavior,
    pub timeout: BoundaryBehavior,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerReviewDecision {
    Approve,
    Reject,
    Narrow,
    Edit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponsibilityLifecycleControl {
    Pause,
    Resume,
    Expire,
    Revoke,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerReviewRequestInput {
    pub id: Ulid,
    pub schema_version: u32,
    pub review_version: u32,
    pub proposal_kind: ProposalKind,
    pub provenance: ProposalProvenance,
    pub title: String,
    pub description: String,
    pub reviewed_scope: ReviewedActionScope,
    pub automatic_effects: Vec<String>,
    pub remaining_boundaries: Vec<String>,
    pub limits: ReviewLimits,
    pub fallback_behavior: ReviewFallbackBehavior,
    pub proposal_digest: Digest,
    pub compatibility_digest: Digest,
    pub available_decisions: BTreeSet<OwnerReviewDecision>,
    pub lifecycle_controls: BTreeSet<ResponsibilityLifecycleControl>,
}

/// Canonical semantic review object rendered by Telegram, terminal, web, or
/// any future authenticated owner surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerReviewRequest {
    pub id: Ulid,
    pub schema_version: u32,
    pub review_version: u32,
    pub proposal_kind: ProposalKind,
    pub provenance: ProposalProvenance,
    pub title: String,
    pub description: String,
    pub reviewed_scope: ReviewedActionScope,
    pub automatic_effects: Vec<String>,
    pub remaining_boundaries: Vec<String>,
    pub limits: ReviewLimits,
    pub fallback_behavior: ReviewFallbackBehavior,
    pub proposal_digest: Digest,
    pub compatibility_digest: Digest,
    pub available_decisions: BTreeSet<OwnerReviewDecision>,
    pub lifecycle_controls: BTreeSet<ResponsibilityLifecycleControl>,
    binding_digest: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OwnerReviewRequestError {
    #[error("owner review field {field} is incomplete")]
    Incomplete { field: &'static str },
    #[error("owner review limits must be finite and positive")]
    InvalidLimits,
    #[error("owner review must offer approve, reject, and narrow decisions")]
    MissingRequiredDecisions,
    #[error("owner review must expose pause and revoke lifecycle controls")]
    MissingRequiredControls,
}

impl OwnerReviewRequest {
    pub fn try_new(input: OwnerReviewRequestInput) -> Result<Self, OwnerReviewRequestError> {
        for (field, invalid) in [
            ("schema_version", input.schema_version == 0),
            ("review_version", input.review_version == 0),
            (
                "provenance.summary",
                input.provenance.summary.trim().is_empty(),
            ),
            ("title", input.title.trim().is_empty()),
            ("description", input.description.trim().is_empty()),
            ("automatic_effects", input.automatic_effects.is_empty()),
            (
                "remaining_boundaries",
                input.remaining_boundaries.is_empty(),
            ),
        ] {
            if invalid {
                return Err(OwnerReviewRequestError::Incomplete { field });
            }
        }
        if input.limits.quota.max == 0
            || input.limits.quota.window_secs <= 0
            || input.limits.rate.max == 0
            || input.limits.rate.window_secs <= 0
            || input.limits.expires_after_secs <= 0
        {
            return Err(OwnerReviewRequestError::InvalidLimits);
        }
        if ![
            OwnerReviewDecision::Approve,
            OwnerReviewDecision::Reject,
            OwnerReviewDecision::Narrow,
        ]
        .iter()
        .all(|decision| input.available_decisions.contains(decision))
        {
            return Err(OwnerReviewRequestError::MissingRequiredDecisions);
        }
        if ![
            ResponsibilityLifecycleControl::Pause,
            ResponsibilityLifecycleControl::Revoke,
        ]
        .iter()
        .all(|control| input.lifecycle_controls.contains(control))
        {
            return Err(OwnerReviewRequestError::MissingRequiredControls);
        }

        let mut request = Self {
            id: input.id,
            schema_version: input.schema_version,
            review_version: input.review_version,
            proposal_kind: input.proposal_kind,
            provenance: input.provenance,
            title: input.title,
            description: input.description,
            reviewed_scope: input.reviewed_scope,
            automatic_effects: input.automatic_effects,
            remaining_boundaries: input.remaining_boundaries,
            limits: input.limits,
            fallback_behavior: input.fallback_behavior,
            proposal_digest: input.proposal_digest,
            compatibility_digest: input.compatibility_digest,
            available_decisions: input.available_decisions,
            lifecycle_controls: input.lifecycle_controls,
            binding_digest: digest_of(&serde_json::Value::Null),
        };
        request.binding_digest = request.calculate_binding_digest();
        Ok(request)
    }

    pub fn binding_digest(&self) -> &Digest {
        &self.binding_digest
    }

    /// Verify that persisted semantic review bytes still match the digest an
    /// authenticated owner surface must approve.
    pub fn binding_is_valid(&self) -> bool {
        self.reviewed_scope.binding_is_valid()
            && self.binding_digest == self.calculate_binding_digest()
    }

    fn calculate_binding_digest(&self) -> Digest {
        digest_of(&serde_json::json!({
            "id": self.id,
            "schema_version": self.schema_version,
            "review_version": self.review_version,
            "proposal_kind": self.proposal_kind,
            "provenance": self.provenance,
            "title": self.title,
            "description": self.description,
            "reviewed_scope": self.reviewed_scope,
            "automatic_effects": self.automatic_effects,
            "remaining_boundaries": self.remaining_boundaries,
            "limits": self.limits,
            "fallback_behavior": self.fallback_behavior,
            "proposal_digest": self.proposal_digest,
            "compatibility_digest": self.compatibility_digest,
            "available_decisions": self.available_decisions,
            "lifecycle_controls": self.lifecycle_controls,
        }))
    }
}

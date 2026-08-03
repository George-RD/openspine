//! Channel-neutral owner review contract for reusable delegation proposals.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::action::DelegationPolicyBounds;
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

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProposalProvenanceError {
    #[error("delegation evidence failed its integrity check")]
    InvalidEvidence,
}

impl ProposalProvenance {
    pub fn try_from_evidence(
        evidence: &DelegationEvidence,
    ) -> Result<Self, ProposalProvenanceError> {
        if !evidence.integrity_is_valid() {
            return Err(ProposalProvenanceError::InvalidEvidence);
        }
        let kind = evidence.kind();
        let evidence_count = evidence.evidence_count();
        let summary = match kind {
            DelegationEvidenceKind::RepeatedApprovals => {
                format!("{evidence_count} matching owner approvals")
            }
            DelegationEvidenceKind::ExplicitOwnerRequest => "Explicit owner request".to_string(),
            DelegationEvidenceKind::CorrectionOrWorkflowProposal => {
                "Correction or workflow proposal".to_string()
            }
            DelegationEvidenceKind::ManuallySuppliedArtifact => {
                "Manually supplied artifact".to_string()
            }
        };
        Ok(Self {
            schema_version: 1,
            kind,
            summary,
            evidence_digest: evidence.provenance_digest().clone(),
            evidence_count,
        })
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
    pub evidence: DelegationEvidence,
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
    #[error("delegation evidence failed its integrity check")]
    InvalidEvidence,
    #[error("repeated-approval evidence belongs to a different reviewed scope")]
    EvidenceScopeMismatch,
    #[error("owner review contains an invalid reviewed scope binding")]
    InvalidReviewedScope,
    #[error("owner review limits must be finite and positive")]
    InvalidLimits,
    #[error("owner review limits exceed the catalog policy bounds")]
    LimitsOutOfBounds,
    #[error("owner review must offer approve, reject, and narrow decisions")]
    MissingRequiredDecisions,
    #[error("owner review must expose pause and revoke lifecycle controls")]
    MissingRequiredControls,
}

impl OwnerReviewRequest {
    pub fn try_new(
        input: OwnerReviewRequestInput,
        policy: &DelegationPolicyBounds,
    ) -> Result<Self, OwnerReviewRequestError> {
        for (field, invalid) in [
            ("schema_version", input.schema_version == 0),
            ("review_version", input.review_version == 0),
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
        if !input.reviewed_scope.binding_is_valid() {
            return Err(OwnerReviewRequestError::InvalidReviewedScope);
        }
        let provenance = ProposalProvenance::try_from_evidence(&input.evidence)
            .map_err(|_| OwnerReviewRequestError::InvalidEvidence)?;
        if input
            .evidence
            .context_class_digest()
            .is_some_and(|digest| digest != input.reviewed_scope.context_class_digest())
        {
            return Err(OwnerReviewRequestError::EvidenceScopeMismatch);
        }
        if input.limits.quota.max == 0
            || input.limits.quota.window_secs <= 0
            || input.limits.rate.max == 0
            || input.limits.rate.window_secs <= 0
            || input.limits.expires_after_secs <= 0
        {
            return Err(OwnerReviewRequestError::InvalidLimits);
        }
        if !policy.quota.contains(input.limits.quota)
            || !policy.rate.contains(input.limits.rate)
            || input.limits.expires_after_secs > policy.maximum_lapse_secs
        {
            return Err(OwnerReviewRequestError::LimitsOutOfBounds);
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
            provenance,
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
        let mut value =
            serde_json::to_value(self).expect("owner review contains only serializable fields");
        value
            .as_object_mut()
            .expect("owner review serializes as an object")
            .remove("binding_digest");
        digest_of(&value)
    }
}

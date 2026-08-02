//! Evidence classes for proposing reusable delegation.
//!
//! Evidence can justify owner-facing provenance copy, but it never activates
//! authority. Only principal-authenticated owner decisions may contribute to
//! repeated-approval evidence.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::artifact::ArtifactRef;
use crate::digest::{digest_of, Digest};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationEvidenceKind {
    RepeatedApprovals,
    ExplicitOwnerRequest,
    CorrectionOrWorkflowProposal,
    ManuallySuppliedArtifact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerApprovalEvidence {
    pub decision_event_id: Ulid,
    pub owner_principal_id: Ulid,
    pub request_digest: Digest,
    pub target_digest: Digest,
    pub payload_digest: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DelegationEvidence {
    RepeatedApprovals {
        schema_version: u32,
        context_class_digest: Digest,
        evidence_set_digest: Digest,
        approval_count: u32,
        approvals: Vec<OwnerApprovalEvidence>,
    },
    ExplicitOwnerRequest {
        schema_version: u32,
        decision_event_id: Ulid,
        owner_principal_id: Ulid,
        request_digest: Digest,
    },
    CorrectionOrWorkflowProposal {
        schema_version: u32,
        proposal_event_id: Ulid,
        proposal_digest: Digest,
        source_event_ids: Vec<Ulid>,
    },
    ManuallySuppliedArtifact {
        schema_version: u32,
        supplied_by_principal_id: Ulid,
        artifact_ref: ArtifactRef,
        provenance_digest: Digest,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DelegationEvidenceError {
    #[error("repeated approval evidence needs at least two decisions, got {count}")]
    TooFewApprovals { count: usize },
    #[error("repeated approval evidence contains duplicate decision event {event_id}")]
    DuplicateDecisionEvent { event_id: Ulid },
    #[error("repeated approval evidence mixes owner principals")]
    MixedOwnerPrincipals,
    #[error("repeated approval evidence mixes request context classes")]
    MixedRequestDigests,
    #[error("evidence count exceeds u32")]
    ApprovalCountOverflow,
}

impl DelegationEvidence {
    pub fn repeated_approvals(
        context_class_digest: Digest,
        mut approvals: Vec<OwnerApprovalEvidence>,
    ) -> Result<Self, DelegationEvidenceError> {
        if approvals.len() < 2 {
            return Err(DelegationEvidenceError::TooFewApprovals {
                count: approvals.len(),
            });
        }
        approvals.sort_by_key(|approval| approval.decision_event_id);
        let mut seen = BTreeSet::new();
        for approval in &approvals {
            if !seen.insert(approval.decision_event_id) {
                return Err(DelegationEvidenceError::DuplicateDecisionEvent {
                    event_id: approval.decision_event_id,
                });
            }
        }
        let owner = approvals[0].owner_principal_id;
        if approvals
            .iter()
            .any(|approval| approval.owner_principal_id != owner)
        {
            return Err(DelegationEvidenceError::MixedOwnerPrincipals);
        }
        let request_digest = &approvals[0].request_digest;
        if approvals
            .iter()
            .any(|approval| &approval.request_digest != request_digest)
        {
            return Err(DelegationEvidenceError::MixedRequestDigests);
        }
        let approval_count = u32::try_from(approvals.len())
            .map_err(|_| DelegationEvidenceError::ApprovalCountOverflow)?;
        let evidence_set_digest = digest_of(&serde_json::json!({
            "schema_version": 1,
            "kind": "repeated_approvals",
            "context_class_digest": context_class_digest,
            "approvals": approvals,
        }));
        Ok(Self::RepeatedApprovals {
            schema_version: 1,
            context_class_digest,
            evidence_set_digest,
            approval_count,
            approvals,
        })
    }

    /// Verify count, uniqueness, canonical ordering, and evidence-set digest
    /// after persisted evidence is deserialized.
    pub fn integrity_is_valid(&self) -> bool {
        match self {
            Self::RepeatedApprovals {
                schema_version,
                context_class_digest,
                approvals,
                ..
            } => {
                *schema_version == 1
                    && Self::repeated_approvals(context_class_digest.clone(), approvals.clone())
                        .is_ok_and(|canonical| canonical == *self)
            }
            Self::ExplicitOwnerRequest { schema_version, .. }
            | Self::CorrectionOrWorkflowProposal { schema_version, .. }
            | Self::ManuallySuppliedArtifact { schema_version, .. } => *schema_version == 1,
        }
    }

    pub fn kind(&self) -> DelegationEvidenceKind {
        match self {
            Self::RepeatedApprovals { .. } => DelegationEvidenceKind::RepeatedApprovals,
            Self::ExplicitOwnerRequest { .. } => DelegationEvidenceKind::ExplicitOwnerRequest,
            Self::CorrectionOrWorkflowProposal { .. } => {
                DelegationEvidenceKind::CorrectionOrWorkflowProposal
            }
            Self::ManuallySuppliedArtifact { .. } => {
                DelegationEvidenceKind::ManuallySuppliedArtifact
            }
        }
    }

    /// Copy such as "Lyra noticed a pattern" is valid only for this class.
    pub fn supports_pattern_claim(&self) -> bool {
        matches!(self, Self::RepeatedApprovals { .. })
    }

    pub fn approval_count(&self) -> Option<u32> {
        match self {
            Self::RepeatedApprovals { approval_count, .. } => Some(*approval_count),
            _ => None,
        }
    }

    pub fn evidence_set_digest(&self) -> Option<&Digest> {
        match self {
            Self::RepeatedApprovals {
                evidence_set_digest,
                ..
            } => Some(evidence_set_digest),
            _ => None,
        }
    }

    pub fn context_class_digest(&self) -> Option<&Digest> {
        match self {
            Self::RepeatedApprovals {
                context_class_digest,
                ..
            } => Some(context_class_digest),
            _ => None,
        }
    }

    pub fn provenance_digest(&self) -> &Digest {
        match self {
            Self::RepeatedApprovals {
                evidence_set_digest,
                ..
            } => evidence_set_digest,
            Self::ExplicitOwnerRequest { request_digest, .. } => request_digest,
            Self::CorrectionOrWorkflowProposal {
                proposal_digest, ..
            } => proposal_digest,
            Self::ManuallySuppliedArtifact {
                provenance_digest, ..
            } => provenance_digest,
        }
    }

    pub fn evidence_count(&self) -> u32 {
        match self {
            Self::RepeatedApprovals { approval_count, .. } => *approval_count,
            Self::CorrectionOrWorkflowProposal {
                source_event_ids, ..
            } => u32::try_from(source_event_ids.len()).unwrap_or(u32::MAX),
            Self::ExplicitOwnerRequest { .. } | Self::ManuallySuppliedArtifact { .. } => 1,
        }
    }
}

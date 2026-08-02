//! Owner-facing responsibility manifest.
//!
//! A responsibility is a reference/view over reviewed workflow and standing-
//! rule artifacts. It is not a grant and contains no live authority lists.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::digest::Digest;
use crate::ids::ArtifactId;
use crate::owner_review::{ResponsibilityLifecycleControl, ReviewLimits};
use crate::resolved_context::ResolvedActionContext;
use crate::reviewed_scope::{ReviewedActionScope, ScopeComparison};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponsibilityStatus {
    Proposed,
    ReviewRequired,
    Active,
    Paused,
    NeedsReview,
    Expired,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResponsibilityCompatibilityBinding {
    pub schema_version: u32,
    pub descriptor_version: u32,
    pub implementation_version: u32,
    pub delegation_policy_version: u32,
    pub workflow_version: u32,
    pub owner_review_binding_digest: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResponsibilityManifest {
    pub id: ArtifactId,
    pub schema_version: u32,
    pub version: u32,
    pub status: ResponsibilityStatus,
    pub workflow_id: ArtifactId,
    pub standing_rule_id: ArtifactId,
    pub reviewed_scope: ReviewedActionScope,
    pub limits: ReviewLimits,
    pub compatibility: ResponsibilityCompatibilityBinding,
    pub controls: BTreeSet<ResponsibilityLifecycleControl>,
    pub provenance_digest: Digest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponsibilityDriftReason {
    ResolvedContextUnavailable,
    ScopeChanged,
    DescriptorVersionChanged,
    ImplementationVersionChanged,
    DelegationPolicyVersionChanged,
    WorkflowVersionChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ResponsibilityAssessment {
    Compatible,
    NeedsReview {
        reasons: BTreeSet<ResponsibilityDriftReason>,
    },
}

impl ResponsibilityManifest {
    /// Deterministically assess whether an active responsibility remains
    /// compatible with the current resolved effect path. Missing connector or
    /// account resolution is represented by `None` and fails to review.
    pub fn assess(
        &self,
        live_context: Option<&ResolvedActionContext>,
        current_policy_version: u32,
        current_workflow_version: u32,
    ) -> ResponsibilityAssessment {
        let Some(context) = live_context else {
            return ResponsibilityAssessment::NeedsReview {
                reasons: BTreeSet::from([ResponsibilityDriftReason::ResolvedContextUnavailable]),
            };
        };

        let mut reasons = BTreeSet::new();
        if !matches!(
            self.reviewed_scope.compare(context),
            ScopeComparison::Matches
        ) {
            reasons.insert(ResponsibilityDriftReason::ScopeChanged);
        }
        if self.compatibility.descriptor_version != context.descriptor_version() {
            reasons.insert(ResponsibilityDriftReason::DescriptorVersionChanged);
        }
        if self.compatibility.implementation_version != context.implementation_version() {
            reasons.insert(ResponsibilityDriftReason::ImplementationVersionChanged);
        }
        if self.compatibility.delegation_policy_version != current_policy_version {
            reasons.insert(ResponsibilityDriftReason::DelegationPolicyVersionChanged);
        }
        if self.compatibility.workflow_version != current_workflow_version {
            reasons.insert(ResponsibilityDriftReason::WorkflowVersionChanged);
        }

        if reasons.is_empty() {
            ResponsibilityAssessment::Compatible
        } else {
            ResponsibilityAssessment::NeedsReview { reasons }
        }
    }
}

//! Reflection miner contracts (AD-022/050/053/054/149).
//!
//! The miner is deliberately a pure worker-role boundary. It receives an
//! already scoped audit slice and an ordinary task grant, then returns
//! proposable artifacts. It has no store handle and no activation or standing
//! rule mutator, so every result must go through the normal artifact lifecycle.

use crate::artifact::{ArtifactRef, Lifecycle};
use crate::event::DataClassification;
use crate::grant::{GrantLimits, TaskGrant};
use crate::policy::Constraints;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// AD-135/D-096's learnable persona default.
pub const DIGEST_BRIEF_DEFAULT_ID: &str = "digest_brief_default";

/// The only classes a miner may emit (AD-053).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionOutputClass {
    CorrectionsWithReasons,
    RepeatedApprovals,
    StatedPreferences,
    Consolidation,
}

/// Encrypted, content-addressed provenance for one audit observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReflectionProvenance {
    pub source_event_id: Ulid,
    pub source_exchange: ArtifactRef,
}

/// One audit-trail entry included in the miner's scoped briefcase.
/// Sensitive content is represented only by encrypted references and bounded
/// classification; no plaintext correction summary is admitted (D-012).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditTrailEntry {
    pub scope: String,
    pub artifact_id: String,
    pub event_id: Ulid,
    pub exchange: ArtifactRef,
    pub classification: DataClassification,
}

/// Read-only audit slice. No kernel state or mutator is exposed here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinerBriefcase {
    pub grant_id: Ulid,
    pub scope: String,
    pub entries: Vec<AuditTrailEntry>,
}

impl MinerBriefcase {
    pub fn scoped(
        grant_id: Ulid,
        scope: impl Into<String>,
        entries: Vec<AuditTrailEntry>,
    ) -> Result<Self, MinerError> {
        let scope = scope.into();
        if scope.is_empty() {
            return Err(MinerError::EmptyScope);
        }
        if entries.iter().any(|entry| entry.scope != scope) {
            return Err(MinerError::BriefcaseScopeMismatch);
        }
        Ok(Self {
            grant_id,
            scope,
            entries,
        })
    }

    pub fn entries(&self) -> &[AuditTrailEntry] {
        &self.entries
    }
}

/// A grant-bound, ordinary miner invocation. This struct is *non-authorizing*:
/// the kernel runtime is the only place that may construct it, and it does so
/// only after the gateway and gate have admitted the underlying task grant and
/// the briefcase has been packed from the verified audit store (never from a
/// caller-supplied slice). The classification ceiling is derived from the
/// authenticated pack's enforced constraints, so it cannot widen beyond the
/// grant's authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrdinaryMinerGrant {
    pub grant_id: Ulid,
    pub classification_ceiling: DataClassification,
    pub limits: GrantLimits,
    pub briefcase: MinerBriefcase,
    pub expires_at: Timestamp,
}
impl OrdinaryMinerGrant {
    /// Construct an admitted miner grant. Callers must have already verified
    /// the underlying task grant through the kernel's gateway + gate boundary
    /// and supplied a kernel-packed `MinerBriefcase`. This is a structural
    /// validator only: it refuses expired grants, non-empty output channels,
    /// missing `model.generate:approved_provider`, direct-mutation actions, a
    /// briefcase bound to another grant, and any audit entry above the pack's
    /// classification ceiling.
    pub fn admit(
        grant: &TaskGrant,
        pack_constraints: &Constraints,
        briefcase: MinerBriefcase,
    ) -> Result<Self, MinerError> {
        if grant.is_expired(Timestamp::now()) {
            return Err(MinerError::GrantExpired);
        }
        if !grant.output_channels.is_empty() {
            return Err(MinerError::OutputChannelsNotEmpty);
        }
        if grant.id != briefcase.grant_id {
            return Err(MinerError::BriefcaseGrantMismatch);
        }
        if !grant
            .allowed_actions
            .iter()
            .any(|id| id.as_str() == "model.generate:approved_provider")
        {
            return Err(MinerError::RequiredActionMissing);
        }
        if grant.allowed_actions.iter().any(|id| {
            let name = id.as_str();
            name.starts_with("standing_rule.")
                || name.starts_with("policy.")
                || name == "artifact.activate"
                || name == "artifact.nominate_upstream"
        }) {
            return Err(MinerError::DirectMutationAction);
        }
        let ceiling = pack_constraints
            .data_classification_max
            .unwrap_or(DataClassification::Private);
        if briefcase
            .entries
            .iter()
            .any(|entry| !classification_within(entry.classification, ceiling))
        {
            return Err(MinerError::ClassificationExceeded);
        }
        Ok(Self {
            grant_id: grant.id,
            classification_ceiling: ceiling,
            limits: grant.limits,
            briefcase,
            expires_at: grant.expires_at,
        })
    }

    /// The kernel-packed audit slice is the sole source of miner context.
    /// Require an exact event-id and encrypted-exchange match before a
    /// reflection observation can be transformed into a proposal.
    pub fn provenance_in_briefcase(&self, provenance: &ReflectionProvenance) -> bool {
        self.briefcase.entries.iter().any(|entry| {
            entry.event_id == provenance.source_event_id
                && entry.exchange == provenance.source_exchange
        })
    }

    /// Whether `artifact_id` appears in the kernel-packed briefcase. The miner
    /// only consolidates artifacts the store has actually recorded (P1-10).
    pub fn briefcase_contains(&self, artifact_id: &str) -> bool {
        self.briefcase
            .entries
            .iter()
            .any(|entry| entry.artifact_id == artifact_id)
    }

    /// Count how many kernel-packed audit entries reference `artifact_id`.
    /// Repeated-approval evidence is derived from this slice, never from a
    /// caller-supplied count (P1-9).
    pub fn audit_entries_for(&self, artifact_id: &str) -> usize {
        self.briefcase
            .entries
            .iter()
            .filter(|entry| entry.artifact_id == artifact_id)
            .count()
    }
}
fn classification_within(value: DataClassification, ceiling: DataClassification) -> bool {
    matches!(
        (value, ceiling),
        (
            DataClassification::Public,
            DataClassification::Public | DataClassification::Internal | DataClassification::Private
        ) | (
            DataClassification::Internal,
            DataClassification::Internal | DataClassification::Private
        ) | (DataClassification::Private, DataClassification::Private)
    )
}
/// A correction whose instruction is shaped as a prohibition ("don't",
/// "never", "avoid", "must not") is not a positive rewrite — the miner must
/// reject it and never emit a prohibition artifact (P1).
pub fn is_prohibition_shaped(instruction: &str) -> bool {
    let lower = instruction.to_ascii_lowercase();
    let trimmed = lower.trim();
    trimmed.starts_with("don't")
        || trimmed.starts_with("do not")
        || trimmed.starts_with("never")
        || trimmed.starts_with("avoid")
        || trimmed.starts_with("must not")
        || lower.contains("must never")
}

/// A correction observation. Negative constraints are retained only as probe
/// input; they can never become a prohibition artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrectionObservation {
    pub kind: String,
    pub artifact_id: String,
    pub version: u32,
    pub instruction: String,
    pub reason: String,
    pub negative_constraint: Option<String>,
    pub provenance: ReflectionProvenance,
}

impl CorrectionObservation {
    /// AD-135 route for an owner correction to the digest/brief default.
    pub fn persona_digest(
        version: u32,
        instruction: impl Into<String>,
        reason: impl Into<String>,
        negative_constraint: Option<String>,
        provenance: ReflectionProvenance,
    ) -> Self {
        Self {
            kind: "persona".to_string(),
            artifact_id: DIGEST_BRIEF_DEFAULT_ID.to_string(),
            version,
            instruction: instruction.into(),
            reason: reason.into(),
            negative_constraint,
            provenance,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalObservation {
    pub kind: String,
    pub artifact_id: String,
    pub version: u32,
    pub action_id: String,
    pub candidate: String,
    pub provenance: ReflectionProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreferenceObservation {
    pub kind: String,
    pub artifact_id: String,
    pub version: u32,
    pub statement: String,
    pub provenance: ReflectionProvenance,
}

/// The miner's input classes. Every variant carries source provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReflectionObservation {
    Correction(CorrectionObservation),
    RepeatedApproval(ApprovalObservation),
    StatedPreference(PreferenceObservation),
}

/// A negative constraint is an eval input, not prompt guidance (AD-054/D-096).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalProbe {
    pub constraint: String,
    pub scenario: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReflectionProposalBody {
    /// Positive steering: replace the instruction with this rewrite.
    InstructionRewrite {
        instruction: String,
        reason: String,
        eval_probe: Option<EvalProbe>,
    },
    /// Candidate only; activation remains the normal lifecycle's job.
    /// The observed `action_id` is the real repeated-approval action, never a
    /// hardcoded default (P1).
    StandingRuleCandidate {
        candidate: String,
        action_id: String,
    },
    StatedPreference {
        statement: String,
    },
    Consolidation {
        merge_ids: Vec<String>,
        prune_ids: Vec<String>,
    },
}

/// A lifecycle-proposable artifact. There is intentionally no `Active` or
/// direct kernel/standing-rule mutation operation in this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionProposal {
    pub id: Ulid,
    pub class: ReflectionOutputClass,
    pub kind: String,
    pub artifact_id: String,
    pub version: u32,
    pub lifecycle_state: Lifecycle,
    pub task_grant_id: Ulid,
    pub provenance: ReflectionProvenance,
    pub body: ReflectionProposalBody,
}

/// Pure reflection miner: scoped in, proposable artifacts out.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReflectionMiner;

impl ReflectionMiner {
    pub fn mine(
        &self,
        grant: &OrdinaryMinerGrant,
        observations: &[ReflectionObservation],
    ) -> Result<Vec<ReflectionProposal>, MinerError> {
        if observations.len() > grant.limits.max_artifacts as usize {
            return Err(MinerError::ArtifactLimitExceeded);
        }
        if observations.iter().any(|observation| {
            let provenance = match observation {
                ReflectionObservation::Correction(value) => &value.provenance,
                ReflectionObservation::RepeatedApproval(value) => &value.provenance,
                ReflectionObservation::StatedPreference(value) => &value.provenance,
            };
            !grant.provenance_in_briefcase(provenance)
        }) {
            return Err(MinerError::ProvenanceOutOfScope);
        }
        observations
            .iter()
            .map(|observation| self.proposal(grant, observation))
            .collect()
    }

    fn proposal(
        &self,
        grant: &OrdinaryMinerGrant,
        observation: &ReflectionObservation,
    ) -> Result<ReflectionProposal, MinerError> {
        let (class, kind, artifact_id, version, provenance, body) = match observation {
            ReflectionObservation::Correction(c) => {
                if c.instruction.trim().is_empty() || c.reason.trim().is_empty() {
                    return Err(MinerError::EmptyCorrection);
                }
                if is_prohibition_shaped(&c.instruction) {
                    return Err(MinerError::ProhibitionShapedCorrection);
                }
                let probe = c.negative_constraint.as_ref().map(|constraint| EvalProbe {
                    constraint: constraint.clone(),
                    scenario: format!(
                        "{} produces an output satisfying the rewrite",
                        c.artifact_id
                    ),
                });
                (
                    ReflectionOutputClass::CorrectionsWithReasons,
                    c.kind.clone(),
                    c.artifact_id.clone(),
                    c.version,
                    c.provenance.clone(),
                    ReflectionProposalBody::InstructionRewrite {
                        instruction: c.instruction.clone(),
                        reason: c.reason.clone(),
                        eval_probe: probe,
                    },
                )
            }
            ReflectionObservation::RepeatedApproval(a) => {
                if a.candidate.trim().is_empty() {
                    return Err(MinerError::EmptyCorrection);
                }
                // Evidence is kernel-verifiable: count the real audit entries
                // in the packed briefcase, never a caller-supplied number.
                if grant.audit_entries_for(&a.artifact_id) < 2 {
                    return Err(MinerError::InsufficientApprovals);
                }
                (
                    ReflectionOutputClass::RepeatedApprovals,
                    a.kind.clone(),
                    a.artifact_id.clone(),
                    a.version,
                    a.provenance.clone(),
                    ReflectionProposalBody::StandingRuleCandidate {
                        candidate: a.candidate.clone(),
                        action_id: a.action_id.clone(),
                    },
                )
            }
            ReflectionObservation::StatedPreference(p) => {
                if p.statement.trim().is_empty() {
                    return Err(MinerError::EmptyPreference);
                }
                (
                    ReflectionOutputClass::StatedPreferences,
                    p.kind.clone(),
                    p.artifact_id.clone(),
                    p.version,
                    p.provenance.clone(),
                    ReflectionProposalBody::StatedPreference {
                        statement: p.statement.clone(),
                    },
                )
            }
        };
        Ok(ReflectionProposal {
            id: Ulid::new(),
            class,
            kind,
            artifact_id,
            version,
            lifecycle_state: Lifecycle::Proposed,
            task_grant_id: grant.grant_id,
            provenance,
            body,
        })
    }

    /// AD-022: produce a lifecycle proposal that merges duplicate content and
    /// prunes explicitly expired learned artifacts. The kernel applies neither
    /// list; it submits this proposal through normal review.
    pub fn consolidation(
        &self,
        grant: &OrdinaryMinerGrant,
        merge_ids: Vec<String>,
        prune_ids: Vec<String>,
        provenance: ReflectionProvenance,
    ) -> Result<ReflectionProposal, MinerError> {
        if merge_ids.is_empty() && prune_ids.is_empty() {
            return Err(MinerError::EmptyConsolidation);
        }
        // P1-10: only consolidate artifacts the store has actually recorded in
        // this grant's scoped briefcase — never arbitrary target ids.
        for id in merge_ids.iter().chain(prune_ids.iter()) {
            if !grant.briefcase_contains(id) {
                return Err(MinerError::ConsolidationTargetNotInBriefcase);
            }
        }
        Ok(ReflectionProposal {
            id: Ulid::new(),
            class: ReflectionOutputClass::Consolidation,
            kind: "learned_artifact_consolidation".to_string(),
            artifact_id: format!("consolidation-{}", Ulid::new()),
            version: 1,
            lifecycle_state: Lifecycle::Proposed,
            task_grant_id: grant.grant_id,
            provenance,
            body: ReflectionProposalBody::Consolidation {
                merge_ids,
                prune_ids,
            },
        })
    }
}

#[path = "reflection_miner_errors.rs"]
mod errors;
pub use errors::MinerError;

#[path = "reflection_miner_payload.rs"]
mod payload;

#[cfg(test)]
#[path = "reflection_miner_tests.rs"]
mod tests;

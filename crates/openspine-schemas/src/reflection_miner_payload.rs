use super::{MinerError, ReflectionProposal, ReflectionProposalBody};
use crate::artifact::Lifecycle;
use crate::standing_rule::StandingRuleManifest;
use serde_json::{json, Value};

impl ReflectionProposal {
    /// Emit the `{kind, yaml}` payload shape the normal `artifact.propose`
    /// handler expects. Persona and standing-rule proposals serialize their
    /// matching manifests; consolidation is a separate maintenance pass.
    pub fn to_proposal_payload(&self) -> Result<Value, MinerError> {
        match (&self.kind[..], &self.body) {
            ("persona", ReflectionProposalBody::InstructionRewrite { instruction, .. })
            | (
                "persona",
                ReflectionProposalBody::StatedPreference {
                    statement: instruction,
                },
            ) => {
                let element = crate::persona::PersonaElement {
                    id: self.artifact_id.clone(),
                    schema_version: 1,
                    version: self.version,
                    lifecycle_state: Lifecycle::Proposed,
                    guidance: instruction.clone(),
                };
                let yaml =
                    serde_yaml::to_string(&element).map_err(|_| MinerError::PayloadSerialize)?;
                Ok(json!({"kind": "persona", "yaml": yaml}))
            }
            (
                "standing_rule",
                ReflectionProposalBody::StandingRuleCandidate {
                    candidate,
                    action_id,
                },
            ) => {
                let manifest = StandingRuleManifest {
                    id: self.artifact_id.clone(),
                    schema_version: 1,
                    version: self.version,
                    lifecycle_state: Lifecycle::Proposed,
                    // The observed repeated-approval action, never a hardcoded
                    // default (P1).
                    action_id: crate::action::ActionId::new(action_id),
                    description: candidate.clone(),
                    quota: crate::standing_rule::BudgetWindow {
                        max: 5,
                        window_secs: 7 * 24 * 3600,
                    },
                    rate: crate::standing_rule::BudgetWindow {
                        max: 1,
                        window_secs: 3600,
                    },
                    expires_after_secs: 90 * 24 * 3600,
                    dark_window: None,
                };
                let yaml =
                    serde_yaml::to_string(&manifest).map_err(|_| MinerError::PayloadSerialize)?;
                Ok(json!({"kind": "standing_rule", "yaml": yaml}))
            }
            _ => Err(MinerError::UnsupportedLifecycleKind),
        }
    }
}

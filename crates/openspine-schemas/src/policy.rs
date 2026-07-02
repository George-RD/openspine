//! Global and session policy (PRD §8.1/§8.2 "global policy" and "user/session
//! policy" authority sources).
//!
//! Policies contribute candidate permissions and constraints exactly like a
//! capability pack — they never grant authority alone (PRD §9). The shape
//! is deliberately identical to [`crate::pack::CapabilityPack`]'s
//! action-list + constraints structure so `compose_authority` can treat all
//! authority sources uniformly.

use serde::{Deserialize, Serialize};

use crate::action::ActionId;
use crate::artifact::Lifecycle;
use crate::event::DataClassification;
use crate::ids::ArtifactId;

/// Constraints shared by capability packs, global policy, and session
/// policy (PRD §11.1/§11.2 `constraints`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Constraints {
    pub data_classification_max: Option<DataClassification>,
    pub external_visibility_max: Option<String>,
    /// Only meaningful for external-communication workflows (PRD §11.2).
    pub external_communication_is_instruction: Option<bool>,
    pub recovery_required: Option<String>,
    pub max_runtime_seconds: Option<u64>,
}

/// The global policy artifact — the outermost intersection every task grant
/// must fit inside (PRD §8.2 step 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub id: ArtifactId,
    pub schema_version: u32,
    #[serde(default = "crate::artifact::default_version")]
    pub version: u32,
    pub lifecycle_state: Lifecycle,
    #[serde(default)]
    pub candidate_allowed_actions: Vec<ActionId>,
    #[serde(default)]
    pub approval_required: Vec<ActionId>,
    #[serde(default)]
    pub denied_actions: Vec<ActionId>,
    #[serde(default)]
    pub constraints: Constraints,
}

/// Per-user/session policy — the other half of PRD §8.2 step 3's
/// intersection. Unlike [`Policy`], it is not a versioned declarative
/// artifact (no `id`/`lifecycle_state`): it is minted per session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionPolicy {
    pub schema_version: u32,
    #[serde(default)]
    pub candidate_allowed_actions: Vec<ActionId>,
    #[serde(default)]
    pub approval_required: Vec<ActionId>,
    #[serde(default)]
    pub denied_actions: Vec<ActionId>,
    #[serde(default)]
    pub constraints: Constraints,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_round_trips_through_serde() {
        let policy = Policy {
            id: "global".to_string(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Active,
            candidate_allowed_actions: vec![ActionId::new("openspine.status.read")],
            approval_required: vec![],
            denied_actions: vec![ActionId::new("email.send")],
            constraints: Constraints {
                data_classification_max: Some(DataClassification::Private),
                ..Default::default()
            },
        };
        let json = serde_json::to_string(&policy).unwrap();
        let back: Policy = serde_json::from_str(&json).unwrap();
        assert_eq!(policy, back);
    }

    #[test]
    fn constraints_default_to_all_unset() {
        let c = Constraints::default();
        assert!(c.data_classification_max.is_none() && c.max_runtime_seconds.is_none());
    }
}

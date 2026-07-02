//! Workflow manifests (PRD §9's role table: "declares expected sequence and
//! required capabilities; contributes constraints, no authority by itself").
//!
//! The PRD's route/task-grant examples reference workflow ids
//! (`owner_control_conversation`, `selected_thread_email_reply_draft`) but
//! give no literal `workflow:` YAML block, unlike routes/agents/packs. This
//! shape is a grounded design choice for this change: enough to name the
//! required agent/pack and the declared step sequence a workflow expects,
//! without inventing authority fields the PRD never describes.

use serde::{Deserialize, Serialize};

use crate::artifact::Lifecycle;
use crate::ids::ArtifactId;

/// A workflow manifest artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowManifest {
    pub id: ArtifactId,
    pub schema_version: u32,
    pub lifecycle_state: Lifecycle,
    pub purpose: String,
    pub required_agent: ArtifactId,
    pub required_capability_pack: ArtifactId,
    /// A declared, human-readable step sequence — descriptive only; it does
    /// not grant authority (PRD §9).
    #[serde(default)]
    pub steps: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_serde() {
        let workflow = WorkflowManifest {
            id: "owner_control_conversation".to_string(),
            schema_version: 1,
            lifecycle_state: Lifecycle::Active,
            purpose: "Owner-facing conversational orchestration.".to_string(),
            required_agent: "main_assistant_agent".to_string(),
            required_capability_pack: "owner_control_basic_pack".to_string(),
            steps: vec![
                "receive verified owner message".to_string(),
                "route to main assistant".to_string(),
            ],
        };
        let json = serde_json::to_string(&workflow).unwrap();
        let back: WorkflowManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(workflow, back);
    }
}

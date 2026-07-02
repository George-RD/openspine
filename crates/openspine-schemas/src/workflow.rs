//! Workflow manifests (PRD §9's role table: "declares expected sequence and
//! required capabilities; contributes constraints, no authority by itself").
//!
//! The PRD's route/task-grant examples reference workflow ids
//! (`owner_control_conversation`, `selected_thread_email_reply_draft`) but
//! give no literal `workflow:` YAML block, unlike routes/agents/packs. This
//! shape is a grounded design choice for this change: enough to name the
//! required agent/pack and the declared step sequence a workflow expects,
//! without inventing authority fields the PRD never describes.
//!
//! `candidate_allowed_actions`/`approval_required`/`denied_actions` were
//! added in `implement-authority-composition` (Step 2): design.md's merge
//! rule step 2 ("gather candidate allows from route, workflow, agent
//! manifest, and capability pack") requires a workflow to contribute a
//! candidate-allow set the same way an agent's `designed_tools` or a
//! capability pack's `candidate_allowed_actions` does. Default to empty so
//! existing fixtures without these keys keep parsing.

use serde::{Deserialize, Serialize};

use crate::action::ActionId;
use crate::artifact::Lifecycle;
use crate::ids::ArtifactId;

/// A workflow manifest artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowManifest {
    pub id: ArtifactId,
    pub schema_version: u32,
    #[serde(default = "crate::artifact::default_version")]
    pub version: u32,
    pub lifecycle_state: Lifecycle,
    pub purpose: String,
    pub required_agent: ArtifactId,
    pub required_capability_pack: ArtifactId,
    /// A declared, human-readable step sequence — descriptive only; it does
    /// not grant authority (PRD §9).
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default)]
    pub candidate_allowed_actions: Vec<ActionId>,
    #[serde(default)]
    pub approval_required: Vec<ActionId>,
    #[serde(default)]
    pub denied_actions: Vec<ActionId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_serde() {
        let workflow = WorkflowManifest {
            id: "owner_control_conversation".to_string(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Active,
            purpose: "Owner-facing conversational orchestration.".to_string(),
            required_agent: "main_assistant_agent".to_string(),
            required_capability_pack: "owner_control_basic_pack".to_string(),
            steps: vec![
                "receive verified owner message".to_string(),
                "route to main assistant".to_string(),
            ],
            candidate_allowed_actions: vec![ActionId::new("openspine.status.read")],
            approval_required: vec![],
            denied_actions: vec![ActionId::new("email.send")],
        };
        let json = serde_json::to_string(&workflow).unwrap();
        let back: WorkflowManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(workflow, back);
    }

    #[test]
    fn action_lists_default_to_empty_when_omitted() {
        let yaml = "id: x\nschema_version: 1\nlifecycle_state: active\npurpose: p\nrequired_agent: a\nrequired_capability_pack: c\n";
        let workflow: WorkflowManifest = serde_yaml::from_str(yaml).unwrap();
        assert!(workflow.candidate_allowed_actions.is_empty());
        assert!(workflow.approval_required.is_empty());
        assert!(workflow.denied_actions.is_empty());
    }
}

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
use std::collections::HashSet;

/// Static reasoning tiers (AD-046/AD-122). The gateway maps these to its
/// provider-routing hint; budget-aware knapsack selection remains deferred.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningTier {
    Low,
    #[default]
    Standard,
    High,
}

/// Whether leaving a state requires a digest-bound owner approval.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalSemantics {
    #[default]
    None,
    Required,
}

/// A deterministic or agentic unit executed while a state is active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStep {
    pub id: String,
    pub kind: WorkflowStepKind,
    #[serde(default)]
    pub reasoning_tier: ReasoningTier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepKind {
    Agentic,
    Deterministic,
}

/// A named escalation point in a declarative workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EscalationPoint {
    pub id: String,
    #[serde(default)]
    pub reason: Option<String>,
}

/// One state in a workflow state machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowState {
    pub id: String,
    #[serde(default)]
    pub steps: Vec<WorkflowStep>,
    #[serde(default)]
    pub approval: ApprovalSemantics,
    #[serde(default)]
    pub approval_action: Option<ActionId>,
    #[serde(default)]
    pub escalation: Option<EscalationPoint>,
}

/// A directed state transition. The source and target ids are exact matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowTransition {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub event: Option<String>,
}

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
    /// A declared, human-readable step sequence retained for legacy manifests.
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default)]
    pub candidate_allowed_actions: Vec<ActionId>,
    #[serde(default)]
    pub approval_required: Vec<ActionId>,
    #[serde(default)]
    pub denied_actions: Vec<ActionId>,
    /// Optional declarative state-machine shape. Empty means legacy workflow.
    #[serde(default)]
    pub initial_state: Option<String>,
    #[serde(default)]
    pub states: Vec<WorkflowState>,
    #[serde(default)]
    pub transitions: Vec<WorkflowTransition>,
}

impl WorkflowManifest {
    /// Validate state-machine references before a run can be started.
    pub fn validate(&self) -> Result<(), String> {
        let mut state_ids = HashSet::new();
        let mut step_ids = HashSet::new();
        let mut approval_state_ids = HashSet::new();
        for state in &self.states {
            if state.id.is_empty() {
                return Err("workflow state ids must not be empty".to_string());
            }
            if !state_ids.insert(&state.id) {
                return Err(format!("duplicate workflow state id {}", state.id));
            }
            if state.approval == ApprovalSemantics::Required {
                approval_state_ids.insert(&state.id);
            }
            for step in &state.steps {
                if step.id.is_empty() {
                    return Err("workflow step ids must not be empty".to_string());
                }
                if !step_ids.insert(&step.id) {
                    return Err(format!(
                        "duplicate workflow step id {} (step ids must be globally unique)",
                        step.id
                    ));
                }
            }
            if state.approval == ApprovalSemantics::Required && state.approval_action.is_none() {
                return Err(format!(
                    "approval-required state {} has no approval_action",
                    state.id
                ));
            }
        }
        if self.initial_state.is_none() && (!self.states.is_empty() || !self.transitions.is_empty())
        {
            return Err(
                "initial_state is required when a declarative workflow state machine is present"
                    .to_string(),
            );
        }
        if let Some(initial) = &self.initial_state {
            if !state_ids.contains(initial) {
                return Err(format!("initial state {initial} is not declared"));
            }
        }
        for transition in &self.transitions {
            if !state_ids.contains(&transition.from) || !state_ids.contains(&transition.to) {
                return Err(format!(
                    "transition {} -> {} references an undeclared state",
                    transition.from, transition.to
                ));
            }
            if transition
                .event
                .as_deref()
                .is_some_and(|event| event.contains(['|', '\n', '\r']))
            {
                return Err(
                    "workflow transition events must not contain Mermaid delimiters or newlines"
                        .to_string(),
                );
            }
            if approval_state_ids.contains(&transition.from)
                && approval_state_ids.contains(&transition.to)
            {
                return Err(format!(
                    "transition {} -> {} cannot leave and enter approval-required states in one step",
                    transition.from, transition.to
                ));
            }
        }
        Ok(())
    }

    /// Render the declarative portion as Mermaid flowchart syntax.
    pub fn to_mermaid(&self) -> String {
        let mut output = String::from("flowchart TD\n");
        for transition in &self.transitions {
            let label = transition.event.as_deref().unwrap_or("transition");
            output.push_str(&format!(
                "    {} -->|{}| {}\n",
                mermaid_id(&transition.from),
                label,
                mermaid_id(&transition.to)
            ));
        }
        output
    }

    /// Resolve a declared step's tier, defaulting legacy/omitted steps safely.
    pub fn reasoning_tier_for_step(&self, step_id: &str) -> ReasoningTier {
        self.states
            .iter()
            .flat_map(|state| state.steps.iter())
            .find(|step| step.id == step_id)
            .map(|step| step.reasoning_tier)
            .unwrap_or_default()
    }
}

fn mermaid_id(id: &str) -> String {
    // Encode every non-ASCII-alphanumeric UTF-8 byte, including `_`, so the
    // mapping is injective (`a-b` cannot collide with `a_b`) and starts with
    // a Mermaid-safe alphabetic token.
    let mut rendered = String::from("state_");
    for byte in id.bytes() {
        if byte.is_ascii_alphanumeric() {
            rendered.push(byte as char);
        } else {
            rendered.push_str(&format!("_{byte:02X}"));
        }
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflow() -> WorkflowManifest {
        WorkflowManifest {
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
            initial_state: Some("received".to_string()),
            states: vec![
                WorkflowState {
                    id: "received".to_string(),
                    steps: vec![WorkflowStep {
                        id: "gather".to_string(),
                        kind: WorkflowStepKind::Deterministic,
                        reasoning_tier: ReasoningTier::Low,
                    }],
                    approval: ApprovalSemantics::None,
                    approval_action: None,
                    escalation: None,
                },
                WorkflowState {
                    id: "send".to_string(),
                    steps: vec![WorkflowStep {
                        id: "compose".to_string(),
                        kind: WorkflowStepKind::Agentic,
                        reasoning_tier: ReasoningTier::High,
                    }],
                    approval: ApprovalSemantics::Required,
                    approval_action: Some(ActionId::new("telegram.reply:owner_channel")),
                    escalation: Some(EscalationPoint {
                        id: "owner-review".to_string(),
                        reason: Some("owner decision required".to_string()),
                    }),
                },
            ],
            transitions: vec![WorkflowTransition {
                from: "received".to_string(),
                to: "send".to_string(),
                event: Some("ready".to_string()),
            }],
        }
    }

    #[test]
    fn round_trips_through_serde() {
        let workflow = workflow();
        let json = serde_json::to_string(&workflow).unwrap();
        let back: WorkflowManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(workflow, back);
    }

    #[test]
    fn action_lists_and_state_machine_default_when_omitted() {
        let yaml = "id: x\nschema_version: 1\nlifecycle_state: active\npurpose: p\nrequired_agent: a\nrequired_capability_pack: c\n";
        let workflow: WorkflowManifest = serde_yaml::from_str(yaml).unwrap();
        assert!(workflow.candidate_allowed_actions.is_empty());
        assert!(workflow.approval_required.is_empty());
        assert!(workflow.denied_actions.is_empty());
        assert!(workflow.states.is_empty());
        assert!(workflow.transitions.is_empty());
        assert_eq!(
            workflow.reasoning_tier_for_step("legacy"),
            ReasoningTier::Standard
        );
    }

    #[test]
    fn renders_mermaid_and_resolves_declared_tier() {
        let workflow = workflow();
        assert_eq!(
            workflow.reasoning_tier_for_step("compose"),
            ReasoningTier::High
        );
        assert!(workflow.validate().is_ok());
        assert!(workflow
            .to_mermaid()
            .contains("state_received -->|ready| state_send"));
    }
    #[test]
    fn duplicate_step_id_across_states_is_rejected() {
        let mut workflow = workflow();
        workflow.states[0].steps.push(WorkflowStep {
            id: "compose".to_string(),
            kind: WorkflowStepKind::Deterministic,
            reasoning_tier: ReasoningTier::Low,
        });
        let err = workflow.validate().unwrap_err();
        assert!(err.contains("duplicate workflow step id compose"));
        assert!(err.contains("globally unique"));
    }

    #[test]
    fn approval_required_to_approval_required_transition_is_rejected() {
        let mut workflow = workflow();
        workflow.states.push(WorkflowState {
            id: "review".to_string(),
            steps: vec![],
            approval: ApprovalSemantics::Required,
            approval_action: Some(ActionId::new("email.create_draft")),
            escalation: None,
        });

        workflow.transitions.push(WorkflowTransition {
            from: "send".to_string(),
            to: "review".to_string(),
            event: Some("review".to_string()),
        });
        let error = workflow.validate().unwrap_err();
        assert!(error.contains("cannot leave and enter approval-required states"));
    }
    #[test]
    fn mermaid_ids_are_injective_and_empty_ids_are_rejected() {
        assert_ne!(mermaid_id("a-b"), mermaid_id("a_b"));
        assert!(mermaid_id("a-b").contains("_2D"));
        let mut empty = workflow();
        empty.states[0].id.clear();
        let error = empty.validate().unwrap_err();
        assert!(error.contains("must not be empty"));
    }

    #[test]
    fn mermaid_event_delimiters_are_rejected() {
        let mut workflow = workflow();
        workflow.transitions[0].event = Some("bad|event\n".to_string());
        let error = workflow.validate().unwrap_err();
        assert!(error.contains("Mermaid delimiters"));
    }

    #[test]
    fn declarative_machine_requires_initial_state() {
        let mut workflow = workflow();
        workflow.initial_state = None;
        let error = workflow.validate().unwrap_err();
        assert!(error.contains("initial_state is required"));
    }
}

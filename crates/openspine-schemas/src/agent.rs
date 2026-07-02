//! Agent manifests (PRD §10) — bounded workers, not authority-bearing
//! identities. `designed_tools` expresses intended use; the task grant
//! decides what the agent actually receives.

use serde::{Deserialize, Serialize};

use crate::action::ActionId;
use crate::artifact::Lifecycle;
use crate::ids::ArtifactId;
use crate::model::Provider;

/// PRD §10.1/§10.2 `persistence`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Persistence {
    Persistent,
    Ephemeral,
}

/// PRD §10.1/§10.2 `model_policy`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPolicy {
    pub allowed_providers: Vec<Provider>,
    pub private_context_requires_gateway: bool,
    pub max_model_calls_per_task: u32,
}

/// PRD §10.1/§10.2 `memory_scope`. Classes/scopes are open vocabularies
/// (product-defined, not a fixed enum) since new memory classes are added
/// without a schema change (D-013: dynamic behavior should be easy).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MemoryScope {
    pub allowed_classes: Vec<String>,
    pub allowed_scopes: Vec<String>,
    pub denied_classes: Vec<String>,
}

/// PRD §10.1/§10.2 `limits`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentLimits {
    pub max_runtime_seconds: u64,
    pub max_artifacts: u32,
    pub max_tokens: u32,
}

/// PRD §10.1/§10.2 `output_channels`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct OutputChannels {
    pub allowed: Vec<String>,
}

/// An agent manifest artifact (PRD §10.1/§10.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentManifest {
    pub id: ArtifactId,
    pub schema_version: u32,
    #[serde(default = "crate::artifact::default_version")]
    pub version: u32,
    pub lifecycle_state: Lifecycle,
    pub purpose: String,
    pub persistence: Persistence,
    pub persona: String,
    pub model_policy: ModelPolicy,
    #[serde(default)]
    pub memory_scope: MemoryScope,
    #[serde(default)]
    pub designed_tools: Vec<ActionId>,
    #[serde(default)]
    pub approval_required_tools: Vec<ActionId>,
    #[serde(default)]
    pub denied_tools: Vec<ActionId>,
    pub limits: AgentLimits,
    #[serde(default)]
    pub output_channels: OutputChannels,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn main_assistant_agent() -> AgentManifest {
        AgentManifest {
            id: "main_assistant_agent".to_string(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Active,
            purpose: "Owner-facing conversational orchestrator.".to_string(),
            persistence: Persistence::Persistent,
            persona: "concise_practical_operator".to_string(),
            model_policy: ModelPolicy {
                allowed_providers: vec![Provider::Local, Provider::Openai, Provider::Anthropic],
                private_context_requires_gateway: true,
                max_model_calls_per_task: 8,
            },
            memory_scope: MemoryScope {
                allowed_classes: vec!["owner_preference".to_string()],
                allowed_scopes: vec!["owner_control".to_string()],
                denied_classes: vec!["raw_email_body".to_string()],
            },
            designed_tools: vec![
                ActionId::new("openspine.status.read"),
                ActionId::new("telegram.reply:owner_channel"),
            ],
            approval_required_tools: vec![ActionId::new("connector.enable")],
            denied_tools: vec![
                ActionId::new("email.read_inbox"),
                ActionId::new("email.send"),
            ],
            limits: AgentLimits {
                max_runtime_seconds: 120,
                max_artifacts: 20,
                max_tokens: 12_000,
            },
            output_channels: OutputChannels {
                allowed: vec!["telegram.owner.reply".to_string()],
            },
        }
    }

    #[test]
    fn round_trips_through_serde() {
        let agent = main_assistant_agent();
        let json = serde_json::to_string(&agent).unwrap();
        let back: AgentManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(agent, back);
    }

    #[test]
    fn main_assistant_denies_broad_email_access() {
        let agent = main_assistant_agent();
        assert!(agent
            .denied_tools
            .contains(&ActionId::new("email.read_inbox")));
        assert!(agent.denied_tools.contains(&ActionId::new("email.send")));
        assert!(!agent
            .designed_tools
            .contains(&ActionId::new("email.read_inbox")));
    }
}

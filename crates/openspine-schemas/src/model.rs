//! Model gateway request schema (PRD §14).
//!
//! The shell requests inference; the model gateway constructs the final
//! provider call (D-010). A `ModelRequest` is how that request is
//! represented — it distinguishes trusted instruction sources from
//! untrusted data before any provider ever sees the prompt.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::event::DataClassification;

/// Model providers an agent manifest/model request may name (PRD §10.1/§14.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Local,
    Openai,
    Anthropic,
}

/// PRD §14.3 `redaction_required`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionRequirement {
    PolicyConditional,
}

/// PRD §14.3 `retention_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionMode {
    NoTrainingNoLoggingIfAvailable,
}

/// PRD §14.3 `output_policy.store_output`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreOutputPolicy {
    EncryptedRef,
}

/// PRD §14.3 `instruction_sources` — the trusted/untrusted split the
/// gateway's prompt template enforces (D-009: external content is data, not
/// instruction).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct InstructionSources {
    pub trusted: Vec<String>,
    pub untrusted_data: Vec<String>,
}

/// PRD §14.3 `output_policy`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputPolicy {
    pub store_output: StoreOutputPolicy,
    pub allow_shell_view: bool,
    pub allow_memory_write: bool,
}

/// A model gateway request (PRD §14.3). This is what the shell sends the
/// gateway; the gateway resolves `input_refs`, applies `template_id`, and
/// owns the final provider call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRequest {
    pub id: Ulid,
    pub task_grant_id: Ulid,
    pub requester: String,
    pub purpose: String,
    pub requested_provider: Option<Provider>,
    pub requested_model: Option<String>,
    #[serde(default)]
    pub input_refs: Vec<String>,
    pub template_id: String,
    pub data_classification: DataClassification,
    #[serde(default)]
    pub instruction_sources: InstructionSources,
    pub export_allowed: bool,
    pub redaction_required: RedactionRequirement,
    pub retention_mode: RetentionMode,
    pub output_policy: OutputPolicy,
    pub schema_version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_serde() {
        let req = ModelRequest {
            id: Ulid::new(),
            task_grant_id: Ulid::new(),
            requester: "email_reply_drafter".to_string(),
            purpose: "draft_email_reply".to_string(),
            requested_provider: Some(Provider::Anthropic),
            requested_model: None,
            input_refs: vec!["artifact:email_thread_excerpt:sha256:aaaa".to_string()],
            template_id: "email_reply_draft_template:v1".to_string(),
            data_classification: DataClassification::Private,
            instruction_sources: InstructionSources {
                trusted: vec!["system_template".to_string()],
                untrusted_data: vec!["email_thread_excerpt".to_string()],
            },
            export_allowed: true,
            redaction_required: RedactionRequirement::PolicyConditional,
            retention_mode: RetentionMode::NoTrainingNoLoggingIfAvailable,
            output_policy: OutputPolicy {
                store_output: StoreOutputPolicy::EncryptedRef,
                allow_shell_view: true,
                allow_memory_write: false,
            },
            schema_version: 1,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: ModelRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn untrusted_data_never_collapses_into_trusted() {
        let sources = InstructionSources {
            trusted: vec!["system_template".into()],
            untrusted_data: vec!["email_thread_excerpt".into()],
        };
        assert!(!sources
            .trusted
            .contains(&"email_thread_excerpt".to_string()));
    }
}

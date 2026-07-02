//! Minimal model gateway (build plan 4c): PRD §14's request shape plus one
//! provider call. The full `implement-model-gateway` change (routing across
//! many providers, retries, cost accounting) remains deferred — Phase 1
//! needs only `model.generate` to work end to end.
//!
//! The gateway owns the final provider call; the shell never talks to a
//! model provider directly and never sees a provider API key (D-010).

mod providers;

pub use providers::ProviderClient;

use serde::{Deserialize, Serialize};

use openspine_schemas::artifact::Lifecycle;

/// A prompt template artifact (design.md 4c: "Prompt templates as
/// artifacts"). Only `system_preamble` is needed for Step 4's
/// `owner_control_template` — untrusted-data wrapping is Step 5's concern
/// (`email_reply_draft_template`) and is added there, not fabricated here
/// ahead of a real caller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptTemplate {
    pub id: String,
    pub schema_version: u32,
    #[serde(default = "openspine_schemas::artifact::default_version")]
    pub version: u32,
    pub lifecycle_state: Lifecycle,
    pub system_preamble: String,
}

/// One turn of an already-normalized conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptMessage {
    pub role: PromptRole,
    pub content: String,
}

/// The fully-resolved prompt a [`ProviderClient`] actually sends. Built by
/// [`build_prompt`] from a trusted [`PromptTemplate`] plus the trusted
/// conversation history — this module never accepts untrusted external
/// content (Step 5 introduces that, with its own instruction/data split).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPrompt {
    pub system: String,
    pub messages: Vec<PromptMessage>,
    pub max_tokens: u32,
}

/// Apply `template` to `conversation`, producing the exact request the
/// provider client sends. Pure function — no I/O, no template loading (the
/// caller already resolved the template artifact).
pub fn build_prompt(
    template: &PromptTemplate,
    conversation: Vec<PromptMessage>,
    max_tokens: u32,
) -> ResolvedPrompt {
    ResolvedPrompt {
        system: template.system_preamble.clone(),
        messages: conversation,
        max_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template() -> PromptTemplate {
        PromptTemplate {
            id: "owner_control_template".to_string(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Active,
            system_preamble: "You are Lyra.".to_string(),
        }
    }

    #[test]
    fn build_prompt_carries_system_preamble_and_conversation_through() {
        let conversation = vec![PromptMessage {
            role: PromptRole::User,
            content: "hello".to_string(),
        }];
        let resolved = build_prompt(&template(), conversation.clone(), 12000);
        assert_eq!(resolved.system, "You are Lyra.");
        assert_eq!(resolved.messages, conversation);
        assert_eq!(resolved.max_tokens, 12000);
    }

    #[test]
    fn template_round_trips_through_yaml() {
        let yaml = serde_yaml::to_string(&template()).unwrap();
        let back: PromptTemplate = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back, template());
    }
}

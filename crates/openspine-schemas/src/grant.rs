//! The task grant (PRD §12) — the only live authority object presented to a
//! running agent/workflow (PRD §9, D-007).

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::action::ActionId;
use crate::artifact::Lifecycle;
use crate::ids::ArtifactId;

/// PRD §12.1/§12.2 `limits`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantLimits {
    pub max_model_calls: u32,
    pub max_artifacts: u32,
    pub max_runtime_seconds: u64,
}

/// The final resolved authority object (PRD §12.1/§12.2).
///
/// `task_token` is not part of the PRD's literal example — it is the
/// kernel↔shell transport bearer secret minted at grant issuance (D-032),
/// recorded here because the kernel must be able to look up "which grant
/// does this token authenticate" from the grant alone.
///
/// **Redaction warning (for Step 4/the kernel):** `task_token` is a live
/// bearer secret. Any place a `TaskGrant` is serialized outward — audit
/// rows, `GET /v1/status`, logs — MUST project through a redacted view
/// that omits it, never serialize this struct directly to an
/// external-facing surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskGrant {
    pub id: Ulid,
    pub schema_version: u32,
    pub lifecycle_state: Lifecycle,
    pub user: String,
    pub purpose: String,
    pub issued_by: String,
    pub issued_at: jiff::Timestamp,
    pub expires_at: jiff::Timestamp,
    pub event_id: Ulid,
    pub route_id: ArtifactId,
    pub agent_id: ArtifactId,
    pub workflow_id: ArtifactId,
    pub capability_pack_id: ArtifactId,
    /// `<kind>:<id>:v<N>` strings, e.g. `capability_pack:owner_control_basic_pack:v1` (D-028).
    #[serde(default)]
    pub authority_sources: Vec<String>,
    #[serde(default)]
    pub selection_tokens: Vec<Ulid>,
    #[serde(default)]
    pub allowed_actions: Vec<ActionId>,
    #[serde(default)]
    pub approval_required_actions: Vec<ActionId>,
    #[serde(default)]
    pub denied_actions: Vec<ActionId>,
    #[serde(default)]
    pub output_channels: Vec<String>,
    pub limits: GrantLimits,
    /// Per-task bearer secret for the kernel API (D-032). Hex-encoded random bytes.
    pub task_token: String,
}

impl TaskGrant {
    pub fn is_expired(&self, now: jiff::Timestamp) -> bool {
        now >= self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::Timestamp;

    fn owner_control_grant() -> TaskGrant {
        let issued_at = Timestamp::now();
        TaskGrant {
            id: Ulid::new(),
            schema_version: 1,
            lifecycle_state: Lifecycle::Active,
            user: "owner".to_string(),
            purpose: "owner_control_conversation".to_string(),
            issued_by: "kernel".to_string(),
            issued_at,
            expires_at: issued_at + std::time::Duration::from_secs(120),
            event_id: Ulid::new(),
            route_id: "owner_telegram_main_assistant".to_string(),
            agent_id: "main_assistant_agent".to_string(),
            workflow_id: "owner_control_conversation".to_string(),
            capability_pack_id: "owner_control_basic_pack".to_string(),
            authority_sources: vec![
                "global_policy:v1".to_string(),
                "route:owner_telegram_main_assistant:v1".to_string(),
                "agent:main_assistant_agent:v1".to_string(),
                "workflow:owner_control_conversation:v1".to_string(),
                "capability_pack:owner_control_basic_pack:v1".to_string(),
            ],
            selection_tokens: vec![],
            allowed_actions: vec![ActionId::new("openspine.status.read")],
            approval_required_actions: vec![ActionId::new("connector.enable")],
            denied_actions: vec![ActionId::new("email.read_inbox")],
            output_channels: vec!["telegram.owner.reply".to_string()],
            limits: GrantLimits {
                max_model_calls: 8,
                max_artifacts: 20,
                max_runtime_seconds: 120,
            },
            task_token: "a".repeat(64),
        }
    }

    #[test]
    fn round_trips_through_serde() {
        let grant = owner_control_grant();
        let json = serde_json::to_string(&grant).unwrap();
        let back: TaskGrant = serde_json::from_str(&json).unwrap();
        assert_eq!(grant, back);
    }

    #[test]
    fn is_expired_uses_expires_at() {
        let grant = owner_control_grant();
        assert!(!grant.is_expired(grant.issued_at));
        assert!(grant.is_expired(grant.expires_at));
        assert!(grant.is_expired(grant.expires_at + std::time::Duration::from_secs(1)));
    }

    #[test]
    fn authority_sources_use_kind_id_version_format() {
        let grant = owner_control_grant();
        for source in &grant.authority_sources {
            assert!(
                source.contains(':'),
                "authority source {source} must be <kind>:<id>:v<N>"
            );
        }
    }
}

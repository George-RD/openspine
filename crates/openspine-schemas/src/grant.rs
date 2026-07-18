//! Task grants: the only live authority object presented to workers (D-007).
// Chain wire shape is `chain: Vec<GrantChainStep>`; each step carries only
// its newly added caveats and its MAC-covered delegation bind.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::action::ActionId;
use crate::artifact::Lifecycle;
use crate::egress::EgressClass;
use crate::grant_chain::{self, Caveat, ChainStep};
use crate::ids::ArtifactId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantLimits {
    pub max_model_calls: u32,
    pub max_artifacts: u32,
    pub max_runtime_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantMode {
    #[default]
    Live,
    Shadow,
}

/// The final resolved authority object. `task_token` is a transport bearer
/// secret and MUST be redacted from outward serialization (D-032/D-047).
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
    /// AD-060: egress classes this grant may exercise. Empty means no
    /// rated egress is authorized (deny-by-default for egress endpoints).
    #[serde(default)]
    pub allowed_egress_classes: Vec<EgressClass>,
    #[serde(default)]
    pub output_channels: Vec<String>,
    pub limits: GrantLimits,
    pub task_token: String,
    /// Immutable root identity. Roots set this to `id`; children copy it.
    #[serde(default)]
    pub root_grant_id: Ulid,
    /// Immediate lineage only; parent is never a second live authority.
    #[serde(default)]
    pub parent_grant_id: Option<Ulid>,
    #[serde(default)]
    pub mode: GrantMode,
    /// Self-contained ordered chain from root to this grant.
    #[serde(default)]
    pub chain: Vec<ChainStep>,
    /// Hex HMAC terminal tip over root authority and every chain hop.
    #[serde(default)]
    pub caveat_mac: String,
    /// Dormant channel-thread binding (AD-148). None until a thread-capable
    /// channel ships; kernel-owned, never set by the shell. A populated
    /// binding is MAC-covered; None is omitted from canonical root bytes for
    /// pre-thread grant compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// AD-136: the kernel-resolved persona that fronts the conversation
    /// this grant was composed for. Additive and audit-only; personas
    /// carry no authority (D-094). `None` means no persona was bound
    /// (an unbound or invalid binding yields no fronting persona, never
    /// the agent's choice). Never set by the shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_id: Option<String>,
}

impl TaskGrant {
    pub fn is_expired(&self, now: jiff::Timestamp) -> bool {
        grant_chain::is_expired(self, now)
    }

    /// Seal a root after all authority fields (including token bindings) are final.
    pub fn seal_root(&mut self, key: &[u8]) {
        grant_chain::seal_root(self, key);
    }
    pub fn effectively_approval_required(&self, action: &ActionId) -> bool {
        grant_chain::effectively_approval_required(self, action)
    }

    pub fn verify_mac(&self, key: &[u8]) -> bool {
        grant_chain::verify_mac(key, self)
    }

    pub fn effectively_allows(&self, action: &ActionId) -> bool {
        grant_chain::effectively_allows(self, action)
    }

    pub fn caveats(&self) -> Vec<&Caveat> {
        grant_chain::flattened_caveats(&self.chain)
    }
}

pub use grant_chain::{Caveat as GrantCaveat, ChainStep as GrantChainStep};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::Lifecycle;
    use crate::grant_chain::TEST_GRANT_HMAC_KEY;
    use jiff::Timestamp;

    fn grant() -> TaskGrant {
        let now = Timestamp::now();
        let id = Ulid::new();
        let mut g = TaskGrant {
            id,
            schema_version: 1,
            lifecycle_state: Lifecycle::Active,
            user: "owner".into(),
            purpose: "test".into(),
            issued_by: "kernel".into(),
            issued_at: now,
            expires_at: now + std::time::Duration::from_secs(60),
            event_id: Ulid::new(),
            route_id: "r".into(),
            agent_id: "a".into(),
            workflow_id: "w".into(),
            capability_pack_id: "p".into(),
            authority_sources: vec![],
            selection_tokens: vec![],
            allowed_actions: vec![ActionId::new("openspine.status.read")],
            approval_required_actions: vec![],
            denied_actions: vec![],
            allowed_egress_classes: vec![],
            output_channels: vec![],
            limits: GrantLimits {
                max_model_calls: 1,
                max_artifacts: 1,
                max_runtime_seconds: 60,
            },
            task_token: "a".repeat(64),
            persona_id: None,
            root_grant_id: id,
            parent_grant_id: None,
            mode: GrantMode::Live,
            chain: vec![],
            caveat_mac: String::new(),
            thread_id: None,
        };
        g.seal_root(TEST_GRANT_HMAC_KEY);
        g
    }

    #[test]
    fn round_trip_and_mac() {
        let g = grant();
        let back: TaskGrant = serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
        assert_eq!(g, back);
        assert!(g.verify_mac(TEST_GRANT_HMAC_KEY));
    }

    #[test]
    fn legacy_without_thread_id_defaults_to_none() {
        let mut value = serde_json::to_value(grant()).unwrap();
        value.as_object_mut().unwrap().remove("thread_id");
        let back: TaskGrant = serde_json::from_value(value).unwrap();
        assert!(back.thread_id.is_none());
    }

    #[test]
    fn thread_id_round_trips_when_populated() {
        let mut value = grant();
        value.thread_id = Some("topic-42".to_string());
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json["thread_id"], "topic-42");
        let back: TaskGrant = serde_json::from_value(json).unwrap();
        assert_eq!(back.thread_id.as_deref(), Some("topic-42"));
        // Thread binding is kernel-owned and authenticated by the grant MAC.
        assert!(!back.verify_mac(TEST_GRANT_HMAC_KEY));
    }

    #[test]
    fn legacy_missing_chain_defaults_but_fails_closed() {
        let g = grant();
        let mut value = serde_json::to_value(g).unwrap();
        let obj = value.as_object_mut().unwrap();
        obj.remove("root_grant_id");
        obj.remove("parent_grant_id");
        obj.remove("mode");
        obj.remove("chain");
        obj.remove("caveat_mac");
        let back: TaskGrant = serde_json::from_value(value).unwrap();
        assert_eq!(back.mode, GrantMode::Live);
        assert!(!back.verify_mac(TEST_GRANT_HMAC_KEY));
    }
}

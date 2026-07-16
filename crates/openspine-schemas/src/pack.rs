//! Capability packs (PRD §11) — reusable policy profiles. Not live authority
//! (PRD §9): they contribute candidate permissions and constraints only.

use serde::{Deserialize, Serialize};

use crate::action::ActionId;
use crate::artifact::Lifecycle;
use crate::egress::EgressClass;
use crate::event::{AccountRole, Connector, EventType, Lane};
use crate::identity::RelationshipKind;
use crate::ids::ArtifactId;
use crate::policy::Constraints;

/// A capability pack's `applies_to` match clause (PRD §11.1/§11.2/§11.3 —
/// deliberately looser/optional than `RouteWhen` since packs are also
/// matched by connector/account-role alone in the system-operations example).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AppliesTo {
    pub event_type: Option<EventType>,
    pub connector: Option<Connector>,
    pub account_role: Option<AccountRole>,
    pub relationship: Option<RelationshipKind>,
    pub channel_trust: Option<crate::event::ChannelTrust>,
    pub verified_source: Option<bool>,
    pub lane: Option<Lane>,
}

/// A capability pack artifact (PRD §11.1/§11.2/§11.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityPack {
    pub id: ArtifactId,
    pub schema_version: u32,
    #[serde(default = "crate::artifact::default_version")]
    pub version: u32,
    pub lifecycle_state: Lifecycle,
    #[serde(default)]
    pub applies_to: AppliesTo,
    #[serde(default)]
    pub candidate_allowed_actions: Vec<ActionId>,
    #[serde(default)]
    pub approval_required: Vec<ActionId>,
    #[serde(default)]
    pub denied_actions: Vec<ActionId>,
    /// AD-060: egress classes this pack authorizes. Packs reference classes
    /// ("may query search-class; may never submit forms"), not individual
    /// endpoints — the connector registry rates endpoints.
    #[serde(default)]
    pub allowed_egress_classes: Vec<EgressClass>,
    #[serde(default)]
    pub constraints: Constraints,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner_control_basic_pack() -> CapabilityPack {
        CapabilityPack {
            id: "owner_control_basic_pack".to_string(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Active,
            applies_to: AppliesTo {
                event_type: Some(EventType::TelegramOwnerMessage),
                relationship: Some(RelationshipKind::Owner),
                channel_trust: Some(crate::event::ChannelTrust::VerifiedOwnerChannel),
                verified_source: Some(true),
                lane: Some(Lane::OwnerControl),
                ..Default::default()
            },
            candidate_allowed_actions: vec![
                ActionId::new("openspine.status.read"),
                ActionId::new("telegram.reply:owner_channel"),
            ],
            approval_required: vec![ActionId::new("connector.enable")],
            denied_actions: vec![
                ActionId::new("email.read_inbox"),
                ActionId::new("email.send"),
            ],
            allowed_egress_classes: vec![],
            constraints: Constraints {
                max_runtime_seconds: Some(120),
                ..Default::default()
            },
        }
    }

    #[test]
    fn round_trips_through_serde() {
        let pack = owner_control_basic_pack();
        let json = serde_json::to_string(&pack).unwrap();
        let back: CapabilityPack = serde_json::from_str(&json).unwrap();
        assert_eq!(pack, back);
    }

    #[test]
    fn allow_and_deny_lists_never_overlap_in_the_fixture() {
        let pack = owner_control_basic_pack();
        for allowed in &pack.candidate_allowed_actions {
            assert!(!pack.denied_actions.contains(allowed));
        }
    }
}

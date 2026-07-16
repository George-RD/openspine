//! Identity records and identity resolution (PRD §5).
//!
//! **Identity is not authority** (D-006): an [`Identity`] deliberately has no
//! `capability_pack_id` or any other live-authority field — it stores
//! entity knowledge only. This is enforced structurally by the absence of
//! such a field, not by a runtime check.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::artifact::ArtifactRef;
use crate::digest::Digest;
use crate::event::ChannelTrust;

/// PRD §5.3 `entity_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Person,
    Organization,
    ServiceAccount,
    Device,
    Agent,
    Unknown,
}

/// PRD §5.3 `identifiers[].type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentifierKind {
    TelegramUserId,
    Email,
    WhatsappNumber,
}

/// PRD §5.3 `identifiers[].verification_method` (union across identifier kinds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentifierVerificationMethod {
    UserConfirmed,
    SetupPairing,
    ConnectorContactMatch,
    DomainVerified,
    Unknown,
    None,
}

/// PRD §5.3 `identifiers[]`. `value_hash` is a hash of the raw identifier
/// value (e.g. a Telegram user id or email address) — the raw value itself
/// is never stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identifier {
    #[serde(rename = "type")]
    pub kind: IdentifierKind,
    pub value_hash: Digest,
    pub verified: bool,
    pub verification_method: IdentifierVerificationMethod,
}

/// PRD §5.3 `relationships[].type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipKind {
    Spouse,
    Colleague,
    Client,
    Vendor,
    Owner,
    Family,
    Unknown,
}

/// PRD §5.3 `relationships[]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relationship {
    #[serde(rename = "type")]
    pub kind: RelationshipKind,
    pub target_id: Ulid,
    /// Confidence in `[0.0, 1.0]`.
    pub confidence: f64,
    pub notes_ref: Option<ArtifactRef>,
}

/// An identity record (PRD §5.3). Stores entity knowledge only — see the
/// module-level note on D-006. There is intentionally no field here that
/// could be mistaken for a live capability grant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub id: Ulid,
    pub display_name: String,
    pub entity_type: EntityType,
    #[serde(default)]
    pub identifiers: Vec<Identifier>,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
    pub schema_version: u32,
}

/// PRD §5.4 `identity_resolution.matched_identifier_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchedIdentifierType {
    TelegramUserId,
    Email,
    Phone,
    Handle,
    Device,
    None,
}

/// The output of identity resolution (PRD §5.4). This is one *input* to
/// route resolution/authority composition — it never grants authority by
/// itself (D-006).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityResolution {
    pub event_id: Ulid,
    pub matched_identity_id: Option<Ulid>,
    /// The resolved principal (AD-146). `Some` ONLY for the owner fast path
    /// in v1. Counterparties and unknowns have `None`, preventing implicit
    /// promotion.
    pub principal_id: Option<Ulid>,
    /// Confidence in `[0.0, 1.0]`.
    pub confidence: f64,
    pub matched_identifier_type: MatchedIdentifierType,
    pub channel_trust: ChannelTrust,
    pub source_verified: bool,
    pub authority_warning: Option<String>,
    pub schema_version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_identity() -> Identity {
        Identity {
            id: Ulid::new(),
            display_name: "George".to_string(),
            entity_type: EntityType::Person,
            identifiers: vec![Identifier {
                kind: IdentifierKind::TelegramUserId,
                value_hash: Digest::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                verified: true,
                verification_method: IdentifierVerificationMethod::UserConfirmed,
            }],
            relationships: vec![],
            schema_version: 1,
        }
    }

    #[test]
    fn round_trips_through_serde() {
        let identity = sample_identity();
        let json = serde_json::to_string(&identity).unwrap();
        let back: Identity = serde_json::from_str(&json).unwrap();
        assert_eq!(identity, back);
    }

    #[test]
    fn identity_json_has_no_authority_field() {
        // Structural proof for D-006 / spec.md's "Identity schemas MUST NOT
        // grant runtime authority": serialize a real identity and assert no
        // authority-shaped key is present anywhere in the object.
        let value = serde_json::to_value(sample_identity()).unwrap();
        let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
        for forbidden in [
            "capability_pack_id",
            "task_grant_id",
            "allowed_actions",
            "route_id",
        ] {
            assert!(
                !keys.iter().any(|k| k.as_str() == forbidden),
                "identity must not carry {forbidden}"
            );
        }
    }

    #[test]
    fn deny_unknown_fields_rejects_capability_pack_id() {
        let mut value = serde_json::to_value(sample_identity()).unwrap();
        value.as_object_mut().unwrap().insert(
            "capability_pack_id".into(),
            serde_json::json!("owner_control_basic_pack"),
        );
        assert!(serde_json::from_value::<Identity>(value).is_err());
    }
}

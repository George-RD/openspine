//! Owner principal aggregate (spec #197, D-002, D-006).
//!
//! `OwnerPrincipal` is the single typed source of truth for the bootstrapped
//! owner's identity. It consolidates the three loose scalars the runtime
//! carries today (`owner_user_id: i64`, `owner_principal_id: Ulid`,
//! `owner_identity_id: Ulid`) into one aggregate minted once at bootstrap and
//! read-only thereafter (AD-146: exactly one owner principal in v1).
//!
//! Like [`crate::principal::Principal`], it carries identity fields only and
//! zero authority fields (D-006): it is the identity-shaped key the kernel
//! composes a grant FOR, never a grant itself. The Telegram channel binding is
//! a private `i64` reachable only through [`OwnerPrincipal::telegram_binding`]
//! (D-002), so no generic kernel code reads the raw channel id directly.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::ids::PrincipalId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerPrincipal {
    /// Typed principal id of the owner (AD-146 single owner).
    pub principal_id: PrincipalId,
    /// The identity record this principal is (the bootstrapped owner identity).
    pub identity_id: Ulid,
    /// The owner's Telegram user id — the only channel binding in v1. Kept
    /// private so it is reachable only via [`OwnerPrincipal::telegram_binding`]
    /// (D-002); no generic kernel code reads the raw i64.
    telegram_binding: i64,
}

impl OwnerPrincipal {
    /// Construct the owner aggregate. Called once at bootstrap; the result is
    /// read-only afterwards.
    pub fn new(principal_id: PrincipalId, identity_id: Ulid, telegram_binding: i64) -> Self {
        Self {
            principal_id,
            identity_id,
            telegram_binding,
        }
    }

    /// The owner's Telegram user id (the v1 channel binding). The only way to
    /// read the raw channel id, keeping channel-specific detail off generic
    /// kernel paths (D-002).
    pub fn telegram_binding(&self) -> i64 {
        self.telegram_binding
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> OwnerPrincipal {
        OwnerPrincipal::new(
            PrincipalId::from(Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap()),
            Ulid::from_string("01BX5ZZKBKACTAV9WEVGEMMVRZ").unwrap(),
            42,
        )
    }

    #[test]
    fn owner_principal_roundtrips_through_serde() {
        let owner = fixture();
        let json = serde_json::to_string(&owner).unwrap();
        let back: OwnerPrincipal = serde_json::from_str(&json).unwrap();
        assert_eq!(owner, back);
        assert_eq!(back.telegram_binding(), 42);
    }

    #[test]
    fn owner_principal_rejects_unknown_fields() {
        let owner = fixture();
        let mut value = serde_json::to_value(&owner).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("is_owner".to_string(), serde_json::json!(true));

        let result: Result<OwnerPrincipal, _> = serde_json::from_value(value);
        assert!(
            result.is_err(),
            "deny_unknown_fields must reject an injected authority-shaped key"
        );
    }

    #[test]
    fn telegram_binding_is_reachable_only_through_accessor() {
        // The field is private; the accessor is the sole read path (D-002).
        assert_eq!(fixture().telegram_binding(), 42);
    }

    /// D-006 (identity is not authority): the owner aggregate is the single
    /// typed identity key the kernel composes a grant FOR — it carries
    /// identity/channel-binding fields only, never a live-authority field.
    /// Pin the exact serialized key set and assert no authority key appears.
    #[test]
    fn owner_principal_carries_no_authority_field() {
        let value = serde_json::to_value(fixture()).unwrap();
        let keys: std::collections::BTreeSet<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            std::collections::BTreeSet::from(["principal_id", "identity_id", "telegram_binding"]),
            "OwnerPrincipal must serialize identity/binding fields only (D-006)"
        );
        for authority_key in [
            "capability_pack_id",
            "allowed_actions",
            "approval_required_actions",
            "denied_actions",
            "allowed_egress_classes",
            "output_channels",
            "caveat_mac",
            "task_token",
        ] {
            assert!(
                !keys.contains(authority_key),
                "OwnerPrincipal must not carry authority field {authority_key} (D-006)"
            );
        }
    }

    /// D-006: `deny_unknown_fields` structurally rejects an injected authority
    /// field, so the owner identity can never smuggle a live grant key.
    #[test]
    fn owner_principal_deserialization_rejects_an_injected_authority_field() {
        let mut value = serde_json::to_value(fixture()).unwrap();
        value.as_object_mut().unwrap().insert(
            "capability_pack_id".to_string(),
            serde_json::json!("owner_control_basic_pack"),
        );
        assert!(
            serde_json::from_value::<OwnerPrincipal>(value).is_err(),
            "deny_unknown_fields must reject an authority-shaped key (D-006)"
        );
    }
}

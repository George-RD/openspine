//! Principal records (AD-146). A Principal is a first-class record that
//! authority composition keys off of. v1 enforces exactly one Principal
//! (the owner). A Principal is NOT authority (D-006): it carries no
//! capability/route/grant fields — it is the identity-shaped key the
//! kernel composes a grant FOR, never a grant itself. Counterparties
//! (even richly bound ones) are NOT principals in v1.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub id: Ulid,
    /// The identity record this principal is. For the owner, the
    /// bootstrapped owner identity.
    pub identity_id: Ulid,
    /// v1: exactly one principal has is_owner == true (AD-146).
    pub is_owner: bool,
    pub schema_version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner_fixture() -> Principal {
        Principal {
            id: Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
            identity_id: Ulid::from_string("01BX5ZZKBKACTAV9WEVGEMMVRZ").unwrap(),
            is_owner: true,
            schema_version: 1,
        }
    }

    /// D-006 (identity is not authority): a `Principal` is the identity-shaped
    /// key authority composes a grant FOR — it must carry only identity fields
    /// and never a live-authority field. The serialized shape is the contract
    /// SQL/audit queries and other crates read, so pin the exact key set.
    #[test]
    fn principal_carries_only_identity_fields_no_authority() {
        let value = serde_json::to_value(owner_fixture()).unwrap();
        let keys: std::collections::BTreeSet<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            std::collections::BTreeSet::from(["id", "identity_id", "is_owner", "schema_version"]),
            "Principal must serialize identity fields only (D-006)"
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
                "Principal must not carry authority field {authority_key} (D-006)"
            );
        }
    }

    /// D-006: `#[serde(deny_unknown_fields)]` structurally rejects an injected
    /// authority field, so an identity record can never smuggle a live grant
    /// key past deserialization.
    #[test]
    fn principal_deserialization_rejects_an_injected_authority_field() {
        let mut value = serde_json::to_value(owner_fixture()).unwrap();
        value.as_object_mut().unwrap().insert(
            "capability_pack_id".to_string(),
            serde_json::json!("owner_control_basic_pack"),
        );
        assert!(
            serde_json::from_value::<Principal>(value).is_err(),
            "deny_unknown_fields must reject an authority-shaped key (D-006)"
        );
    }
}

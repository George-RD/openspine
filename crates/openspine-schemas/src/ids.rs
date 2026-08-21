//! Identifier conventions.
//!
//! OpenSpine has two distinct id shapes (PRD §4–§12): declarative artifacts
//! (routes, agents, workflows, capability packs, policies) use a stable,
//! human-readable slug chosen at authoring time (e.g. `main_assistant_agent`);
//! runtime instances (events, identities, task grants, approvals, selection
//! tokens, model requests, audit events) use a [`ulid::Ulid`] minted at
//! creation time. `ArtifactId` names the former.
//!
//! Most runtime ids are still bare `ulid::Ulid`. Where a runtime id benefits
//! from a distinct type — so it can never be confused with another id or with a
//! raw string — it wears a `#[serde(transparent)]` newtype whose wire form
//! stays the canonical Ulid string, identical to `Ulid::to_string()`.
//! [`PrincipalId`] is the first such newtype (typed owner identity, spec #197);
//! [`IdentityRef`] is its sibling for counterparty identities (spec #220, #222).

use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// A stable, human-authored identifier for a declarative artifact.
pub type ArtifactId = String;

/// Typed identifier for a [`crate::principal::Principal`] (spec #197).
///
/// A `#[serde(transparent)]` newtype over [`ulid::Ulid`]: its wire form is the
/// canonical Ulid string, byte-identical to `Ulid::to_string()`. A `TaskGrant`
/// MAC that sealed a stringified principal Ulid therefore verifies unchanged
/// once its `user` field is retyped to `PrincipalId` (D-005), and non-Ulid
/// `user` shapes (`"owner"`, `"kernel"`, raw i64) become unrepresentable by
/// construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PrincipalId(Ulid);

impl PrincipalId {
    /// The underlying [`ulid::Ulid`].
    pub fn as_ulid(&self) -> Ulid {
        self.0
    }
}

impl From<Ulid> for PrincipalId {
    fn from(id: Ulid) -> Self {
        PrincipalId(id)
    }
}

impl std::fmt::Display for PrincipalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

/// Typed reference to a counterparty [`crate::identity::Identity`] as a
/// provenance origin (spec #220 / #222, D-174).
///
/// The sibling of [`PrincipalId`] under the same discipline: a
/// `#[serde(transparent)]` newtype over [`ulid::Ulid`] whose wire form is the
/// canonical Ulid string, byte-identical to `Ulid::to_string()`. Non-Ulid
/// shapes (a raw `"counterparty"` string, an i64) are unrepresentable by
/// construction, so a worker can never set an origin from a raw string — the
/// compile-time half of the provenance hybrid (#190).
///
/// **Identity is not authority (D-006):** an `IdentityRef` names *whose* data
/// an item is; it carries no capability/route/grant field and grants nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IdentityRef(Ulid);

impl IdentityRef {
    /// The underlying [`ulid::Ulid`].
    pub fn as_ulid(&self) -> Ulid {
        self.0
    }
}

impl From<Ulid> for IdentityRef {
    fn from(id: Ulid) -> Self {
        IdentityRef(id)
    }
}

impl std::fmt::Display for IdentityRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principal_id_serializes_as_plain_ulid_string() {
        let ulid = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let pid = PrincipalId::from(ulid);

        // Transparent: the wire form is the bare Ulid string, so a grant MAC
        // sealed over `Ulid::to_string()` verifies unchanged (D-005).
        let json = serde_json::to_string(&pid).unwrap();
        assert_eq!(json, "\"01ARZ3NDEKTSV4RRFFQ69G5FAV\"");
        assert_eq!(json, serde_json::to_string(&ulid.to_string()).unwrap());
    }

    #[test]
    fn principal_id_roundtrips_through_serde() {
        let pid = PrincipalId::from(Ulid::new());
        let json = serde_json::to_string(&pid).unwrap();
        let back: PrincipalId = serde_json::from_str(&json).unwrap();
        assert_eq!(pid, back);
    }

    #[test]
    fn principal_id_display_matches_inner_ulid() {
        let ulid = Ulid::new();
        let pid = PrincipalId::from(ulid);
        assert_eq!(pid.to_string(), ulid.to_string());
        assert_eq!(pid.as_ulid(), ulid);
    }

    #[test]
    fn identity_ref_serializes_as_plain_ulid_string() {
        let ulid = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let id = IdentityRef::from(ulid);

        // Transparent: the wire form is the bare Ulid string, identical to
        // `PrincipalId` (D-174 sibling discipline).
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"01ARZ3NDEKTSV4RRFFQ69G5FAV\"");
        assert_eq!(json, serde_json::to_string(&ulid.to_string()).unwrap());
    }

    #[test]
    fn identity_ref_roundtrips_through_serde() {
        let id = IdentityRef::from(Ulid::new());
        let json = serde_json::to_string(&id).unwrap();
        let back: IdentityRef = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn identity_ref_json_is_a_bare_string_no_authority_field() {
        // Structural proof for D-006: a transparent Ulid string can carry no
        // object key at all, so no authority-shaped field is representable.
        let value = serde_json::to_value(IdentityRef::from(Ulid::new())).unwrap();
        assert!(
            value.is_string(),
            "IdentityRef must serialize as a bare string"
        );
        assert!(value.as_object().is_none());
    }

    #[test]
    fn identity_ref_display_matches_inner_ulid() {
        let ulid = Ulid::new();
        let id = IdentityRef::from(ulid);
        assert_eq!(id.to_string(), ulid.to_string());
        assert_eq!(id.as_ulid(), ulid);
    }
}

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
//! [`PrincipalId`] is the first such newtype (typed owner identity, spec #197).

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
}

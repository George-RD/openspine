//! Artifact references and the artifact lifecycle (PRD §13, §18).

use serde::{Deserialize, Serialize};

use crate::digest::Digest;

/// A reference to an encrypted, content-addressed artifact blob.
///
/// Private payloads are never stored as raw audit text (PRD §18) — every
/// place that would otherwise hold plaintext (a raw Telegram message, an
/// email body, a model prompt/output, a draft body) holds an `ArtifactRef`
/// instead.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    pub digest: Digest,
    pub schema_version: u32,
}

/// Default content version for a versioned declarative artifact (D-028:
/// "monotonically increasing `v<N>` per artifact id"). Distinct from
/// `schema_version`, which versions the *shape*, not the *content*.
pub fn default_version() -> u32 {
    1
}

/// AD-070 base/overlay namespacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactNamespace {
    Base,
    Overlay,
}

/// Artifact lifecycle state (PRD §13.1).
///
/// `proposed → validated → review_required → approved → active → quarantined | retired`.
/// Only `active` artifacts participate in routing/authority composition;
/// `quarantined` artifacts cannot participate in task grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Proposed,
    Validated,
    ReviewRequired,
    Approved,
    Active,
    Quarantined,
    Retired,
}

/// Whether `from -> to` is a legal lifecycle transition (PRD §13.1/§13.2).
///
/// Only the chain drawn in the PRD is legal: `proposed -> validated ->
/// review_required -> approved -> active -> {quarantined, retired}`.
/// `quarantined` and `retired` are terminal — nothing transitions out of
/// them (moving a quarantined artifact back into use requires a fresh
/// proposal, not a lifecycle transition, per "quarantined artifacts cannot
/// participate in task grants").
pub fn can_transition(from: Lifecycle, to: Lifecycle) -> bool {
    use Lifecycle::*;
    matches!(
        (from, to),
        (Proposed, Validated)
            | (Validated, ReviewRequired)
            | (ReviewRequired, Approved)
            | (Approved, Active)
            | (Active, Approved)
            | (Active, Quarantined)
            | (Active, Retired)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_chain_is_legal() {
        use Lifecycle::*;
        assert!(can_transition(Proposed, Validated));
        assert!(can_transition(Validated, ReviewRequired));
        assert!(can_transition(ReviewRequired, Approved));
        assert!(can_transition(Approved, Active));
        assert!(can_transition(Active, Quarantined));
        assert!(can_transition(Active, Retired));
    }

    #[test]
    fn terminal_states_have_no_outgoing_transitions() {
        for to in [
            Lifecycle::Proposed,
            Lifecycle::Validated,
            Lifecycle::ReviewRequired,
            Lifecycle::Approved,
            Lifecycle::Active,
            Lifecycle::Quarantined,
            Lifecycle::Retired,
        ] {
            assert!(!can_transition(Lifecycle::Quarantined, to));
            assert!(!can_transition(Lifecycle::Retired, to));
        }
    }

    #[test]
    fn no_skipping_stages() {
        assert!(!can_transition(Lifecycle::Proposed, Lifecycle::Active));
        assert!(!can_transition(Lifecycle::Proposed, Lifecycle::Approved));
        assert!(!can_transition(Lifecycle::Active, Lifecycle::Proposed));
    }

    #[test]
    fn artifact_ref_rejects_unknown_fields() {
        let json = serde_json::json!({
            "digest": format!("sha256:{}", "a".repeat(64)),
            "schema_version": 1,
            "unexpected": "field",
        });
        let err = serde_json::from_value::<ArtifactRef>(json).unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }
}

//! Artifact generation/lineage model (agent-OS design log, non-retrofittable
//! set; change `define-lineage-and-eval-store`).
//!
//! Lineage is the derivation/provenance model for an artifact — which parent
//! artifact(s) it was generated from and its generation depth. It is DISTINCT
//! from `version` (the per-artifact content version, D-028: "monotonically
//! increasing `v<N>` per artifact id"): `version` tracks edits to ONE artifact
//! over time; `lineage` tracks how an artifact came to BE.
//!
//! Both fields are part of the non-retrofittable set — they MUST exist in the
//! schema before any generation/derivation feature ships, because adding them
//! later cannot backfill the rows that already exist without that column.

use serde::{Deserialize, Serialize};

/// A reference to a parent artifact in a lineage chain. Identifies the parent
/// by its full declarative identity — kind, artifact id, and the content
/// version the child derived from — matching the `proposed_artifacts`
/// `UNIQUE(kind, artifact_id, version)` key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineageParent {
    pub kind: String,
    pub artifact_id: String,
    pub version: u32,
}

/// The generation/lineage of an artifact (distinct from its content
/// `version`).
///
/// `generation` is the derivation depth: a root artifact (no parents) has
/// `generation == 0`; a child derived from a generation-N parent has
/// `generation == N + 1`. An artifact MAY derive from more than one parent
/// (a merge/synthesis), in which case its generation is one greater than the
/// deepest parent.
///
/// This type carries no authority and grants nothing — it is provenance
/// metadata only (identity-is-not-authority, D-006).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactLineage {
    pub generation: u32,
    pub parents: Vec<LineageParent>,
}

impl ArtifactLineage {
    /// Lineage for a root artifact with no parents (generation 0) — the
    /// default for any freshly proposed artifact that was not derived from
    /// another artifact.
    pub fn root() -> Self {
        Self {
            generation: 0,
            parents: Vec::new(),
        }
    }

    /// True for artifacts derived from one or more parents.
    pub fn is_derived(&self) -> bool {
        !self.parents.is_empty()
    }

    /// Whether the lineage is internally consistent: a derived artifact
    /// (parents present) MUST have `generation >= 1`; a root artifact
    /// (no parents) MUST have `generation == 0`. This is a pure sanity check
    /// on the shape — it cannot verify the parent references resolve, only
    /// that the self-described depth agrees with the presence of parents.
    pub fn is_consistent(&self) -> bool {
        if self.parents.is_empty() {
            self.generation == 0
        } else {
            self.generation >= 1
        }
    }
}

impl Default for ArtifactLineage {
    fn default() -> Self {
        Self::root()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_has_generation_zero_and_no_parents() {
        let root = ArtifactLineage::root();
        assert_eq!(root.generation, 0);
        assert!(root.parents.is_empty());
        assert!(!root.is_derived());
        assert!(root.is_consistent());
    }

    #[test]
    fn default_is_root() {
        assert_eq!(ArtifactLineage::default(), ArtifactLineage::root());
    }

    #[test]
    fn derived_lineage_round_trips_through_json() {
        let lineage = ArtifactLineage {
            generation: 2,
            parents: vec![
                LineageParent {
                    kind: "route".to_string(),
                    artifact_id: "main_route".to_string(),
                    version: 1,
                },
                LineageParent {
                    kind: "route".to_string(),
                    artifact_id: "fallback_route".to_string(),
                    version: 3,
                },
            ],
        };
        assert!(lineage.is_derived());
        assert!(lineage.is_consistent());

        let json = serde_json::to_string(&lineage).unwrap();
        let back: ArtifactLineage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, lineage);
    }

    #[test]
    fn root_round_trips_through_json() {
        let json = serde_json::to_string(&ArtifactLineage::root()).unwrap();
        let back: ArtifactLineage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ArtifactLineage::root());
    }

    #[test]
    fn inconsistent_lineage_is_flagged() {
        // Parents present but generation 0 — inconsistent.
        let bad = ArtifactLineage {
            generation: 0,
            parents: vec![LineageParent {
                kind: "route".to_string(),
                artifact_id: "p".to_string(),
                version: 1,
            }],
        };
        assert!(!bad.is_consistent());

        // No parents but generation > 0 — inconsistent.
        let bad2 = ArtifactLineage {
            generation: 5,
            parents: Vec::new(),
        };
        assert!(!bad2.is_consistent());
    }

    #[test]
    fn rejects_unknown_fields() {
        let json = serde_json::json!({
            "generation": 1,
            "parents": [],
            "unexpected": "field",
        });
        let err = serde_json::from_value::<ArtifactLineage>(json).unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn parent_rejects_unknown_fields() {
        let json = serde_json::json!({
            "kind": "route",
            "artifact_id": "x",
            "version": 1,
            "extra": true,
        });
        let err = serde_json::from_value::<LineageParent>(json).unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }
}

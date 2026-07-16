//! Lineage column round-trip tests for `proposed_artifacts` (change
//! `define-lineage-and-eval-store`). Done-when: artifact rows can carry
//! lineage.

use jiff::Timestamp;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::lineage::{ArtifactLineage, LineageParent};
use ulid::Ulid;

use super::proposed_artifacts::ProposedArtifact;
use super::Store;

fn row_with(action_request_id: Ulid, lineage: Option<ArtifactLineage>) -> ProposedArtifact {
    ProposedArtifact {
        id: Ulid::new(),
        kind: "route".to_string(),
        artifact_id: "main_route".to_string(),
        version: 1,
        state: Lifecycle::Proposed,
        yaml_digest: format!("sha256:{}", "a".repeat(64)),
        task_grant_id: Ulid::new(),
        action_request_id: Some(action_request_id),
        proposed_at: Timestamp::now(),
        lineage,
    }
}

#[test]
fn root_lineage_round_trips_on_artifact_row() {
    let store = Store::open_in_memory().expect("open store");
    let action_request_id = Ulid::new();
    let row = row_with(action_request_id, Some(ArtifactLineage::root()));
    store
        .insert_proposed_artifact(&row)
        .expect("insert root artifact");

    let loaded = store
        .find_proposed_artifact_by_action_request(action_request_id)
        .expect("find")
        .expect("row must exist");
    assert_eq!(loaded.lineage, Some(ArtifactLineage::root()));
    let lineage = loaded.lineage.expect("present");
    assert!(!lineage.is_derived());
    assert!(lineage.is_consistent());
}

#[test]
fn derived_lineage_round_trips_on_artifact_row() {
    let store = Store::open_in_memory().expect("open store");
    let action_request_id = Ulid::new();
    let lineage = ArtifactLineage {
        generation: 2,
        parents: vec![
            LineageParent {
                kind: "route".to_string(),
                artifact_id: "parent_a".to_string(),
                version: 1,
            },
            LineageParent {
                kind: "route".to_string(),
                artifact_id: "parent_b".to_string(),
                version: 3,
            },
        ],
    };
    assert!(lineage.is_derived());
    assert!(lineage.is_consistent());

    let mut row = row_with(action_request_id, Some(lineage.clone()));
    row.artifact_id = "derived_route".to_string();
    store
        .insert_proposed_artifact(&row)
        .expect("insert derived artifact");

    let loaded = store
        .find_proposed_artifact_by_action_request(action_request_id)
        .expect("find")
        .expect("row must exist");
    assert_eq!(loaded.lineage, Some(lineage));
    let lineage = loaded.lineage.expect("present");
    assert_eq!(lineage.generation, 2);
    assert_eq!(lineage.parents.len(), 2);
    assert_eq!(lineage.parents[0].artifact_id, "parent_a");
    assert_eq!(lineage.parents[1].version, 3);
}

#[test]
fn unknown_lineage_is_none_not_root() {
    // Legacy / unknown provenance is represented as None — never silently
    // rewritten as generation-0 root.
    let store = Store::open_in_memory().expect("open store");
    let action_request_id = Ulid::new();
    let row = row_with(action_request_id, None);
    store
        .insert_proposed_artifact(&row)
        .expect("insert unknown-lineage artifact");

    let loaded = store
        .find_proposed_artifact_by_action_request(action_request_id)
        .expect("find")
        .expect("row must exist");
    assert_eq!(loaded.lineage, None);
}

#[test]
fn lineage_is_distinct_from_version() {
    // A derived artifact can have content version 1 while generation is 2 —
    // version tracks edits of THIS artifact; generation tracks derivation
    // depth. The two counters MUST NOT be conflated.
    let store = Store::open_in_memory().expect("open store");
    let action_request_id = Ulid::new();
    let mut row = row_with(
        action_request_id,
        Some(ArtifactLineage {
            generation: 2,
            parents: vec![LineageParent {
                kind: "route".to_string(),
                artifact_id: "parent".to_string(),
                version: 5,
            }],
        }),
    );
    row.version = 1;
    store.insert_proposed_artifact(&row).expect("insert");

    let loaded = store
        .find_proposed_artifact_by_action_request(action_request_id)
        .expect("find")
        .expect("row must exist");
    assert_eq!(loaded.version, 1);
    let lineage = loaded.lineage.expect("lineage present");
    assert_eq!(lineage.generation, 2);
    assert_ne!(loaded.version, lineage.generation);
}

#[test]
fn inconsistent_generation_zero_with_parents_is_rejected() {
    let store = Store::open_in_memory().expect("open store");
    let row = row_with(
        Ulid::new(),
        Some(ArtifactLineage {
            generation: 0,
            parents: vec![LineageParent {
                kind: "route".into(),
                artifact_id: "parent".into(),
                version: 1,
            }],
        }),
    );
    let err = store
        .insert_proposed_artifact(&row)
        .expect_err("must reject");
    assert!(matches!(err, super::StoreError::InconsistentLineage(_)));
}

#[test]
fn inconsistent_positive_generation_without_parents_is_rejected() {
    let store = Store::open_in_memory().expect("open store");
    let row = row_with(
        Ulid::new(),
        Some(ArtifactLineage {
            generation: 1,
            parents: Vec::new(),
        }),
    );
    let err = store
        .insert_proposed_artifact(&row)
        .expect_err("must reject");
    assert!(matches!(err, super::StoreError::InconsistentLineage(_)));
}

#[test]
fn stored_inconsistent_lineage_fails_closed_on_load() {
    let store = Store::open_in_memory().expect("open store");
    let action_request_id = Ulid::new();
    let row = row_with(action_request_id, Some(ArtifactLineage::root()));
    store.insert_proposed_artifact(&row).expect("insert");
    {
        let conn = store.conn.lock();
        conn.execute(
            "UPDATE proposed_artifacts SET lineage_json = ?1 WHERE id = ?2",
            rusqlite::params![
                r#"{"generation":0,"parents":[{"kind":"route","artifact_id":"p","version":1}]}"#,
                row.id.to_string(),
            ],
        )
        .expect("corrupt lineage");
    }
    let err = store
        .find_proposed_artifact_by_action_request(action_request_id)
        .expect_err("must fail closed");
    assert!(matches!(err, super::StoreError::InconsistentLineage(_)));
}

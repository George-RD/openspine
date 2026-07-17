//! Tests for the learned overlay artifact store (AD-023/070/071), split out
//! of `learned_artifacts.rs` to keep that module under the 500-line gate.

use super::*;
use openspine_schemas::digest::digest_of_bytes;

fn sample() -> LearnedArtifact {
    LearnedArtifact {
        kind: "route".into(),
        artifact_id: "learned-route".into(),
        version: 1,
        namespace: ArtifactNamespace::Overlay,
        provenance: Provenance::ProducedBy {
            source_event_id: Ulid::new(),
            source_exchange: ArtifactRef {
                digest: digest_of_bytes(b"exchange"),
                schema_version: 1,
            },
        },
        accepted_via: None,
        learned_at: Timestamp::now(),
        compatibility: CompatibilityStatus::Compatible,
        nomination: NominationStatus::None,
        pending_reconfirmation_id: None,
        pending_yaml_digest: None,
        accepted_dependency_fingerprint: None,
        source_path: None,
        accepted_base_epoch: None,
    }
}

#[test]
fn learned_artifact_round_trip_preserves_typed_provenance() {
    let store = Store::open_in_memory().unwrap();
    let artifact = sample();
    store.record_learned_artifact(&artifact).unwrap();
    assert_eq!(store.list_learned_artifacts().unwrap(), vec![artifact]);
}

#[test]
fn base_namespace_cannot_be_recorded_as_learned() {
    let store = Store::open_in_memory().unwrap();
    let mut artifact = sample();
    artifact.namespace = ArtifactNamespace::Base;
    assert!(store.record_learned_artifact(&artifact).is_err());
}

#[test]
fn owner_accepted_is_durable_across_round_trip() {
    let store = Store::open_in_memory().unwrap();
    let mut artifact = sample();
    artifact.compatibility = CompatibilityStatus::OwnerAccepted;
    store.record_learned_artifact(&artifact).unwrap();
    assert_eq!(
        store.list_learned_artifacts().unwrap()[0].compatibility,
        CompatibilityStatus::OwnerAccepted
    );
}

#[test]
fn accepted_dependency_fingerprint_round_trips() {
    // The canonical non-empty accepted-dangling anchor must survive a
    // record/list round-trip (catches a dropped INSERT/SELECT/migration field).
    let store = Store::open_in_memory().unwrap();
    let mut artifact = sample();
    artifact.compatibility = CompatibilityStatus::OwnerAccepted;
    artifact.accepted_dependency_fingerprint = Some(dependency_fingerprint(&[
        "agent:removed".to_string(),
        "workflow:gone".to_string(),
    ]));
    store.record_learned_artifact(&artifact).unwrap();
    let stored = store.list_learned_artifacts().unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(
        stored[0].accepted_dependency_fingerprint,
        artifact.accepted_dependency_fingerprint
    );
}

#[test]
fn accepted_dependency_fingerprint_migration_is_idempotent() {
    // A pre-existing DB whose learned_artifacts table predates the
    // accepted_dependency_fingerprint column must gain it via the idempotent
    // migration, persist a recorded anchor, and reopen without error.
    let dir = std::env::temp_dir().join(format!("overlay_mig_{}", Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("kernel.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE learned_artifacts (
               kind TEXT NOT NULL, artifact_id TEXT NOT NULL, version INTEGER NOT NULL,
               namespace TEXT NOT NULL DEFAULT 'overlay', provenance TEXT NOT NULL,
               accepted_via TEXT, learned_at TEXT NOT NULL,
               compatibility TEXT NOT NULL DEFAULT 'compatible',
               nomination TEXT NOT NULL DEFAULT 'none', pending_reconfirmation_id TEXT,
               pending_yaml_digest TEXT, source_path TEXT, accepted_base_epoch TEXT,
               PRIMARY KEY(kind, artifact_id, version));",
        )
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    let mut artifact = sample();
    artifact.compatibility = CompatibilityStatus::OwnerAccepted;
    artifact.accepted_dependency_fingerprint =
        Some(dependency_fingerprint(&["agent:removed".to_string()]));
    store.record_learned_artifact(&artifact).unwrap();
    assert_eq!(
        store.list_learned_artifacts().unwrap()[0].accepted_dependency_fingerprint,
        artifact.accepted_dependency_fingerprint
    );
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.list_learned_artifacts().unwrap()[0].accepted_dependency_fingerprint,
        artifact.accepted_dependency_fingerprint
    );
}

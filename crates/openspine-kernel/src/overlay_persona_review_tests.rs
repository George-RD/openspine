use super::*;
use crate::store::learned_artifacts::NominationStatus;
use jiff::Timestamp;
use openspine_schemas::artifact::ArtifactNamespace;
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::provenance::ProvenanceOrigin;

fn produced_persona(
    store: &Store,
    artifacts: &ArtifactStore,
    id: &str,
    version: u32,
) -> LearnedArtifact {
    let exchange = artifacts.put(b"review producing exchange").unwrap();
    let event = store
        .append_audit(
            "artifact.superseded",
            None,
            None,
            None,
            None,
            &[],
            std::slice::from_ref(&exchange),
        )
        .unwrap();
    let at: Timestamp = "2026-01-01T00:00:00Z".parse().unwrap();
    LearnedArtifact {
        kind: "persona".into(),
        artifact_id: id.into(),
        version,
        namespace: ArtifactNamespace::Overlay,
        provenance: Provenance::ProducedBy {
            source_event_id: event.id,
            source_exchange: exchange,
            source_scope: ProvenanceOrigin::system(),
        },
        accepted_via: None,
        learned_at: at,
        compatibility: CompatibilityStatus::Compatible,
        nomination: NominationStatus::None,
        pending_reconfirmation_id: None,
        pending_yaml_digest: Some(digest_of_bytes(b"persona yaml").to_string()),
        accepted_dependency_fingerprint: None,
        source_path: None,
        accepted_base_epoch: None,
    }
}

#[test]
fn review_capture_matches_startup_findings_for_current_evidence() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(root.path().join("objects"), [5; 32]).unwrap();
    let row = produced_persona(&store, &artifacts, "persona", 1);

    let startup = CapturedPersonaProvenance::capture_for_startup(
        &store,
        &artifacts,
        std::slice::from_ref(&row),
    )
    .unwrap()
    .evaluate();
    let review = CapturedPersonaProvenance::capture_for_review(
        &store,
        &artifacts,
        std::slice::from_ref(&row),
    )
    .unwrap()
    .evaluate();

    assert_eq!(review, startup);
}

#[test]
fn review_capture_rejects_duplicate_identity_before_audit_reads() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(root.path().join("objects"), [5; 32]).unwrap();
    let row = produced_persona(&store, &artifacts, "persona", 1);
    store.break_audit_for_test();

    let error =
        CapturedPersonaProvenance::capture_for_review(&store, &artifacts, &[row.clone(), row])
            .err()
            .unwrap();
    assert!(error.to_string().contains("duplicate persona provenance"));
}

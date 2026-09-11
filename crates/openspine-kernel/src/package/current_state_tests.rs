use super::*;
use crate::store::learned_artifacts::{
    CompatibilityStatus, NominationStatus, Provenance,
};
use crate::store::proposed_artifacts::ProposedArtifact;
use jiff::Timestamp;
use openspine_schemas::artifact::{ArtifactNamespace, ArtifactRef};
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::route::{Route, RouteEffect};
use std::fs;
use tempfile::TempDir;
use ulid::Ulid;

const TEST_KEY: [u8; 32] = [41; 32];

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let source = entry.unwrap().path();
        let destination = destination.join(source.file_name().unwrap());
        if source.is_dir() {
            copy_tree(&source, &destination);
        } else {
            fs::copy(source, destination).unwrap();
        }
    }
}

fn fixture() -> (TempDir, PathBuf, Store, ArtifactStore) {
    let root = tempfile::tempdir().unwrap();
    let data_root = root.path().join("data");
    fs::create_dir_all(&data_root).unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(data_root.join("artifacts"), TEST_KEY).unwrap();
    (root, data_root, store, artifacts)
}

fn configured_base(root: &Path) -> PathBuf {
    let destination = root.join("configured-base");
    copy_tree(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra"),
        &destination,
    );
    destination
}

fn route_yaml(id: &str, version: u32) -> Vec<u8> {
    serde_yaml::to_string(&Route {
        id: id.into(),
        schema_version: 1,
        version,
        lifecycle_state: Lifecycle::Active,
        priority: None,
        effect: RouteEffect::Allow,
        when: Default::default(),
        agent: None,
        workflow: None,
        capability_pack: None,
        persona: None,
    })
    .unwrap()
    .into_bytes()
}

fn learned_route(id: &str, version: u32, digest: &str) -> LearnedArtifact {
    LearnedArtifact {
        kind: "route".into(),
        artifact_id: id.into(),
        version,
        namespace: ArtifactNamespace::Overlay,
        provenance: Provenance::ProducedBy {
            source_event_id: Ulid::new(),
            source_exchange: ArtifactRef {
                digest: digest_of_bytes(b"exchange"),
                schema_version: 1,
            },
            source_scope: openspine_schemas::provenance::ProvenanceOrigin::system(),
        },
        accepted_via: None,
        learned_at: Timestamp::now(),
        compatibility: CompatibilityStatus::Compatible,
        nomination: NominationStatus::None,
        pending_reconfirmation_id: None,
        pending_yaml_digest: Some(digest.into()),
        accepted_dependency_fingerprint: None,
        source_path: None,
        accepted_base_epoch: None,
    }
}

fn active_route(store: &Store, id: &str, version: u32, digest: &str) {
    let proposal_id = Ulid::new();
    store
        .insert_proposed_artifact(&ProposedArtifact {
            id: proposal_id,
            kind: "route".into(),
            artifact_id: id.into(),
            version,
            state: Lifecycle::Proposed,
            yaml_digest: digest.into(),
            task_grant_id: Ulid::new(),
            action_request_id: None,
            proposed_at: Timestamp::now(),
            lineage: None,
        })
        .unwrap();
    store
        .force_proposed_artifact_state_for_test(proposal_id, Lifecycle::Active)
        .unwrap();
}

#[test]
fn capture_uses_the_configured_nondefault_base_and_owns_its_typed_state() {
    let (root, data_root, store, artifacts) = fixture();
    let base = configured_base(root.path());

    let captured = CapturedCurrentState::capture(&base, "lyra", &data_root, &store, &artifacts)
        .unwrap();
    assert_eq!(captured.base.configured_path, base);
    assert_eq!(captured.base.identity.package_id, "lyra");
    assert!(captured
        .base
        .registry
        .agents
        .contains_key("main_assistant_agent"));

    fs::write(base.join("agents/main_assistant_agent.yaml"), b"not yaml").unwrap();
    fs::remove_file(base.join("package.yaml")).unwrap();
    assert!(captured
        .base
        .registry
        .agents
        .contains_key("main_assistant_agent"));
    assert_eq!(captured.base.identity.package_id, "lyra");
}

#[test]
fn capture_rejects_a_different_configured_product_identity() {
    let (root, data_root, store, artifacts) = fixture();
    let base = configured_base(root.path());
    let manifest = fs::read_to_string(base.join("package.yaml")).unwrap();
    fs::write(
        base.join("package.yaml"),
        manifest.replacen("id: lyra", "id: bell", 1),
    )
    .unwrap();

    assert!(matches!(
        CapturedCurrentState::capture(&base, "lyra", &data_root, &store, &artifacts),
        Err(CurrentStateCaptureError::PackageIdMismatch)
    ));
}

#[test]
fn capture_marks_a_committed_active_overlay_whose_file_is_missing() {
    let (root, data_root, store, artifacts) = fixture();
    let base = configured_base(root.path());
    let yaml = route_yaml("missing-active", 1);
    let digest = digest_of_bytes(&yaml).to_string();
    artifacts.put(&yaml).unwrap();
    active_route(&store, "missing-active", 1, &digest);
    store
        .record_learned_artifact(&learned_route("missing-active", 1, &digest))
        .unwrap();

    let captured = CapturedCurrentState::capture(&base, "lyra", &data_root, &store, &artifacts)
        .unwrap();
    let control = &captured.overlay.controls[&(
        "route".into(),
        "missing-active".into(),
        1,
    )];
    assert_eq!(control.lifecycle, Some(Lifecycle::Active));
    assert_eq!(control.highest_active_version, Some(1));
    assert!(!control.source_present);
    assert!(control.recoverable_blob_present);
}

#[test]
fn captured_overlay_controls_do_not_adopt_later_activation() {
    let (root, data_root, store, artifacts) = fixture();
    let base = configured_base(root.path());
    let overlay = data_root.join("artifacts.d/routes");
    fs::create_dir_all(&overlay).unwrap();
    let yaml = route_yaml("stable", 1);
    let digest = digest_of_bytes(&yaml).to_string();
    fs::write(
        overlay.join(crate::artifact_loader::overlay_filename("stable", 1)),
        &yaml,
    )
    .unwrap();
    artifacts.put(&yaml).unwrap();
    active_route(&store, "stable", 1, &digest);
    store
        .record_learned_artifact(&learned_route("stable", 1, &digest))
        .unwrap();

    let captured = CapturedCurrentState::capture(&base, "lyra", &data_root, &store, &artifacts)
        .unwrap();
    let v2 = route_yaml("stable", 2);
    let d2 = digest_of_bytes(&v2).to_string();
    active_route(&store, "stable", 2, &d2);

    assert_eq!(
        captured.overlay.controls[&("route".into(), "stable".into(), 1)]
            .highest_active_version,
        Some(1)
    );
}

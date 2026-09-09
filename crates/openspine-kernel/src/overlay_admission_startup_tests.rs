//! Exercise the real startup adapter, not the pure evaluator's output alone.
use crate::artifact_store::ArtifactStore;
use crate::store::learned_artifacts::{
    CompatibilityStatus, LearnedArtifact, NominationStatus, Provenance,
};
use crate::store::Store;
use openspine_schemas::artifact::ArtifactNamespace;
use openspine_schemas::digest::digest_of_bytes;

#[test]
fn startup_collision_reuses_only_exact_version_reconfirmation() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("base");
    let data = root.path().join("data");
    let overlay = data.join("artifacts.d/routes");
    std::fs::create_dir_all(base.join("routes")).unwrap();
    std::fs::create_dir_all(&overlay).unwrap();
    let yaml = |version| {
        format!(
            "id: collision\nschema_version: 1\nversion: {version}\nlifecycle_state: active\neffect: allow\n"
        )
    };
    std::fs::write(base.join("routes/base.yaml"), yaml(1)).unwrap();
    let path = overlay.join("current.yaml");
    std::fs::write(&path, yaml(2)).unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(data.join("artifacts"), [3u8; 32]).unwrap();
    let at = "2026-09-09T00:00:00Z".parse::<jiff::Timestamp>().unwrap();
    let stale_id = ulid::Ulid::new();
    let current_id = ulid::Ulid::new();
    for (version, request_id) in [(1, stale_id), (2, current_id)] {
        store
            .record_learned_artifact(&LearnedArtifact {
                kind: "route".into(),
                artifact_id: "collision".into(),
                version,
                namespace: ArtifactNamespace::Overlay,
                provenance: Provenance::LegacyMigration { discovered_at: at },
                accepted_via: None,
                learned_at: at,
                compatibility: CompatibilityStatus::Compatible,
                nomination: NominationStatus::None,
                pending_reconfirmation_id: Some(request_id),
                pending_yaml_digest: Some(digest_of_bytes(yaml(version).as_bytes()).to_string()),
                accepted_dependency_fingerprint: None,
                source_path: Some(path.to_string_lossy().into_owned()),
                accepted_base_epoch: None,
            })
            .unwrap();
    }
    let payload = artifacts.put(yaml(2).as_bytes()).unwrap();
    crate::overlay_compat::ensure_reconfirm_request(
        &store,
        "route",
        "collision",
        2,
        current_id,
        payload,
    )
    .unwrap();

    let startup = crate::overlay_startup::load(&base, &data, &store, &artifacts).unwrap();
    assert_eq!(startup.pending_reconfirm_buttons.len(), 1);
    assert_eq!(startup.pending_reconfirm_buttons[0].0, current_id);
    assert!(store.find_action_request(stale_id).unwrap().is_none());
    assert_eq!(startup.registry.routes.len(), 1);
    assert_eq!(startup.registry.routes[0].version, 1, "base stays live");
}

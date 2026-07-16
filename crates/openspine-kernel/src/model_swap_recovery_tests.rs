use std::path::Path;

use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::model_swap::{ModelRole, ModelSwapManifest};
use tempfile::tempdir;
use ulid::Ulid;

use crate::artifact_store::ArtifactStore;
use crate::store::proposed_artifacts::ProposedArtifact;
use crate::store::Store;

fn swap_yaml_version(provider: &str, lifecycle: Lifecycle, version: u32) -> String {
    serde_yaml::to_string(&ModelSwapManifest {
        id: "base".to_string(),
        version,
        lifecycle_state: lifecycle,
        role: ModelRole::Base,
        target_provider_id: provider.to_string(),
        golden_set_id: "model_swap_default".to_string(),
        golden_set_result: None,
    })
    .unwrap()
}

fn swap_yaml(provider: &str, lifecycle: Lifecycle) -> String {
    swap_yaml_version(provider, lifecycle, 1)
}

fn fixture() -> (Store, ArtifactStore, std::path::PathBuf, String) {
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(tempdir().unwrap().keep(), [7; 32]).unwrap();
    let overlay = tempdir().unwrap().keep();
    let model_swaps = overlay.join("model_swaps");
    std::fs::create_dir_all(&model_swaps).unwrap();
    (
        store,
        artifacts,
        model_swaps,
        swap_yaml("provider-a", Lifecycle::Proposed),
    )
}

fn persist_active_provenance(store: &Store, artifacts: &ArtifactStore, reviewed: &str) {
    let reviewed_ref = artifacts.put(reviewed.as_bytes()).unwrap();
    let id = Ulid::new();
    let grant_id = Ulid::new();
    store
        .insert_proposed_artifact(&ProposedArtifact {
            id,
            kind: "model_swap".to_string(),
            artifact_id: "base".to_string(),
            version: 1,
            state: Lifecycle::Proposed,
            yaml_digest: reviewed_ref.digest.as_str().to_string(),
            task_grant_id: grant_id,
            action_request_id: None,
            proposed_at: jiff::Timestamp::now(),
            lineage: None,
        })
        .unwrap();
    store
        .set_proposed_artifact_state(id, Lifecycle::Proposed, Lifecycle::Validated)
        .unwrap();
    store
        .force_proposed_artifact_state_for_test(id, Lifecycle::Approved)
        .unwrap();
    store
        .activate_with_audit(
            id,
            &openspine_schemas::action::ActionId::new("artifact.activate"),
            grant_id,
            &reviewed_ref,
        )
        .unwrap();
}

fn write_pending_version(model_swaps: &Path, yaml: &str, version: u32) -> std::path::PathBuf {
    let path = model_swaps.join(format!("base-v{version}.pending"));
    std::fs::write(&path, yaml).unwrap();
    path
}

fn write_pending(model_swaps: &Path, yaml: &str) -> std::path::PathBuf {
    write_pending_version(model_swaps, yaml, 1)
}

#[test]
fn pre_commit_pending_crash_discards_pending_and_keeps_old_disk() {
    let (store, artifacts, model_swaps, reviewed) = fixture();
    persist_active_provenance(&store, &artifacts, &reviewed);
    std::fs::write(
        model_swaps.join("base-v1.yaml"),
        swap_yaml("provider-a", Lifecycle::Active),
    )
    .unwrap();
    let pending = write_pending_version(
        &model_swaps,
        &swap_yaml_version("provider-b", Lifecycle::Active, 2),
        2,
    );

    super::model_swap_recovery::reconcile_model_swap_overlay(
        &store,
        &artifacts,
        model_swaps.parent().unwrap(),
    )
    .unwrap();

    assert!(!pending.exists());
    assert!(model_swaps.join("base-v1.yaml").exists());
    let mut registry = crate::artifact_loader::ArtifactRegistry::default();
    crate::artifact_loader::load_registry_into(&mut registry, model_swaps.parent().unwrap())
        .unwrap();
    assert_eq!(registry.model_swaps["base"].version, 1);
    assert_eq!(
        store
            .count_audit_events_of_kind("artifact.activation_recovered")
            .unwrap(),
        1
    );
}

#[test]
fn post_commit_pending_crash_restores_matching_reviewed_overlay() {
    let (store, artifacts, model_swaps, reviewed) = fixture();
    persist_active_provenance(&store, &artifacts, &reviewed);
    let pending = write_pending(&model_swaps, &swap_yaml("provider-a", Lifecycle::Active));

    super::model_swap_recovery::reconcile_model_swap_overlay(
        &store,
        &artifacts,
        model_swaps.parent().unwrap(),
    )
    .unwrap();

    assert!(!pending.exists());
    assert!(model_swaps.join("base-v1.yaml").exists());
    let mut registry = crate::artifact_loader::ArtifactRegistry::default();
    crate::artifact_loader::load_registry_into(&mut registry, model_swaps.parent().unwrap())
        .unwrap();
    assert_eq!(registry.model_swaps["base"].version, 1);
    assert_eq!(
        store
            .count_audit_events_of_kind("artifact.activation_recovered")
            .unwrap(),
        1
    );
}

#[test]
fn tampered_pending_is_quarantined_by_provenance_digest() {
    let (store, artifacts, model_swaps, reviewed) = fixture();
    persist_active_provenance(&store, &artifacts, &reviewed);
    let pending = write_pending(
        &model_swaps,
        &swap_yaml("attacker-provider", Lifecycle::Active),
    );

    super::model_swap_recovery::reconcile_model_swap_overlay(
        &store,
        &artifacts,
        model_swaps.parent().unwrap(),
    )
    .unwrap();

    assert!(!pending.exists());
    assert!(model_swaps.join("base-v1.quarantine").exists());
    assert!(!model_swaps.join("base-v1.yaml").exists());
    assert_eq!(
        store
            .count_audit_events_of_kind("artifact.activation_recovery_failed")
            .unwrap(),
        1
    );
}

use super::tests::{fixture, insert_active_proposal, learned_row, overlay_file, overlay_yaml};
use crate::store::learned_artifacts::{
    dependency_fingerprint, CompatibilityStatus, LearnedArtifact,
};
use crate::store::proposed_artifacts::ProposedArtifact;
use jiff::Timestamp;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::route::Route;
use tempfile::tempdir;
use ulid::Ulid;

#[test]
fn highest_active_prunes_stale_loaded_lower_version() {
    let (store, artifacts, data_dir, overlay_dir) = fixture();
    let lyra_dir = tempdir().unwrap().keep();
    std::fs::create_dir_all(overlay_dir.join("routes")).unwrap();

    let v2 = overlay_yaml("r1", 2);
    let v2_digest = digest_of_bytes(v2.as_bytes()).to_string();
    let _v2_ref = artifacts.put(v2.as_bytes()).unwrap();
    insert_active_proposal(&store, "r1", 2, &v2_digest);
    store
        .record_learned_artifact(&learned_row("r1", 2, &v2_digest))
        .unwrap();

    let v1 = overlay_yaml("r1", 1);
    let v1_digest = digest_of_bytes(v1.as_bytes()).to_string();
    std::fs::write(overlay_file(&overlay_dir, "route", "r1", 1), &v1).unwrap();
    store
        .record_learned_artifact(&learned_row("r1", 1, &v1_digest))
        .unwrap();

    let startup = crate::overlay_startup::load(&lyra_dir, &data_dir, &store, &artifacts).unwrap();
    let live: Vec<&Route> = startup
        .registry
        .routes
        .iter()
        .filter(|r| r.id == "r1")
        .collect();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].version, 2);
    assert!(overlay_file(&overlay_dir, "route", "r1", 2).exists());
    assert!(overlay_file(&overlay_dir, "route", "r1", 1).exists());
}

#[test]
fn missing_highest_blob_fails_closed_no_rollback() {
    let (store, artifacts, data_dir, overlay_dir) = fixture();
    let lyra_dir = tempdir().unwrap().keep();
    std::fs::create_dir_all(overlay_dir.join("routes")).unwrap();

    let v2 = overlay_yaml("r1", 2);
    let v2_digest = digest_of_bytes(v2.as_bytes()).to_string();
    insert_active_proposal(&store, "r1", 2, &v2_digest);
    store
        .record_learned_artifact(&learned_row("r1", 2, &v2_digest))
        .unwrap();

    let v1 = overlay_yaml("r1", 1);
    let v1_digest = digest_of_bytes(v1.as_bytes()).to_string();
    std::fs::write(overlay_file(&overlay_dir, "route", "r1", 1), &v1).unwrap();
    store
        .record_learned_artifact(&learned_row("r1", 1, &v1_digest))
        .unwrap();

    let startup = crate::overlay_startup::load(&lyra_dir, &data_dir, &store, &artifacts).unwrap();
    assert!(startup
        .registry
        .routes
        .iter()
        .find(|r| r.id == "r1")
        .is_none());
    assert!(!overlay_file(&overlay_dir, "route", "r1", 2).exists());
    assert!(overlay_file(&overlay_dir, "route", "r1", 1).exists());
}

#[test]
fn on_disk_artifact_without_active_proposal_is_excluded() {
    let (store, _artifacts, data_dir, overlay_dir) = fixture();
    let lyra_dir = tempdir().unwrap().keep();
    std::fs::create_dir_all(overlay_dir.join("routes")).unwrap();

    let yaml = overlay_yaml("r1", 1);
    let digest = digest_of_bytes(yaml.as_bytes()).to_string();
    std::fs::write(overlay_file(&overlay_dir, "route", "r1", 1), &yaml).unwrap();
    store
        .insert_proposed_artifact(&ProposedArtifact {
            id: Ulid::new(),
            kind: "route".to_string(),
            artifact_id: "r1".to_string(),
            version: 1,
            state: Lifecycle::Proposed,
            yaml_digest: digest.to_string(),
            task_grant_id: Ulid::new(),
            action_request_id: None,
            proposed_at: Timestamp::now(),
            lineage: None,
        })
        .unwrap();
    store
        .record_learned_artifact(&learned_row("r1", 1, &digest))
        .unwrap();

    let startup = crate::overlay_startup::load(&lyra_dir, &data_dir, &store, &_artifacts).unwrap();
    assert!(startup
        .registry
        .routes
        .iter()
        .find(|r| r.id == "r1")
        .is_none());
}

fn overlay_template_yaml(id: &str, version: u32) -> String {
    let template = crate::model_gateway::PromptTemplate {
        id: id.to_string(),
        schema_version: 1,
        version,
        lifecycle_state: Lifecycle::Active,
        system_preamble: "You are a helpful assistant.".to_string(),
        untrusted_data_preamble: None,
    };
    serde_yaml::to_string(&template).unwrap()
}

fn learned_template_row(id: &str, version: u32, digest: &str) -> LearnedArtifact {
    let mut row = learned_row(id, version, digest);
    row.kind = "template".to_string();
    row.compatibility = CompatibilityStatus::OwnerAccepted;
    row.accepted_dependency_fingerprint = Some(dependency_fingerprint(&[]));
    row
}

#[test]
fn reconfirmed_legacy_template_stays_live_across_restart() {
    let (store, _artifacts, data_dir, overlay_dir) = fixture();
    let lyra_dir = tempdir().unwrap().keep();
    std::fs::create_dir_all(overlay_dir.join("templates")).unwrap();

    let yaml = overlay_template_yaml("tpl1", 1);
    let digest = digest_of_bytes(yaml.as_bytes()).to_string();
    let path = overlay_file(&overlay_dir, "template", "tpl1", 1);
    std::fs::write(&path, &yaml).unwrap();

    let mut row = learned_template_row("tpl1", 1, &digest);
    row.source_path = Some(path.to_string_lossy().into_owned());
    store.record_learned_artifact(&row).unwrap();
    let proposal_id = Ulid::new();
    store
        .insert_proposed_artifact(&ProposedArtifact {
            id: proposal_id,
            kind: "template".to_string(),
            artifact_id: "tpl1".to_string(),
            version: 1,
            state: Lifecycle::Proposed,
            yaml_digest: digest.to_string(),
            task_grant_id: Ulid::new(),
            action_request_id: None,
            proposed_at: Timestamp::now(),
            lineage: None,
        })
        .unwrap();
    store
        .force_proposed_artifact_state_for_test(proposal_id, Lifecycle::Active)
        .unwrap();

    let startup = crate::overlay_startup::load(&lyra_dir, &data_dir, &store, &_artifacts).unwrap();
    assert_eq!(
        startup.registry.templates.get("tpl1").map(|t| t.version),
        Some(1)
    );
    assert!(startup.pending_reconfirm_buttons.is_empty());
    assert!(startup.pending_reconfirm_notices.is_empty());
}

#[test]
fn orphan_active_yaml_without_db_row_is_quarantined() {
    let (store, _artifacts, data_dir, overlay_dir) = fixture();
    let lyra_dir = tempdir().unwrap().keep();
    std::fs::create_dir_all(overlay_dir.join("routes")).unwrap();

    let yaml = overlay_yaml("r1", 1);
    std::fs::write(overlay_file(&overlay_dir, "route", "r1", 1), &yaml).unwrap();

    let startup = crate::overlay_startup::load(&lyra_dir, &data_dir, &store, &_artifacts).unwrap();
    assert!(startup
        .registry
        .routes
        .iter()
        .find(|r| r.id == "r1")
        .is_none());
    assert!(!startup.pending_reconfirm_buttons.is_empty());
    let reconfirmed = store.list_learned_artifacts().unwrap();
    assert!(reconfirmed
        .iter()
        .any(|a| a.compatibility == CompatibilityStatus::ReconfirmationRequired));
}

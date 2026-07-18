//! Focused crash-recovery tests for the overlay activation pipeline (AD-070).
//!
//! Mirrors the model-swap recovery windows against `overlay_startup::load`
//! (the real startup entry) and the `republish_missing_committed` helper:
use crate::store::learned_artifacts::{
    CompatibilityStatus, LearnedArtifact, NominationStatus, Provenance,
};

use crate::artifact_loader::{ArtifactRegistry, ArtifactSource};
use crate::artifact_store::ArtifactStore;
use crate::store::proposed_artifacts::ProposedArtifact;
use crate::store::Store;
use jiff::Timestamp;
use openspine_schemas::artifact::{ArtifactNamespace, ArtifactRef, Lifecycle};
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::route::{Route, RouteEffect};
use tempfile::tempdir;
use ulid::Ulid;

const TEST_KEY: [u8; 32] = [3u8; 32];

/// `(store, artifacts, data_dir, overlay_dir)` where `overlay_dir ==
/// data_dir.join("artifacts.d")` — the layout `overlay_startup::load` expects.
pub(super) fn fixture() -> (Store, ArtifactStore, std::path::PathBuf, std::path::PathBuf) {
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(tempdir().unwrap().keep(), TEST_KEY).unwrap();
    let data_dir = tempdir().unwrap().keep();
    let overlay_dir = data_dir.join("artifacts.d");
    (store, artifacts, data_dir, overlay_dir)
}

pub(super) fn overlay_file(
    overlay_dir: &std::path::Path,
    kind: &str,
    id: &str,
    version: u32,
) -> std::path::PathBuf {
    let subdir =
        crate::artifact_loader::overlay_subdir_for_kind(kind).expect("test kind is proposable");
    overlay_dir
        .join(subdir)
        .join(crate::artifact_loader::overlay_filename(id, version))
}

pub(super) fn overlay_yaml(id: &str, version: u32) -> String {
    let route = Route {
        id: id.to_string(),
        schema_version: 1,
        version,
        lifecycle_state: Lifecycle::Active,
        priority: None,
        effect: RouteEffect::Allow,
        when: Default::default(),
        agent: None,
        workflow: None,
        capability_pack: None,
    };
    serde_yaml::to_string(&route).unwrap()
}

pub(super) fn learned_row(id: &str, version: u32, digest: &str) -> LearnedArtifact {
    LearnedArtifact {
        kind: "route".to_string(),
        artifact_id: id.to_string(),
        version,
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
        pending_yaml_digest: Some(digest.to_string()),
        accepted_dependency_fingerprint: None,
        source_path: None,
        accepted_base_epoch: None,
    }
}

fn insert_proposal(store: &Store, id: &str, version: u32, digest: &str, state: Lifecycle) {
    let proposal_id = Ulid::new();
    store
        .insert_proposed_artifact(&ProposedArtifact {
            id: proposal_id,
            kind: "route".to_string(),
            artifact_id: id.to_string(),
            version,
            state: Lifecycle::Proposed,
            yaml_digest: digest.to_string(),
            task_grant_id: Ulid::new(),
            action_request_id: None,
            proposed_at: Timestamp::now(),
            lineage: None,
        })
        .unwrap();
    if state != Lifecycle::Proposed {
        store
            .force_proposed_artifact_state_for_test(proposal_id, state)
            .unwrap();
    }
}

pub(super) fn insert_active_proposal(store: &Store, id: &str, version: u32, digest: &str) {
    insert_proposal(store, id, version, digest, Lifecycle::Active);
}

pub(super) fn insert_approved_proposal(store: &Store, id: &str, version: u32, digest: &str) {
    insert_proposal(store, id, version, digest, Lifecycle::Approved);
}

#[test]
fn stage_before_commit_crash_discards_temp_and_keeps_old() {
    let (store, _artifacts, data_dir, overlay_dir) = fixture();
    let lyra_dir = tempdir().unwrap().keep();
    std::fs::create_dir_all(overlay_dir.join("routes")).unwrap();

    // Old v1 previously committed Active and published on disk.
    let old_yaml = overlay_yaml("r1", 1);
    let old_digest = digest_of_bytes(old_yaml.as_bytes()).to_string();
    std::fs::write(overlay_file(&overlay_dir, "route", "r1", 1), &old_yaml).unwrap();
    insert_active_proposal(&store, "r1", 1, &old_digest);
    store
        .record_learned_artifact(&learned_row("r1", 1, &old_digest))
        .unwrap();

    // Crash leaves a staged-but-uncommitted temp for a newer v2 (the exact
    // `*.tmp.<ULID>` form the activation pipeline writes before the commit).
    let staged_yaml = overlay_yaml("r1", 2);
    let tmp_path =
        overlay_file(&overlay_dir, "route", "r1", 2).with_extension(format!("tmp.{}", Ulid::new()));
    std::fs::write(&tmp_path, &staged_yaml).unwrap();

    let startup = crate::overlay_startup::load(&lyra_dir, &data_dir, &store, &_artifacts).unwrap();

    assert!(
        !tmp_path.exists(),
        "staged temp must be discarded at startup"
    );
    assert!(
        overlay_file(&overlay_dir, "route", "r1", 1).exists(),
        "old effective overlay must remain"
    );
    // Assert on the startup registry itself (not a raw reload from disk), so
    // startup exclusions/quarantine are exercised.
    let live: Vec<&Route> = startup
        .registry
        .routes
        .iter()
        .filter(|r| r.id == "r1")
        .collect();
    assert_eq!(live.len(), 1, "exactly one live route expected");
    assert_eq!(live[0].version, 1, "old effective version must remain");
}

#[test]
fn commit_before_publish_crash_republishes_exact_active_ref() {
    let (store, artifacts, data_dir, overlay_dir) = fixture();
    let lyra_dir = tempdir().unwrap().keep();
    std::fs::create_dir_all(overlay_dir.join("routes")).unwrap();

    // Committed Active, but the publish/rename crashed before the file landed.
    let yaml = overlay_yaml("r1", 1);
    let digest = digest_of_bytes(yaml.as_bytes()).to_string();
    let active_ref = artifacts.put(yaml.as_bytes()).unwrap();
    insert_active_proposal(&store, "r1", 1, &digest);
    store
        .record_learned_artifact(&learned_row("r1", 1, &digest))
        .unwrap();

    crate::overlay_startup::load(&lyra_dir, &data_dir, &store, &artifacts).unwrap();

    let path = overlay_file(&overlay_dir, "route", "r1", 1);
    assert!(
        path.exists(),
        "missing overlay must be republished after commit-before-publish"
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        digest_of_bytes(&bytes),
        active_ref.digest,
        "republished bytes must equal the committed active ref exactly"
    );
    let mut registry = ArtifactRegistry::default();
    crate::artifact_loader::load_registry_into(&mut registry, &overlay_dir).unwrap();
    assert_eq!(registry.routes.len(), 1);
    assert_eq!(registry.routes[0].version, 1);
}

#[test]
fn approved_dangling_recovery_quarantines_and_requests_reconfirm() {
    let (store, artifacts, data_dir, overlay_dir) = fixture();
    let lyra_dir = tempdir().unwrap().keep();
    std::fs::create_dir_all(overlay_dir.join("routes")).unwrap();

    // Proposal is Approved (dangling) but never reached Active; the file is
    // already on disk from an earlier publish.
    let yaml = overlay_yaml("r1", 1);
    let digest = digest_of_bytes(yaml.as_bytes()).to_string();
    let _active_ref = artifacts.put(yaml.as_bytes()).unwrap();
    insert_approved_proposal(&store, "r1", 1, &digest);
    std::fs::write(overlay_file(&overlay_dir, "route", "r1", 1), &yaml).unwrap();
    store
        .record_learned_artifact(&learned_row("r1", 1, &digest))
        .unwrap();

    let startup = crate::overlay_startup::load(&lyra_dir, &data_dir, &store, &artifacts).unwrap();

    assert!(
        startup
            .registry
            .routes
            .iter()
            .find(|r| r.id == "r1")
            .is_none(),
        "approved-dangling artifact must not hold live authority"
    );
    assert!(
        !startup.pending_reconfirm_buttons.is_empty(),
        "owner re-confirmation must be requested for the dangling approval"
    );
    assert!(
        overlay_file(&overlay_dir, "route", "r1", 1).exists(),
        "already-published file must be preserved"
    );
    let reconfirmed = store.list_learned_artifacts().unwrap();
    assert!(
        reconfirmed
            .iter()
            .any(|a| a.compatibility == CompatibilityStatus::ReconfirmationRequired),
        "dangling approval must be marked reconfirmation_required"
    );
}

#[test]
fn persona_with_dangling_source_event_is_excluded_even_with_exchange_blob() {
    let (store, artifacts, data_dir, overlay_dir) = fixture();
    let lyra_dir = tempdir().unwrap().keep();
    let persona = openspine_schemas::persona::PersonaElement {
        id: "dangling-persona".into(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        guidance: "positive guidance".into(),
    };
    let yaml = serde_yaml::to_string(&persona).unwrap();
    let yaml_digest = digest_of_bytes(yaml.as_bytes()).to_string();
    let exchange_ref = artifacts.put(b"valid exchange blob").unwrap();
    std::fs::create_dir_all(overlay_dir.join("personas")).unwrap();
    std::fs::write(
        overlay_dir
            .join("personas")
            .join(crate::artifact_loader::overlay_filename(
                &persona.id,
                persona.version,
            )),
        yaml,
    )
    .unwrap();
    store
        .record_learned_artifact(&LearnedArtifact {
            kind: "persona".into(),
            artifact_id: persona.id.clone(),
            version: persona.version,
            namespace: ArtifactNamespace::Overlay,
            provenance: Provenance::ProducedBy {
                source_event_id: Ulid::new(),
                source_exchange: exchange_ref,
            },
            accepted_via: None,
            learned_at: Timestamp::now(),
            compatibility: CompatibilityStatus::Compatible,
            nomination: NominationStatus::None,
            pending_reconfirmation_id: None,
            pending_yaml_digest: Some(yaml_digest),
            accepted_dependency_fingerprint: None,
            source_path: None,
            accepted_base_epoch: None,
        })
        .unwrap();

    let startup = crate::overlay_startup::load(&lyra_dir, &data_dir, &store, &artifacts).unwrap();

    assert!(
        !startup.registry.personas.contains_key(&persona.id),
        "dangling source event must quarantine persona despite valid exchange bytes"
    );
}

#[test]
fn active_v2_survives_stale_approved_v1_recovery() {
    let (store, artifacts, data_dir, overlay_dir) = fixture();
    let lyra_dir = tempdir().unwrap().keep();
    std::fs::create_dir_all(overlay_dir.join("routes")).unwrap();

    // Active v2 committed and published.
    let v2 = overlay_yaml("r1", 2);
    let v2_digest = digest_of_bytes(v2.as_bytes()).to_string();
    insert_active_proposal(&store, "r1", 2, &v2_digest);
    std::fs::write(overlay_file(&overlay_dir, "route", "r1", 2), &v2).unwrap();
    store
        .record_learned_artifact(&learned_row("r1", 2, &v2_digest))
        .unwrap();

    // Stale approved-dangling v1, still in the store but never published.
    let v1 = overlay_yaml("r1", 1);
    let v1_digest = digest_of_bytes(v1.as_bytes()).to_string();
    let _v1_ref = artifacts.put(v1.as_bytes()).unwrap();
    insert_approved_proposal(&store, "r1", 1, &v1_digest);
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
    assert_eq!(live.len(), 1, "exactly one live r1 expected");
    assert_eq!(
        live[0].version, 2,
        "active v2 must not be excluded by stale v1 recovery"
    );
    assert!(
        overlay_file(&overlay_dir, "route", "r1", 2).exists(),
        "active v2 file must remain"
    );
    assert!(
        !overlay_file(&overlay_dir, "route", "r1", 1).exists(),
        "stale v1 must not be published (no live authority)"
    );
}

fn mixed_active_approved_recovery(v1_file_present: bool) {
    let (store, artifacts, data_dir, overlay_dir) = fixture();
    let lyra_dir = tempdir().unwrap().keep();
    std::fs::create_dir_all(overlay_dir.join("routes")).unwrap();
    let v1 = overlay_yaml("r1", 1);
    let v2 = overlay_yaml("r1", 2);
    let d1 = digest_of_bytes(v1.as_bytes()).to_string();
    let d2 = digest_of_bytes(v2.as_bytes()).to_string();
    artifacts.put(v1.as_bytes()).unwrap();
    artifacts.put(v2.as_bytes()).unwrap();
    insert_active_proposal(&store, "r1", 1, &d1);
    insert_approved_proposal(&store, "r1", 2, &d2);
    store
        .record_learned_artifact(&learned_row("r1", 1, &d1))
        .unwrap();
    store
        .record_learned_artifact(&learned_row("r1", 2, &d2))
        .unwrap();
    if v1_file_present {
        std::fs::write(overlay_file(&overlay_dir, "route", "r1", 1), &v1).unwrap();
    }
    std::fs::write(overlay_file(&overlay_dir, "route", "r1", 2), &v2).unwrap();
    let startup = crate::overlay_startup::load(&lyra_dir, &data_dir, &store, &artifacts).unwrap();
    assert_eq!(
        startup
            .registry
            .routes
            .iter()
            .find(|r| r.id == "r1")
            .map(|r| r.version),
        Some(1)
    );
    assert!(!startup
        .registry
        .routes
        .iter()
        .any(|r| r.id == "r1" && r.version == 2));
    assert_eq!(startup.pending_reconfirm_buttons.len(), 1);
    let learned = store.list_learned_artifacts().unwrap();
    let v1_row = learned.iter().find(|r| r.version == 1).unwrap();
    let v2_row = learned.iter().find(|r| r.version == 2).unwrap();
    assert_eq!(v1_row.compatibility, CompatibilityStatus::Compatible);
    assert_eq!(
        v2_row.compatibility,
        CompatibilityStatus::ReconfirmationRequired
    );
    assert_eq!(
        v2_row.pending_reconfirmation_id,
        Some(startup.pending_reconfirm_buttons[0].0)
    );
}

#[test]
fn mixed_active_v1_approved_v2_with_v1_file_keeps_v1_live() {
    mixed_active_approved_recovery(true);
}

#[test]
fn mixed_active_v1_approved_v2_missing_v1_file_republishes_v1() {
    mixed_active_approved_recovery(false);
}

#[test]
fn prune_non_highest_active_preserves_persona_typed_and_source_entries() {
    let store = Store::open_in_memory().unwrap();
    let mut registry = ArtifactRegistry::default();
    let persona = openspine_schemas::persona::PersonaElement {
        id: "seed".to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        guidance: "positive guidance".to_string(),
    };
    let bytes = serde_yaml::to_string(&persona).unwrap().into_bytes();
    let digest = digest_of_bytes(&bytes);
    registry.personas.insert(persona.id.clone(), persona);
    registry.sources.insert(
        ("persona".to_string(), "seed".to_string(), 1),
        ArtifactSource {
            path: std::path::PathBuf::from("/tmp/persona-seed.yaml"),
            bytes,
        },
    );
    store
        .record_learned_artifact(&LearnedArtifact {
            kind: "persona".into(),
            artifact_id: "seed".into(),
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
            pending_yaml_digest: Some(digest.to_string()),
            accepted_dependency_fingerprint: None,
            source_path: None,
            accepted_base_epoch: None,
        })
        .unwrap();

    let excluded =
        crate::overlay_recovery::prune_non_highest_active(&mut registry, &store).unwrap();
    assert!(excluded.is_empty());
    assert!(registry.personas.contains_key("seed"));
    assert!(registry
        .sources
        .contains_key(&("persona".to_string(), "seed".to_string(), 1)));
}

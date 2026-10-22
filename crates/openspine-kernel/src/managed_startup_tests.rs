//! A committed selection is loaded from its exact retained bytes before any
//! effect, and a damaged one makes startup unavailable instead of falling back.
use super::startup_base;
use crate::artifact_loader;
use crate::artifact_store::ArtifactStore;
use crate::overlay_startup::{load_with_base, BaseSource};
use crate::package::managed_source::tests::{variant, Fixture, MARKER};
use crate::store::learned_artifacts::CompatibilityStatus;
use crate::store::package_selection_fixtures::learned;
use crate::store::proposed_artifacts::ProposedArtifact;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::route::{Route, RouteEffect};
use std::fs;
use std::path::Path;

fn lyra_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra")
}

fn artifacts(fixture: &Fixture) -> ArtifactStore {
    ArtifactStore::open(fixture.data.join("artifacts"), [3u8; 32]).unwrap()
}

#[test]
fn legacy_mode_uses_the_configured_directory_while_no_selection_exists() {
    let fixture = Fixture::new();
    let path = lyra_dir();
    assert!(matches!(
        startup_base(&fixture.store, &fixture.data, &path).unwrap(),
        BaseSource::Legacy(used) if used == path
    ));
}

#[test]
fn a_selected_base_loads_without_reading_any_legacy_directory() {
    let fixture = Fixture::new();
    let receipt = fixture.install(&variant(fixture.root.path(), 3));
    fixture.select(&receipt);
    // The obsolete legacy setting points nowhere and must not matter.
    let gone = fixture.root.path().join("vanished-lyra");
    let base = startup_base(&fixture.store, &fixture.data, &gone).unwrap();
    assert!(matches!(base, BaseSource::Managed(_)));
    let startup =
        load_with_base(base, &fixture.data, &fixture.store, &artifacts(&fixture)).unwrap();
    let template = &startup.registry.templates["owner_terminal_template"];
    assert!(template.system_preamble.starts_with(MARKER));
    let selected = crate::package::managed_source::resolve(&fixture.store, &fixture.data)
        .unwrap()
        .unwrap();
    let ids = artifact_loader::artifact_identity_pairs(selected.snapshot.registry());
    assert_eq!(
        startup.base_compatibility_epoch,
        crate::overlay_compat::compatibility_epoch(selected.snapshot.registry(), &ids)
    );
    // The selected revision's epoch differs from the legacy base's.
    let legacy = artifact_loader::load_base_registry(&lyra_dir()).unwrap();
    let legacy_ids = artifact_loader::artifact_identity_pairs(&legacy);
    assert_ne!(
        startup.base_compatibility_epoch,
        crate::overlay_compat::compatibility_epoch(&legacy, &legacy_ids)
    );
}

#[test]
fn a_damaged_selection_fails_before_any_effect_and_never_uses_the_legacy_base() {
    let fixture = Fixture::new();
    let receipt = fixture.install(&variant(fixture.root.path(), 3));
    fixture.select(&receipt);
    let audit_before = fixture
        .store
        .count_audit_events_of_kind("artifact.reconfirmation_required")
        .unwrap();
    let rows_before = fixture.store.list_learned_artifacts().unwrap().len();
    fs::write(
        fixture.object_dir(&receipt).join("package.yaml"),
        b"id: lyra\n",
    )
    .unwrap();
    // A perfectly valid legacy directory is configured and must not rescue it.
    let error = match startup_base(&fixture.store, &fixture.data, &lyra_dir()) {
        Err(error) => format!("{error:#}"),
        Ok(_) => panic!("a corrupt selection must not start"),
    };
    assert!(error.contains("missing or corrupt"), "{error}");
    assert_eq!(
        fixture
            .store
            .count_audit_events_of_kind("artifact.reconfirmation_required")
            .unwrap(),
        audit_before
    );
    assert_eq!(
        fixture.store.list_learned_artifacts().unwrap().len(),
        rows_before
    );
}

/// A committed, published overlay route with an owner-accepted provenance row.
fn accepted(fixture: &Fixture, id: &str, epoch: &str) {
    let route = Route {
        id: id.to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        priority: None,
        effect: RouteEffect::Allow,
        when: Default::default(),
        agent: None,
        workflow: None,
        capability_pack: None,
        persona: None,
    };
    let yaml = serde_yaml::to_string(&route).unwrap();
    let digest = digest_of_bytes(yaml.as_bytes()).to_string();
    let path = fixture
        .data
        .join("artifacts.d")
        .join(artifact_loader::overlay_subdir_for_kind("route").unwrap())
        .join(artifact_loader::overlay_filename(id, 1));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, &yaml).unwrap();
    let proposal_id = ulid::Ulid::new();
    fixture
        .store
        .insert_proposed_artifact(&ProposedArtifact {
            id: proposal_id,
            kind: "route".into(),
            artifact_id: id.into(),
            version: 1,
            state: Lifecycle::Proposed,
            yaml_digest: digest.clone(),
            task_grant_id: ulid::Ulid::new(),
            action_request_id: None,
            proposed_at: jiff::Timestamp::now(),
            lineage: None,
        })
        .unwrap();
    fixture
        .store
        .force_proposed_artifact_state_for_test(proposal_id, Lifecycle::Active)
        .unwrap();
    let mut row = learned("route", id, CompatibilityStatus::OwnerAccepted);
    row.source_path = Some(path.to_string_lossy().into_owned());
    row.pending_yaml_digest = Some(digest);
    row.accepted_base_epoch = Some(epoch.into());
    fixture.store.record_learned_artifact(&row).unwrap();
}

fn state(fixture: &Fixture, id: &str) -> (CompatibilityStatus, Option<String>) {
    let row = fixture
        .store
        .list_learned_artifacts()
        .unwrap()
        .into_iter()
        .find(|row| row.artifact_id == id)
        .unwrap();
    (row.compatibility, row.accepted_base_epoch)
}

#[test]
fn managed_startup_never_refreshes_an_acceptance_epoch_past_the_base_it_binds() {
    let fixture = Fixture::new();
    let receipt = fixture.install(&variant(fixture.root.path(), 3));
    fixture.select(&receipt);
    let selected = crate::package::managed_source::resolve(&fixture.store, &fixture.data)
        .unwrap()
        .unwrap();
    let registry = selected.snapshot.registry();
    let epoch = crate::overlay_compat::compatibility_epoch(
        registry,
        &artifact_loader::artifact_identity_pairs(registry),
    );
    accepted(&fixture, "bound-elsewhere", "an-older-base-epoch");
    accepted(&fixture, "bound-here", &epoch);
    let configured = lyra_dir();
    let base = startup_base(&fixture.store, &fixture.data, &configured).unwrap();
    let startup =
        load_with_base(base, &fixture.data, &fixture.store, &artifacts(&fixture)).unwrap();
    // The mismatched acceptance is not silently re-blessed: it returns to the
    // ordinary reconfirmation lifecycle, keeps its recorded epoch, and is
    // excluded from the effective registry until the owner reconfirms.
    assert_eq!(
        state(&fixture, "bound-elsewhere"),
        (
            CompatibilityStatus::ReconfirmationRequired,
            Some("an-older-base-epoch".into())
        )
    );
    assert!(startup
        .registry
        .routes
        .iter()
        .all(|route| route.id != "bound-elsewhere"));
    assert_eq!(
        startup.pending_reconfirm_buttons.len(),
        1,
        "{:?} {:?}",
        startup.pending_reconfirm_notices,
        startup
            .registry
            .routes
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>()
    );
    // An acceptance that already binds this base stays accepted and effective.
    assert_eq!(
        state(&fixture, "bound-here"),
        (CompatibilityStatus::OwnerAccepted, Some(epoch))
    );
    assert!(startup
        .registry
        .routes
        .iter()
        .any(|route| route.id == "bound-here"));
}

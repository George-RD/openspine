//! Version-phase coverage only: no test here claims full package readiness.
use super::*;

fn captured_versions() -> (TempDir, Store, CapturedCurrentState) {
    let (root, data_root, store, artifacts) = fixture();
    let base = configured_base(root.path());
    let directory = data_root.join("artifacts.d/routes");
    fs::create_dir_all(&directory).unwrap();
    for version in [1, 2] {
        let yaml = route_yaml("version-review", version);
        let digest = digest_of_bytes(&yaml).to_string();
        fs::write(
            directory.join(crate::artifact_loader::overlay_filename(
                "version-review",
                version,
            )),
            &yaml,
        )
        .unwrap();
        artifacts.put(&yaml).unwrap();
        store
            .record_learned_artifact(&learned_route("version-review", version, &digest))
            .unwrap();
        if version == 1 {
            active_route(&store, "version-review", version, &digest);
        }
    }
    let captured =
        CapturedCurrentState::capture(&base, "lyra", &data_root, &store, &artifacts).unwrap();
    assert_eq!(captured.overlay.registry.sources.len(), 2);
    (root, store, captured)
}

fn key(version: u32) -> ArtifactVersion {
    ("route".into(), "version-review".into(), version)
}

#[test]
fn captured_version_review_ignores_later_activation_and_source_removal() {
    let (root, store, captured) = captured_versions();
    let digest = digest_of_bytes(&route_yaml("version-review", 2)).to_string();
    active_route(&store, "version-review", 2, &digest);
    assert_eq!(
        store
            .highest_active_version("route", "version-review")
            .unwrap(),
        Some(2)
    );
    drop(store);
    root.close().unwrap();

    let admitted = captured.overlay.evaluate_versions().unwrap();
    assert_eq!(
        artifact_loader::artifact_version(&admitted.registry, "route", "version-review"),
        Some(1)
    );
    assert_eq!(admitted.excluded, BTreeSet::from([key(2)]));
    assert_eq!(
        admitted.registry.sources[&key(1)].bytes,
        route_yaml("version-review", 1)
    );
}

#[test]
fn captured_version_review_is_repeatable_and_preserves_inputs() {
    let (_root, store, captured) = captured_versions();
    let controls = captured.overlay.controls.clone();
    let sources: BTreeSet<_> = captured.overlay.registry.sources.keys().cloned().collect();
    let first = captured.overlay.evaluate_versions().unwrap();
    let second = captured.overlay.evaluate_versions().unwrap();
    assert_eq!(first.excluded, second.excluded);
    assert_eq!(
        first.registry.sources[&key(1)].bytes,
        second.registry.sources[&key(1)].bytes
    );
    assert_eq!(captured.overlay.controls, controls);
    assert_eq!(
        captured
            .overlay
            .registry
            .sources
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>(),
        sources
    );
    assert_eq!(
        store
            .highest_active_version("route", "version-review")
            .unwrap(),
        Some(1)
    );
}

#[test]
fn captured_version_review_preserves_explicit_absence_of_active_version() {
    let (_root, _store, mut captured) = captured_versions();
    for control in captured.overlay.controls.values_mut() {
        control.highest_active_version = None;
    }
    let admitted = captured.overlay.evaluate_versions().unwrap();
    assert!(admitted.registry.routes.is_empty());
    assert_eq!(admitted.excluded, BTreeSet::from([key(1), key(2)]));
}

#[test]
fn captured_version_review_does_not_recover_a_missing_highest_source() {
    let (_root, _store, mut captured) = captured_versions();
    for control in captured.overlay.controls.values_mut() {
        control.highest_active_version = Some(3);
    }
    captured.overlay.controls.insert(
        key(3),
        CapturedOverlayControl {
            lifecycle: Some(Lifecycle::Active),
            highest_active_version: Some(3),
            source_present: false,
            recoverable_blob_present: true,
        },
    );
    let admitted = captured.overlay.evaluate_versions().unwrap();
    assert!(admitted.registry.routes.is_empty());
    assert_eq!(admitted.excluded, BTreeSet::from([key(1), key(2)]));
    // The separate missing-source blocker remains visible to the full reviewer.
    assert!(!captured.overlay.controls[&key(3)].source_present);
    assert!(captured.overlay.controls[&key(3)].recoverable_blob_present);
}

#[test]
fn captured_version_review_rejects_missing_exact_version_control() {
    let (_root, _store, mut captured) = captured_versions();
    captured.overlay.controls.remove(&key(2));
    assert!(captured.overlay.evaluate_versions().is_err());
}

#[test]
fn captured_version_review_rejects_conflicting_highest_version_controls() {
    let (_root, _store, mut captured) = captured_versions();
    captured
        .overlay
        .controls
        .get_mut(&key(2))
        .unwrap()
        .highest_active_version = Some(2);
    assert!(captured.overlay.evaluate_versions().is_err());
}

#[test]
fn captured_version_review_rejects_source_presence_drift() {
    let (_root, _store, mut captured) = captured_versions();
    captured
        .overlay
        .controls
        .get_mut(&key(1))
        .unwrap()
        .source_present = false;
    assert!(captured.overlay.evaluate_versions().is_err());
}

#[test]
fn captured_version_review_rejects_a_falsely_present_missing_source() {
    let (_root, _store, mut captured) = captured_versions();
    let control = captured.overlay.controls[&key(1)].clone();
    captured.overlay.controls.insert(key(3), control);
    assert!(captured.overlay.evaluate_versions().is_err());
}

#[test]
fn captured_version_constructor_rejects_absent_control_instead_of_assuming_inactive() {
    let (_root, _store, captured) = captured_versions();
    assert!(CapturedVersionAdmission::from_captured(
        captured.overlay.registry,
        captured.overlay.learned,
        BTreeMap::new(),
    )
    .is_err());
}

#[test]
fn captured_version_constructor_does_not_apply_proposal_controls_to_personas() {
    let highest = BTreeMap::from([(("persona".into(), "test-persona".into()), Some(1))]);
    assert!(CapturedVersionAdmission::from_captured(
        ArtifactRegistry::default(),
        Vec::new(),
        highest,
    )
    .is_err());
}

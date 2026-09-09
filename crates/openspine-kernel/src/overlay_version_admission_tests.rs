//! Captured version-admission contracts, distinct from full overlay admission.
use std::collections::BTreeSet;

use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::persona::PersonaElement;
use tempfile::{tempdir, TempDir};

use super::{AdmittedOverlay, CapturedVersionAdmission};
use crate::artifact_loader::{self, ArtifactRegistry};
use crate::overlay_recovery::tests::{
    insert_active_proposal, insert_approved_proposal, learned_row, overlay_yaml,
};
use crate::store::Store;

/// Load real route files, retaining all source versions through the real loader.
fn routes(versions: &[(&str, u32)]) -> (TempDir, ArtifactRegistry) {
    let directory = tempdir().unwrap();
    std::fs::create_dir(directory.path().join("routes")).unwrap();
    for (id, version) in versions {
        let path = directory
            .path()
            .join("routes")
            .join(artifact_loader::overlay_filename(id, *version));
        std::fs::write(path, overlay_yaml(id, *version)).unwrap();
    }
    let mut registry = ArtifactRegistry::default();
    artifact_loader::load_registry_into(&mut registry, directory.path()).unwrap();
    (directory, registry)
}

/// Load real persona sources without manufacturing a proposal lifecycle.
fn personas(versions: &[u32]) -> (TempDir, ArtifactRegistry) {
    let directory = tempdir().unwrap();
    std::fs::create_dir(directory.path().join("personas")).unwrap();
    for version in versions {
        let persona = PersonaElement {
            id: "style".into(),
            schema_version: 1,
            version: *version,
            lifecycle_state: Lifecycle::Active,
            guidance: format!("Guidance version {version}"),
        };
        let path = directory
            .path()
            .join("personas")
            .join(artifact_loader::overlay_filename("style", *version));
        std::fs::write(path, serde_yaml::to_string(&persona).unwrap()).unwrap();
    }
    let mut registry = ArtifactRegistry::default();
    artifact_loader::load_registry_into(&mut registry, directory.path()).unwrap();
    (directory, registry)
}

/// Persist the active proposal control used by normal startup pruning.
fn activate(store: &Store, id: &str, version: u32) {
    let digest = digest_of_bytes(overlay_yaml(id, version).as_bytes());
    insert_active_proposal(store, id, version, digest.as_str());
}

/// Record the exact persona source digest with its original producing origin.
fn back_persona(store: &Store, registry: &ArtifactRegistry, version: u32) {
    let source = &registry.sources[&("persona".into(), "style".into(), version)];
    let mut row = learned_row("style", version, digest_of_bytes(&source.bytes).as_str());
    row.kind = "persona".into();
    store.record_learned_artifact(&row).unwrap();
}

/// Read the real typed registry result, not merely the retained source map.
fn route_version(result: &AdmittedOverlay, id: &str) -> Option<u32> {
    artifact_loader::artifact_version(&result.registry, "route", id)
}

/// A later activation cannot change a captured highest-active choice.
#[test]
fn capture_does_not_adopt_a_later_active_version() {
    let store = Store::open_in_memory().unwrap();
    let (_directory, registry) = routes(&[("route", 1), ("route", 2)]);
    activate(&store, "route", 1);
    let captured = CapturedVersionAdmission::capture(&registry, &store).unwrap();
    activate(&store, "route", 2);

    let admitted = captured.evaluate().unwrap();
    assert_eq!(route_version(&admitted, "route"), Some(1));
    assert_eq!(
        admitted.excluded,
        BTreeSet::from([("route".into(), "route".into(), 2)])
    );
}

/// Explicit absence at capture cannot become authority through a later write.
#[test]
fn capture_preserves_absence_of_an_active_proposal() {
    let store = Store::open_in_memory().unwrap();
    let (_directory, registry) = routes(&[("route", 1)]);
    let captured = CapturedVersionAdmission::capture(&registry, &store).unwrap();
    activate(&store, "route", 1);

    let admitted = captured.evaluate().unwrap();
    assert_eq!(route_version(&admitted, "route"), None);
    assert!(admitted.registry.sources.is_empty());
}

/// Provenance written after capture cannot admit a previously unbacked persona.
#[test]
fn capture_does_not_adopt_later_persona_provenance() {
    let store = Store::open_in_memory().unwrap();
    let (_directory, registry) = personas(&[1]);
    let captured = CapturedVersionAdmission::capture(&registry, &store).unwrap();
    back_persona(&store, &registry, 1);

    let admitted = captured.evaluate().unwrap();
    assert!(admitted.registry.personas.is_empty());
    assert_eq!(
        admitted.excluded,
        BTreeSet::from([("persona".into(), "style".into(), 1)])
    );
}

/// Rehydrating the captured active version never reopens its original path.
#[test]
fn capture_keeps_bytes_after_live_sources_are_removed() {
    let store = Store::open_in_memory().unwrap();
    let (directory, registry) = routes(&[("route", 1), ("route", 2)]);
    activate(&store, "route", 1);
    let original = registry.sources[&("route".into(), "route".into(), 1)].clone();
    let captured = CapturedVersionAdmission::capture(&registry, &store).unwrap();
    drop(directory);
    assert!(!original.path.exists());

    let admitted = captured.evaluate().unwrap();
    assert_eq!(route_version(&admitted, "route"), Some(1));
    assert_eq!(
        admitted.registry.sources[&("route".into(), "route".into(), 1)],
        original
    );
}

/// Mutating the caller's registry cannot replace the captured typed/raw state.
#[test]
fn capture_cannot_be_changed_through_the_original_registry() {
    let store = Store::open_in_memory().unwrap();
    let (_directory, mut registry) = routes(&[("route", 1)]);
    activate(&store, "route", 1);
    let captured = CapturedVersionAdmission::capture(&registry, &store).unwrap();
    registry.routes.clear();
    registry.sources.clear();

    let admitted = captured.evaluate().unwrap();
    assert_eq!(route_version(&admitted, "route"), Some(1));
    assert_eq!(admitted.registry.sources.len(), 1);
}

/// Missing DB-highest bytes must not revive an older on-disk active version.
#[test]
fn missing_highest_active_never_revives_lower_bytes() {
    let store = Store::open_in_memory().unwrap();
    let (_directory, registry) = routes(&[("route", 1)]);
    activate(&store, "route", 1);
    activate(&store, "route", 2);

    let admitted = CapturedVersionAdmission::capture(&registry, &store)
        .unwrap()
        .evaluate()
        .unwrap();
    assert_eq!(route_version(&admitted, "route"), None);
    assert!(admitted.registry.sources.is_empty());
    assert_eq!(
        admitted.excluded,
        BTreeSet::from([("route".into(), "route".into(), 1)])
    );
}

/// An approved-but-not-active proposal still has no runtime admission.
#[test]
fn approved_proposal_is_not_active_authority() {
    let store = Store::open_in_memory().unwrap();
    let (_directory, registry) = routes(&[("route", 1)]);
    let digest = digest_of_bytes(overlay_yaml("route", 1).as_bytes());
    insert_approved_proposal(&store, "route", 1, digest.as_str());

    let admitted = CapturedVersionAdmission::capture(&registry, &store)
        .unwrap()
        .evaluate()
        .unwrap();
    assert_eq!(route_version(&admitted, "route"), None);
}

/// Exclusions identify exact dropped versions in canonical identity order.
#[test]
fn exclusions_are_canonical_and_version_exact() {
    let store = Store::open_in_memory().unwrap();
    let (_directory, registry) = routes(&[("z", 2), ("a", 2), ("z", 1), ("a", 1)]);
    activate(&store, "a", 2);

    let admitted = CapturedVersionAdmission::capture(&registry, &store)
        .unwrap()
        .evaluate()
        .unwrap();
    assert_eq!(route_version(&admitted, "a"), Some(2));
    assert_eq!(route_version(&admitted, "z"), None);
    assert_eq!(
        admitted.excluded.into_iter().collect::<Vec<_>>(),
        vec![
            ("route".into(), "a".into(), 1),
            ("route".into(), "z".into(), 1),
            ("route".into(), "z".into(), 2),
        ]
    );
}

/// An unbacked higher persona cannot hide a correctly backed lower version.
#[test]
fn persona_versions_require_matching_row_and_digest() {
    let store = Store::open_in_memory().unwrap();
    let (_directory, registry) = personas(&[1, 2, 3]);
    back_persona(&store, &registry, 1);
    let mut wrong_digest = learned_row("style", 2, digest_of_bytes(b"different").as_str());
    wrong_digest.kind = "persona".into();
    store.record_learned_artifact(&wrong_digest).unwrap();

    let admitted = CapturedVersionAdmission::capture(&registry, &store)
        .unwrap()
        .evaluate()
        .unwrap();
    assert_eq!(admitted.registry.personas["style"].version, 1);
    assert_eq!(
        admitted.excluded,
        BTreeSet::from([
            ("persona".into(), "style".into(), 2),
            ("persona".into(), "style".into(), 3),
        ])
    );
}

/// Evaluation preserves the persisted provenance, activation and control rows.
#[test]
fn evaluation_preserves_learned_rows_and_control_state() {
    let store = Store::open_in_memory().unwrap();
    let (_directory, registry) = personas(&[1, 2]);
    back_persona(&store, &registry, 1);
    store.set_kv("version-admission-control", "unchanged").unwrap();
    let learned = store.list_learned_artifacts().unwrap();
    let captured = CapturedVersionAdmission::capture(&registry, &store).unwrap();

    let admitted = captured.evaluate().unwrap();
    assert_eq!(admitted.registry.personas["style"].version, 1);
    assert_eq!(store.list_learned_artifacts().unwrap(), learned);
    assert_eq!(
        store.get_kv("version-admission-control").unwrap().as_deref(),
        Some("unchanged")
    );
    assert_eq!(store.highest_active_version("persona", "style").unwrap(), None);
}

/// Invalid retained rehydration bytes fail rather than falling back to a path.
#[test]
fn malformed_captured_highest_bytes_fail_without_path_fallback() {
    let store = Store::open_in_memory().unwrap();
    let (_directory, mut registry) = routes(&[("route", 1), ("route", 2)]);
    activate(&store, "route", 1);
    registry
        .sources
        .get_mut(&("route".into(), "route".into(), 1))
        .unwrap()
        .bytes = b"not a route".to_vec();

    assert!(CapturedVersionAdmission::capture(&registry, &store)
        .unwrap()
        .evaluate()
        .is_err());
}

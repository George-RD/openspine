use super::*;
use crate::artifact_loader::{self, ArtifactSource};
use crate::overlay_compat;
use crate::overlay_persona_admission::PersonaProvenanceFindings;
use crate::package::current_state::{
    CapturedCurrentBase, CapturedOverlayControl, CapturedOverlayState,
};
use crate::package::install_types::PackageIdentity;
use crate::store::learned_artifacts::{
    CompatibilityStatus, LearnedArtifact, NominationStatus, Provenance,
};
use openspine_schemas::artifact::{ArtifactNamespace, Lifecycle};
use openspine_schemas::digest::digest_of_bytes;
use std::collections::BTreeMap;

fn state() -> CapturedCurrentState {
    let registry = ArtifactRegistry::default();
    let ids = artifact_loader::artifact_identity_pairs(&registry);
    CapturedCurrentState {
        base_compatibility_epoch: overlay_compat::compatibility_epoch(&registry, &ids),
        base_artifact_ids: ids,
        base: CapturedCurrentBase {
            configured_path: "/not-a-live-input".into(),
            identity: PackageIdentity {
                package_id: "test".into(),
                revision: 1,
                inventory_format_version: 1,
                content_digest: digest_of_bytes(b"base"),
                manifest_digest: digest_of_bytes(b"manifest"),
            },
            registry,
        },
        overlay: CapturedOverlayState {
            registry: ArtifactRegistry::default(),
            learned: Vec::new(),
            controls: BTreeMap::new(),
            persona_findings: PersonaProvenanceFindings {
                expected_digests: BTreeMap::new(),
                excluded: BTreeMap::new(),
            },
        },
    }
}

fn insert(registry: &mut ArtifactRegistry, kind: &str, id: &str, yaml: &str) {
    let parsed = artifact_loader::parse_proposal(kind, yaml).unwrap();
    parsed.insert_into(registry).unwrap();
    registry.sources.insert(
        (kind.into(), id.into(), 1),
        ArtifactSource {
            path: "/must-not-be-reopened".into(),
            bytes: yaml.as_bytes().to_vec(),
        },
    );
}

fn route(id: &str) -> String {
    format!("id: {id}\nschema_version: 1\nversion: 1\nlifecycle_state: active\neffect: allow\n")
}

fn overlay(current: &mut CapturedCurrentState, kind: &str, id: &str, yaml: &str) {
    insert(&mut current.overlay.registry, kind, id, yaml);
    let at = "2026-09-24T00:00:00Z".parse().unwrap();
    current.overlay.learned.push(LearnedArtifact {
        kind: kind.into(),
        artifact_id: id.into(),
        version: 1,
        namespace: ArtifactNamespace::Overlay,
        provenance: Provenance::LegacyMigration { discovered_at: at },
        accepted_via: None,
        learned_at: at,
        compatibility: CompatibilityStatus::Compatible,
        nomination: NominationStatus::None,
        pending_reconfirmation_id: None,
        pending_yaml_digest: Some(digest_of_bytes(yaml.as_bytes()).to_string()),
        accepted_dependency_fingerprint: None,
        source_path: None,
        accepted_base_epoch: None,
    });
    current.overlay.controls.insert(
        (kind.into(), id.into(), 1),
        CapturedOverlayControl {
            lifecycle: Some(Lifecycle::Active),
            highest_active_version: Some(1),
            source_present: true,
            recoverable_blob_present: true,
        },
    );
}

fn reasons<'a>(view: &'a OverlayViewAssessment, id: &str) -> Vec<&'a str> {
    view.consequences
        .iter()
        .filter(|item| item.artifact_id == id)
        .map(|item| item.reason.as_str())
        .collect()
}

#[test]
fn captured_overlay_review_preserves_unchanged_authority_and_is_deterministic() {
    let mut current = state();
    overlay(&mut current, "route", "r", &route("r"));
    let first = assess(&current, &current.base.registry).unwrap();
    assert_eq!(first, assess(&current, &current.base.registry).unwrap());
    assert_eq!(
        first.before.effective_artifacts,
        vec![("route".into(), "r".into(), 1)]
    );
    assert_eq!(first.before, first.after);
    assert!(first.blockers.is_empty());
    assert!(!first.reusable_authority_reconfirmation_required);
    assert!(first.after.consequences.is_empty());
}

#[test]
fn captured_overlay_review_requires_reconfirmation_when_base_epoch_changes() {
    let mut current = state();
    overlay(&mut current, "route", "r", &route("r"));
    let mut candidate = ArtifactRegistry::default();
    insert(&mut candidate, "route", "base", &route("base"));
    let result = assess(&current, &candidate).unwrap();
    assert!(result.reusable_authority_reconfirmation_required);
    assert_eq!(result.before.effective_artifacts.len(), 1);
    assert!(result.after.effective_artifacts.is_empty());
    assert!(reasons(&result.after, "r").contains(&"base_epoch_reconfirmation_required"));
    assert_eq!(
        current.overlay.learned[0].compatibility,
        CompatibilityStatus::Compatible
    );
}

#[test]
fn captured_overlay_review_missing_highest_source_blocks_even_if_blob_recoverable() {
    let mut current = state();
    overlay(&mut current, "route", "r", &route("r"));
    current
        .overlay
        .controls
        .get_mut(&("route".into(), "r".into(), 1))
        .unwrap()
        .highest_active_version = Some(2);
    current.overlay.controls.insert(
        ("route".into(), "r".into(), 2),
        CapturedOverlayControl {
            lifecycle: Some(Lifecycle::Active),
            highest_active_version: Some(2),
            source_present: false,
            recoverable_blob_present: true,
        },
    );
    let result = assess(&current, &current.base.registry).unwrap();
    assert!(result
        .blockers
        .iter()
        .any(|x| x.reason == "highest_active_source_missing" && x.version == 2));
    assert!(result.before.effective_artifacts.is_empty());
    assert_eq!(current.overlay.registry.sources.len(), 1);
}

#[test]
fn captured_overlay_review_excludes_erased_pending_and_retired_without_revival() {
    for status in [
        CompatibilityStatus::Erased,
        CompatibilityStatus::ReconfirmationRequired,
    ] {
        let mut current = state();
        overlay(&mut current, "route", "r", &route("r"));
        current.overlay.learned[0].compatibility = status;
        let result = assess(&current, &current.base.registry).unwrap();
        assert!(result.after.effective_artifacts.is_empty());
        assert!(!reasons(&result.after, "r").is_empty());
    }
    let mut current = state();
    overlay(&mut current, "route", "r", &route("r"));
    current
        .overlay
        .controls
        .get_mut(&("route".into(), "r".into(), 1))
        .unwrap()
        .lifecycle = Some(Lifecycle::Retired);
    let result = assess(&current, &current.base.registry).unwrap();
    assert!(result.after.effective_artifacts.is_empty());
    assert!(!result.blockers.is_empty());
}

#[test]
fn captured_overlay_review_reports_collision_digest_mismatch_and_missing_provenance() {
    let mut current = state();
    overlay(&mut current, "route", "r", &route("r"));
    let mut candidate = ArtifactRegistry::default();
    insert(&mut candidate, "route", "r", &route("r"));
    let result = assess(&current, &candidate).unwrap();
    assert!(reasons(&result.after, "r").contains(&"base_overlay_collision"));
    current.overlay.learned[0].pending_yaml_digest = Some(digest_of_bytes(b"tampered").to_string());
    let result = assess(&current, &current.base.registry).unwrap();
    assert!(reasons(&result.after, "r").contains(&"approved_overlay_digest_mismatch"));
    assert!(result.after.effective_artifacts.is_empty());
    current.overlay.learned.clear();
    let result = assess(&current, &current.base.registry).unwrap();
    assert!(reasons(&result.after, "r").contains(&"missing_provenance"));
}

#[test]
fn captured_overlay_review_converges_transitive_dangling_dependencies() {
    let mut current = state();
    let workflow = "id: w\nschema_version: 1\nversion: 1\nlifecycle_state: active\npurpose: p\nrequired_agent: absent\nrequired_capability_pack: absent\n";
    overlay(&mut current, "workflow", "w", workflow);
    overlay(&mut current, "route", "r", &(route("r") + "workflow: w\n"));
    let result = assess(&current, &current.base.registry).unwrap();
    assert!(result.after.effective_artifacts.is_empty());
    assert!(reasons(&result.after, "w").contains(&"dangling_dependencies"));
    assert!(result
        .after
        .consequences
        .iter()
        .any(|x| x.artifact_id == "r" && x.details == ["workflow:w"]));
}

#[test]
fn captured_overlay_review_fingerprint_binds_controls_and_ignores_input_order() {
    let mut current = state();
    overlay(&mut current, "route", "a", &route("a"));
    overlay(&mut current, "route", "z", &route("z"));
    let first = assess(&current, &current.base.registry).unwrap();
    current.overlay.learned.reverse();
    current.overlay.registry.routes.reverse();
    assert_eq!(first, assess(&current, &current.base.registry).unwrap());
    current.overlay.learned[0].compatibility = CompatibilityStatus::ReconfirmationRequired;
    let changed = assess(&current, &current.base.registry).unwrap();
    assert_ne!(first.control_fingerprint, changed.control_fingerprint);
}

#[test]
fn captured_overlay_review_rejects_duplicate_provenance_and_missing_control() {
    let mut current = state();
    overlay(&mut current, "route", "r", &route("r"));
    current
        .overlay
        .learned
        .push(current.overlay.learned[0].clone());
    assert!(assess(&current, &current.base.registry).is_err());
    current.overlay.learned.pop();
    current.overlay.controls.clear();
    assert!(assess(&current, &current.base.registry).is_err());
}

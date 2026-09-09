use super::*;
use crate::artifact_loader::ArtifactSource;
use crate::store::learned_artifacts::{NominationStatus, Provenance};
use openspine_schemas::artifact::ArtifactNamespace;
use openspine_schemas::digest::digest_of_bytes;

fn fixture(view: &mut ArtifactRegistry, id: &str) -> LearnedArtifact {
    let yaml = format!(
        "id: {id}\nschema_version: 1\nversion: 1\nlifecycle_state: active\neffect: allow\n"
    );
    let mut parsed = artifact_loader::parse_proposal("route", &yaml).unwrap();
    parsed.activate();
    parsed.insert_into(view).unwrap();
    view.sources.insert(
        ("route".into(), id.into(), 1),
        ArtifactSource {
            path: std::path::PathBuf::from("not-a-capture-input.yaml"),
            bytes: yaml.as_bytes().to_vec(),
        },
    );
    let at = "2026-09-09T00:00:00Z".parse::<jiff::Timestamp>().unwrap();
    LearnedArtifact {
        kind: "route".into(),
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
    }
}

fn assess(view: &ArtifactRegistry, learned: &[LearnedArtifact]) -> AdmissionFindings {
    evaluate(view, learned, &HashSet::new())
}

#[test]
fn captured_admission_accepts_matching_approved_bytes_for_both_statuses() {
    for status in [
        CompatibilityStatus::Compatible,
        CompatibilityStatus::OwnerAccepted,
    ] {
        let mut view = ArtifactRegistry::default();
        let mut item = fixture(&mut view, "r");
        item.compatibility = status;
        let result = assess(&view, &[item]);
        assert!(result.digest_invalid.is_empty());
        assert!(result.missing.is_empty());
        assert!(result.collisions.is_empty());
    }
}

#[test]
fn captured_admission_reports_missing_approved_source() {
    for status in [
        CompatibilityStatus::Compatible,
        CompatibilityStatus::OwnerAccepted,
    ] {
        let mut view = ArtifactRegistry::default();
        let mut item = fixture(&mut view, "r");
        item.compatibility = status;
        view.sources.clear();
        let result = assess(&view, &[item]);
        assert_eq!(result.digest_invalid.len(), 1);
        assert_eq!(
            result.digest_invalid[0].dangling_references,
            ["approved_overlay_source_missing"]
        );
    }
}

#[test]
fn captured_admission_does_not_substitute_another_source_version() {
    let mut view = ArtifactRegistry::default();
    let item = fixture(&mut view, "r");
    let bytes = view
        .sources
        .remove(&("route".into(), "r".into(), 1))
        .unwrap();
    view.sources.insert(("route".into(), "r".into(), 2), bytes);
    let result = assess(&view, &[item]);
    assert_eq!(result.digest_invalid.len(), 1);
    assert_eq!(result.digest_invalid[0].version, 1);
}

#[test]
fn captured_admission_distinguishes_missing_digest_and_changed_bytes() {
    let mut view = ArtifactRegistry::default();
    let mut item = fixture(&mut view, "r");
    item.pending_yaml_digest = None;
    let result = assess(&view, &[item.clone()]);
    assert_eq!(
        result.digest_invalid[0].dangling_references,
        ["approved_overlay_digest_missing"]
    );
    item.pending_yaml_digest = Some(digest_of_bytes(b"other bytes").to_string());
    let result = assess(&view, &[item]);
    assert_eq!(
        result.digest_invalid[0].dangling_references,
        ["approved_overlay_digest_mismatch"]
    );
    assert!(result.missing.is_empty());
}

#[test]
fn captured_admission_does_not_use_another_learned_version_as_provenance() {
    let mut view = ArtifactRegistry::default();
    let mut item = fixture(&mut view, "r");
    item.version = 2;
    item.pending_yaml_digest = None;
    let result = assess(&view, &[item]);
    assert!(result.digest_invalid.is_empty());
    assert_eq!(result.missing.len(), 1);
    assert_eq!(result.missing[0].version, 1);
}

#[test]
fn captured_admission_leaves_pending_and_erased_handling_to_their_own_phases() {
    for status in [
        CompatibilityStatus::ReconfirmationRequired,
        CompatibilityStatus::Erased,
    ] {
        let mut view = ArtifactRegistry::default();
        let mut item = fixture(&mut view, "r");
        item.compatibility = status;
        item.pending_yaml_digest = None;
        let result = assess(&view, &[item]);
        assert!(result.digest_invalid.is_empty());
        assert_eq!(view.routes.len(), 1);
    }
}

#[test]
fn captured_admission_collisions_are_kind_scoped_and_exact_version() {
    let mut view = ArtifactRegistry::default();
    let mut item = fixture(&mut view, "r");
    let different_kind = HashSet::from([("agent".into(), "r".into())]);
    assert!(evaluate(&view, &[item.clone()], &different_kind)
        .collisions
        .is_empty());
    let base = HashSet::from([("route".into(), "r".into())]);
    let result = evaluate(&view, &[item.clone()], &base);
    assert_eq!(result.collisions, [("route".into(), "r".into())]);
    assert_eq!(result.collision_orphans.len(), 1);
    assert_eq!(result.collision_orphans[0].version, 1);
    item.version = 2;
    let result = evaluate(&view, &[item], &base);
    assert_eq!(result.collisions.len(), 1);
    assert!(result.collision_orphans.is_empty());
    assert_eq!(result.missing.len(), 1);
}

#[test]
fn captured_admission_preserves_overlapping_collision_and_integrity_findings() {
    let mut view = ArtifactRegistry::default();
    let mut item = fixture(&mut view, "r");
    item.pending_yaml_digest = None;
    let result = evaluate(
        &view,
        &[item],
        &HashSet::from([("route".into(), "r".into())]),
    );
    assert_eq!(result.collision_orphans.len(), 1);
    assert_eq!(result.digest_invalid.len(), 1);
    assert!(result.missing.is_empty());
    assert_eq!(view.routes.len(), 1);
}

#[test]
fn captured_admission_digest_findings_do_not_depend_on_learned_row_order() {
    let mut view = ArtifactRegistry::default();
    let mut a = fixture(&mut view, "a");
    let mut z = fixture(&mut view, "z");
    a.pending_yaml_digest = None;
    z.pending_yaml_digest = None;
    assert_eq!(
        assess(&view, &[z.clone(), a.clone()]),
        assess(&view, &[a, z])
    );
}

#[test]
fn captured_admission_missing_findings_do_not_depend_on_registry_order() {
    let mut view = ArtifactRegistry::default();
    fixture(&mut view, "z");
    fixture(&mut view, "a");
    let first = assess(&view, &[]);
    view.routes.reverse();
    assert_eq!(first, assess(&view, &[]));
}

#[test]
fn captured_admission_preserves_duplicate_integrity_evidence() {
    let mut view = ArtifactRegistry::default();
    let mut item = fixture(&mut view, "r");
    item.pending_yaml_digest = None;
    let result = assess(&view, &[item.clone(), item]);
    assert_eq!(result.digest_invalid.len(), 2);
}

#[test]
fn captured_admission_never_reopens_paths_or_changes_captured_inputs() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("r.yaml");
    let mut view = ArtifactRegistry::default();
    let mut item = fixture(&mut view, "r");
    let key = ("route".into(), "r".into(), 1);
    view.sources.get_mut(&key).unwrap().path = path.clone();
    item.source_path = Some(path.to_string_lossy().into_owned());
    std::fs::write(&path, b"changed on disk, not a review input").unwrap();
    let sources = view.sources.clone();
    let routes = serde_json::to_value(&view.routes).unwrap();
    let learned = item.clone();
    let first = assess(&view, std::slice::from_ref(&item));
    assert!(first.digest_invalid.is_empty());
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"changed on disk, not a review input"
    );
    std::fs::remove_file(&path).unwrap();
    assert_eq!(first, assess(&view, std::slice::from_ref(&item)));
    assert_eq!(view.sources, sources);
    assert_eq!(serde_json::to_value(&view.routes).unwrap(), routes);
    assert_eq!(item, learned);
    assert!(!path.exists());
}

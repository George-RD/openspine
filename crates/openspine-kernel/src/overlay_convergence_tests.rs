//! Captured-state contract for the shared dependency evaluator (#285).
use super::*;
use crate::store::learned_artifacts::{dependency_fingerprint, NominationStatus, Provenance};
use openspine_schemas::artifact::ArtifactNamespace;
use std::collections::BTreeMap;

fn route(id: &str, agent: Option<&str>, workflow: Option<&str>) -> String {
    let mut yaml = format!(
        "id: {id}\nschema_version: 1\nversion: 1\nlifecycle_state: active\neffect: allow\n"
    );
    if let Some(agent) = agent {
        yaml.push_str(&format!("agent: {agent}\n"));
    }
    if let Some(workflow) = workflow {
        yaml.push_str(&format!("workflow: {workflow}\n"));
    }
    yaml
}

fn registry(yaml: &str) -> ArtifactRegistry {
    let mut registry = ArtifactRegistry::default();
    let mut parsed = crate::artifact_loader::parse_proposal("route", yaml).unwrap();
    parsed.activate();
    parsed.insert_into(&mut registry).unwrap();
    registry
}

fn accepted(id: &str, yaml: &str) -> LearnedArtifact {
    let at = "2026-09-09T00:00:00Z".parse::<jiff::Timestamp>().unwrap();
    LearnedArtifact {
        kind: "route".into(),
        artifact_id: id.into(),
        version: 1,
        namespace: ArtifactNamespace::Overlay,
        provenance: Provenance::LegacyMigration { discovered_at: at },
        accepted_via: None,
        learned_at: at,
        compatibility: CompatibilityStatus::OwnerAccepted,
        nomination: NominationStatus::None,
        pending_reconfirmation_id: None,
        pending_yaml_digest: Some(digest_of_bytes(yaml.as_bytes()).to_string()),
        accepted_dependency_fingerprint: Some(dependency_fingerprint(&[])),
        source_path: None,
        accepted_base_epoch: None,
    }
}

fn sources(id: &str, yaml: &str) -> BTreeMap<(String, String, u32), Vec<u8>> {
    BTreeMap::from([(("route".into(), id.into(), 1), yaml.as_bytes().to_vec())])
}

#[test]
fn captured_dependency_evaluation_does_not_reread_changed_source_path() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("r.yaml");
    let yaml = route("r", None, None);
    let mut item = accepted("r", &yaml);
    item.source_path = Some(path.to_string_lossy().into_owned());
    let capture = sources("r", &yaml);
    std::fs::write(&path, route("r", Some("injected"), None)).unwrap();
    let mut view = registry(&yaml);
    let (ordinary, invalid) =
        evaluate_captured_dependencies(&mut view, &[item], &HashSet::new(), &[], &capture);
    assert!(ordinary.is_empty());
    assert!(
        invalid.is_empty(),
        "must use the supplied capture: {invalid:?}"
    );
    assert_eq!(view.routes.len(), 1);
}

#[test]
fn captured_dependency_evaluation_cannot_replace_tampered_capture_with_disk() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("r.yaml");
    let original = route("r", None, None);
    let changed = route("r", Some("changed"), None);
    std::fs::write(&path, &original).unwrap();
    let mut item = accepted("r", &original);
    item.source_path = Some(path.to_string_lossy().into_owned());
    let mut view = registry(&changed);
    let (_, invalid) = evaluate_captured_dependencies(
        &mut view,
        &[item],
        &HashSet::new(),
        &[],
        &sources("r", &changed),
    );
    assert_eq!(invalid.len(), 1);
    assert_eq!(
        invalid[0].dangling_references,
        ["owner_accepted_digest_tampered"]
    );
    assert!(view.routes.is_empty());
}

#[test]
fn captured_dependency_evaluation_missing_capture_does_not_fall_back_to_path() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("r.yaml");
    let yaml = route("r", None, None);
    std::fs::write(&path, &yaml).unwrap();
    let mut item = accepted("r", &yaml);
    item.source_path = Some(path.to_string_lossy().into_owned());
    let mut view = registry(&yaml);
    let (_, invalid) =
        evaluate_captured_dependencies(&mut view, &[item], &HashSet::new(), &[], &BTreeMap::new());
    assert_eq!(invalid.len(), 1);
    assert!(view.routes.is_empty());
}

#[test]
fn captured_dependency_outcomes_are_canonical_across_learned_row_order() {
    let a = accepted("a", &route("a", None, None));
    let z = accepted("z", &route("z", None, None));
    let evaluate = |items: &[LearnedArtifact]| {
        evaluate_captured_dependencies(
            &mut ArtifactRegistry::default(),
            items,
            &HashSet::new(),
            &[],
            &BTreeMap::new(),
        )
    };
    assert_eq!(evaluate(&[z.clone(), a.clone()]), evaluate(&[a, z]));
}

#[test]
fn captured_dependency_evaluation_preserves_accepted_dangling_references() {
    let yaml = route("r", Some("previously-missing"), None);
    let mut item = accepted("r", &yaml);
    item.accepted_dependency_fingerprint =
        Some(dependency_fingerprint(&["agent:previously-missing".into()]));
    let mut view = registry(&yaml);
    let (ordinary, invalid) = evaluate_captured_dependencies(
        &mut view,
        &[item],
        &HashSet::new(),
        &[],
        &sources("r", &yaml),
    );
    assert!(ordinary.is_empty());
    assert!(invalid.is_empty());
    assert_eq!(view.routes.len(), 1);
}

#[test]
fn captured_dependency_evaluation_invalidates_new_dangling_without_removing_base() {
    let yaml = route("r", Some("newly-missing"), None);
    let item = accepted("r", &yaml);
    let mut view = registry(&yaml);
    let (_, invalid) = evaluate_captured_dependencies(
        &mut view,
        &[item],
        &HashSet::from([("route".into(), "r".into())]),
        &[],
        &sources("r", &yaml),
    );
    assert_eq!(invalid.len(), 1);
    assert_eq!(invalid[0].dangling_references, ["agent:newly-missing"]);
    assert_eq!(
        view.routes.len(),
        1,
        "a base collision must never be removed"
    );
}

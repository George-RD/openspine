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

fn insert(view: &mut ArtifactRegistry, kind: &str, yaml: &str) {
    let mut parsed = crate::artifact_loader::parse_proposal(kind, yaml).unwrap();
    parsed.activate();
    parsed.insert_into(view).unwrap();
}

fn add_registry_source(view: &mut ArtifactRegistry, yaml: &str) {
    view.sources.insert(
        ("route".into(), "r".into(), 1),
        crate::artifact_loader::ArtifactSource {
            path: "unused-registry-source.yaml".into(),
            bytes: yaml.as_bytes().to_vec(),
        },
    );
}

#[test]
fn captured_missing_source_cannot_fall_back_to_registry_bytes() {
    let yaml = route("r", None, None);
    let mut view = registry(&yaml);
    add_registry_source(&mut view, &yaml);
    let (_, invalid) = evaluate_captured_dependencies(
        &mut view,
        &[accepted("r", &yaml)],
        &HashSet::new(),
        &[],
        &BTreeMap::new(),
    );
    assert_eq!(invalid.len(), 1);
    assert!(view.routes.is_empty());
}

#[test]
fn captured_sources_are_exact_version_bound() {
    let yaml = route("r", None, None);
    let mut view = registry(&yaml);
    let wrong_version =
        BTreeMap::from([(("route".into(), "r".into(), 2), yaml.as_bytes().to_vec())]);
    let (_, invalid) = evaluate_captured_dependencies(
        &mut view,
        &[accepted("r", &yaml)],
        &HashSet::new(),
        &[],
        &wrong_version,
    );
    assert_eq!(invalid.len(), 1);
    assert!(view.routes.is_empty());
}

#[test]
fn legacy_adapter_retains_registry_fallback_but_pure_evaluation_does_not() {
    let yaml = route("r", None, None);
    let mut view = registry(&yaml);
    add_registry_source(&mut view, &yaml);
    let (ordinary, requests, invalid) = crate::overlay_compat::converge_owner_accepted_dependencies(
        &mut view,
        &[accepted("r", &yaml)],
        &HashSet::new(),
        &[],
    );
    assert!(ordinary.is_empty());
    assert!(requests.is_empty());
    assert!(invalid.is_empty());
    assert_eq!(view.routes.len(), 1);
}

#[test]
fn canonical_ordinary_results_keep_review_ids_bound_to_exact_version() {
    let a_yaml = route("a", None, None);
    let z_yaml = route("z", None, None);
    let mut a = accepted("a", &a_yaml);
    let mut z = accepted("z", &z_yaml);
    a.compatibility = CompatibilityStatus::ReconfirmationRequired;
    z.compatibility = CompatibilityStatus::ReconfirmationRequired;
    a.pending_reconfirmation_id = Some(ulid::Ulid::from(1_u128));
    z.pending_reconfirmation_id = Some(ulid::Ulid::from(2_u128));
    let mut stale_a = a.clone();
    stale_a.version = 0;
    stale_a.pending_reconfirmation_id = Some(ulid::Ulid::from(3_u128));
    let evaluate = |items: &[LearnedArtifact]| {
        let mut view = registry(&a_yaml);
        insert(&mut view, "route", &z_yaml);
        let result = crate::overlay_compat::apply_compatibility(&mut view, items);
        assert!(view.routes.is_empty());
        result
    };
    let (ordinary, requests) = evaluate(&[stale_a.clone(), z.clone(), a.clone()]);
    assert_eq!(ordinary.len(), 2);
    assert_eq!(ordinary[0].artifact_id, "a");
    assert_eq!(ordinary[1].artifact_id, "z");
    assert_eq!(
        requests,
        [ulid::Ulid::from(1_u128), ulid::Ulid::from(2_u128)]
    );
    assert_eq!((ordinary, requests), evaluate(&[a, z, stale_a]));
}

#[test]
fn captured_dependencies_converge_through_ordinary_accepted_ordinary_chain() {
    let agent_yaml = include_str!("../../../artifacts/lyra/agents/main_assistant_agent.yaml");
    let pack_yaml = include_str!("../../../artifacts/lyra/packs/owner_control_basic_pack.yaml");
    let workflow_yaml = "id: b\nschema_version: 1\nversion: 1\nlifecycle_state: active\npurpose: p\nrequired_agent: main_assistant_agent\nrequired_capability_pack: owner_control_basic_pack\n";
    let route_yaml = route("r", None, Some("b"));
    let mut trigger = accepted("main_assistant_agent", agent_yaml);
    trigger.kind = "agent".into();
    trigger.compatibility = CompatibilityStatus::ReconfirmationRequired;
    let mut middle = accepted("b", workflow_yaml);
    middle.kind = "workflow".into();
    let mut dependent = accepted("r", &route_yaml);
    dependent.compatibility = CompatibilityStatus::Compatible;
    let capture = BTreeMap::from([(
        ("workflow".into(), "b".into(), 1),
        workflow_yaml.as_bytes().to_vec(),
    )]);
    let evaluate = |items: &[LearnedArtifact]| {
        let mut view = registry(&route_yaml);
        insert(&mut view, "agent", agent_yaml);
        insert(&mut view, "pack", pack_yaml);
        insert(&mut view, "workflow", workflow_yaml);
        let result =
            evaluate_captured_dependencies(&mut view, items, &HashSet::new(), &[], &capture);
        assert!(view.agents.is_empty());
        assert!(view.workflows.is_empty());
        assert!(view.routes.is_empty());
        assert_eq!(view.packs.len(), 1);
        result
    };
    let result = evaluate(&[dependent.clone(), middle.clone(), trigger.clone()]);
    assert_eq!(result.0.len(), 2);
    assert_eq!(result.0[0].artifact_id, "main_assistant_agent");
    assert_eq!(result.0[1].artifact_id, "r");
    assert_eq!(result.1.len(), 1);
    assert_eq!(result.1[0].artifact_id, "b");
    assert_eq!(
        result.1[0].dangling_references,
        ["agent:main_assistant_agent"]
    );
    assert_eq!(result, evaluate(&[trigger, middle, dependent]));
}

#[test]
fn captured_prior_invalidations_remain_canonical_without_losing_facts() {
    let yaml = route("r", None, None);
    let prior = OrphanedArtifact {
        kind: "route".into(),
        artifact_id: "r".into(),
        version: 1,
        dangling_references: vec!["z".into(), "a".into(), "a".into()],
    };
    let (_, invalid) = evaluate_captured_dependencies(
        &mut ArtifactRegistry::default(),
        &[accepted("r", &yaml)],
        &HashSet::new(),
        &[prior],
        &sources("r", &yaml),
    );
    assert_eq!(invalid.len(), 1);
    assert_eq!(invalid[0].dangling_references, ["a", "a", "z"]);
}

#[test]
fn captured_matching_bytes_still_require_a_recorded_digest() {
    let yaml = route("r", None, None);
    let mut item = accepted("r", &yaml);
    item.pending_yaml_digest = None;
    let mut view = registry(&yaml);
    let (_, invalid) = evaluate_captured_dependencies(
        &mut view,
        &[item],
        &HashSet::new(),
        &[],
        &sources("r", &yaml),
    );
    assert_eq!(invalid.len(), 1);
    assert!(view.routes.is_empty());
}

#[test]
fn captured_malformed_bytes_with_matching_digest_remain_invalid() {
    let malformed = "not: [valid";
    let mut view = registry(&route("r", None, None));
    let (_, invalid) = evaluate_captured_dependencies(
        &mut view,
        &[accepted("r", malformed)],
        &HashSet::new(),
        &[],
        &sources("r", malformed),
    );
    assert_eq!(invalid.len(), 1);
    assert_eq!(
        invalid[0].dangling_references,
        ["owner_accepted_parse_failed"]
    );
    assert!(view.routes.is_empty());
}

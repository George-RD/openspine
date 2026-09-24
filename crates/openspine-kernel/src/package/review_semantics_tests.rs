use super::*;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::route::{Route, RouteEffect};

fn route(id: &str) -> Route {
    Route {
        id: id.into(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        priority: Some(1),
        effect: RouteEffect::Allow,
        when: Default::default(),
        agent: None,
        workflow: None,
        capability_pack: None,
        persona: None,
    }
}

#[test]
fn typed_route_change_exposes_complete_before_and_after_bindings() {
    let mut before = ArtifactRegistry::default();
    before.routes.push(route("owner-route"));
    let mut after = before.clone();
    after.routes[0].effect = RouteEffect::Deny;
    after.routes[0].priority = Some(9);
    after.routes[0].when.channel_account = Some("owner-account".into());
    let result = compare(&before, &after);
    assert_ne!(result.before_runtime_digest, result.after_runtime_digest);
    assert_eq!(result.changes.len(), 1);
    let change = &result.changes[0];
    assert_eq!(change.kind, "route");
    assert_eq!(change.id, "owner-route");
    assert_eq!(change.before.as_ref().unwrap()["fields"]["effect"], "allow");
    assert_eq!(change.after.as_ref().unwrap()["fields"]["effect"], "deny");
    assert_eq!(change.after.as_ref().unwrap()["fields"]["priority"], 9);
    assert_eq!(
        change.after.as_ref().unwrap()["fields"]["when"]["channel_account"],
        "owner-account"
    );
}

fn fixture_registry() -> ArtifactRegistry {
    crate::artifact_loader::load_base_registry(
        &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra"),
    )
    .unwrap()
}

#[test]
fn authority_review_retains_pack_policy_and_workflow_requirements_and_catalog_text() {
    use openspine_schemas::action::ActionId;
    use openspine_schemas::workflow::{ApprovalSemantics, WorkflowState};
    let before = fixture_registry();
    let mut after = before.clone();
    let pack = after.packs.get_mut("owner_control_basic_pack").unwrap();
    pack.candidate_allowed_actions
        .push(ActionId::new("email.create_draft"));
    pack.denied_actions.push(ActionId::new("email.send"));
    pack.approval_required
        .push(ActionId::new("email.create_draft"));
    pack.constraints.max_runtime_seconds = Some(17);
    let policy_id = after.policies.keys().min().unwrap().clone();
    after
        .policies
        .get_mut(&policy_id)
        .unwrap()
        .constraints
        .max_runtime_seconds = Some(19);
    let workflow_id = after.workflows.keys().min().unwrap().clone();
    after
        .workflows
        .get_mut(&workflow_id)
        .unwrap()
        .states
        .push(WorkflowState {
            id: "owner-review".into(),
            steps: vec![],
            approval: ApprovalSemantics::Required,
            approval_action: Some(ActionId::new("email.create_draft")),
            escalation: None,
        });
    let result = compare(&before, &after);
    let change = |kind: &str| result.changes.iter().find(|row| row.kind == kind).unwrap();
    let pack = &change("pack").after.as_ref().unwrap()["fields"];
    assert_eq!(pack["constraints"]["max_runtime_seconds"], 17);
    for name in [
        "candidate_allowed_actions",
        "approval_required",
        "denied_actions",
    ] {
        assert!(!pack[name].as_array().unwrap().is_empty());
    }
    assert_eq!(
        change("policy").after.as_ref().unwrap()["fields"]["constraints"]["max_runtime_seconds"],
        19
    );
    let workflow = &change("workflow").after.as_ref().unwrap()["fields"];
    assert_eq!(
        workflow["states"].as_array().unwrap().last().unwrap()["approval_action"],
        "email.create_draft"
    );
    assert!(workflow["required_capability_pack"].is_string());
    assert_eq!(
        result.action_descriptors["email.create_draft"]["source"],
        "kernel-action-catalog"
    );
    assert_eq!(
        result.action_descriptors["email.create_draft"]["delegation"]["semantics"]["owner_object"],
        "email draft"
    );
    assert!(result.action_descriptors["openspine.status.read"]["tool"]["description"].is_string());
    assert!(!result.supporting_artifacts.is_empty());
}

#[test]
fn hash_insertion_and_route_iteration_order_do_not_change_review() {
    let before = fixture_registry();
    let mut reordered = before.clone();
    reordered.routes.reverse();
    reordered.agents = before
        .agents
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    reordered.sources = before
        .sources
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    assert_eq!(
        serde_json::to_value(compare(&before, &before)).unwrap(),
        serde_json::to_value(compare(&reordered, &reordered)).unwrap(),
    );
}

#[test]
fn missing_catalog_meaning_and_oversized_required_detail_are_blockers() {
    use openspine_schemas::action::ActionId;
    let before = fixture_registry();
    let mut after = before.clone();
    let pack = after.packs.get_mut("owner_control_basic_pack").unwrap();
    pack.candidate_allowed_actions
        .push(ActionId::new("unknown.widening"));
    pack.constraints.external_visibility_max = Some("x".repeat(MAX_DETAIL_BYTES));
    let result = compare(&before, &after);
    assert!(result
        .blockers
        .iter()
        .any(|code| code == "unknown-catalog-action"));
    assert!(result
        .blockers
        .iter()
        .any(|code| code == "missing-owner-action-description"));
    assert!(result
        .blockers
        .iter()
        .any(|code| code == "semantic-detail-limit-exceeded"));
    let detail = result
        .changes
        .iter()
        .find(|row| row.kind == "pack")
        .unwrap()
        .after
        .as_ref()
        .unwrap();
    assert_eq!(detail["detail_status"], "limit-exceeded");
    assert_eq!(detail["required_detail_rendered"], false);
    assert!(detail.get("fields").is_none());
    assert!(detail["detail_digest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
}

#[test]
fn byte_only_edit_is_visible_without_an_authority_verdict() {
    let before = fixture_registry();
    let mut after = before.clone();
    let key = after.sources.keys().min().unwrap().clone();
    after
        .sources
        .get_mut(&key)
        .unwrap()
        .bytes
        .extend_from_slice(b"\n# formatting\n");
    let result = compare(&before, &after);
    assert_eq!(result.before_runtime_digest, result.after_runtime_digest);
    assert_eq!(result.source_changes.len(), 1);
    assert_eq!(result.changes.len(), 1);
    let output = serde_json::to_value(result).unwrap();
    assert!(output.get("safe").is_none());
    assert!(output.get("approved").is_none());
}

#[test]
fn nonfinite_route_threshold_cannot_be_rendered_as_an_ordinary_null() {
    use openspine_schemas::route::RouteActorWhen;
    let before = ArtifactRegistry::default();
    let mut after = before.clone();
    let mut hostile = route("nonfinite");
    hostile.when.actor = Some(RouteActorWhen {
        identity_confidence_min: Some(f64::NAN),
        ..Default::default()
    });
    after.routes.push(hostile);
    let result = compare(&before, &after);
    assert!(result
        .blockers
        .iter()
        .any(|code| code == "unrenderable-typed-value"));
}

#[test]
fn unresolved_workflow_state_semantics_block_complete_review() {
    let before = fixture_registry();
    let mut after = before.clone();
    let workflow = after.workflows.values_mut().next().unwrap();
    workflow.initial_state = Some("missing-state".into());
    let result = compare(&before, &after);
    assert!(result
        .blockers
        .iter()
        .any(|code| code == "invalid-workflow-semantics"));
}

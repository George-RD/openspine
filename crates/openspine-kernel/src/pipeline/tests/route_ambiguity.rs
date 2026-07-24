use crate::pipeline::driver::run_pipeline;
use crate::pipeline::lanes::{owner_control_lane, EventInputs};
use crate::pipeline::AppState;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::{test_state, test_state_with_telegram};
use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::egress::EgressClass;
use openspine_schemas::event::{EventEnvelope, EventType, Lane};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// --- wire-authority-equivalence-selection: route tie resolves through
// authority-equivalence classes (D-109/D-110) ---

fn owner_inputs() -> EventInputs {
    EventInputs {
        chat_id: 555,
        text: "hello lyra".to_string(),
        thread_id: None,
        owner_verified: Some(crate::telegram::VerifiedOwnerContext::test_new()),
        principal_override: None,
        event_type_override: None,
        timer_event_id: None,
        correlated_task_id: None,
        dispatch_key: None,
        dispatch_timer_id: None,
    }
}

async fn notification_state() -> (AppState, MockServer) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": 555, "type": "private"},
                "from": {"id": 1, "is_bot": true, "first_name": "bot"},
                "text": "ok"
            }
        })))
        .mount(&server)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".to_string(),
        server.uri().parse().expect("mock URL"),
    ));
    (state, server)
}

fn widen_selected_pack_after_resolution(
    state: &AppState,
    _event: &EventEnvelope,
    _lane: Lane,
) -> anyhow::Result<bool> {
    let mut registry = state.registry.write();
    let pack_id = registry
        .routes
        .iter()
        .find(|route| route.id == "owner_telegram_main_assistant")
        .and_then(|route| route.capability_pack.clone())
        .expect("base owner route names a pack");
    registry
        .packs
        .get_mut(&pack_id)
        .expect("base owner pack present")
        .allowed_egress_classes = vec![EgressClass::Search];
    Ok(false)
}

/// Two routes that tie (identical `when`, priority, and authority artifacts)
/// are authority-equivalent, so the ambiguous tie must resolve deterministically
/// to a single composed grant rather than escalating or dropping the event.
#[tokio::test]
async fn tied_authority_equivalent_routes_select_within_class() {
    let state = test_state();
    {
        let mut registry = state.registry.write();
        let mut duplicate = registry
            .routes
            .iter()
            .find(|r| r.id == "owner_telegram_main_assistant")
            .cloned()
            .expect("base owner route present in fixtures");
        duplicate.id = "owner_telegram_main_assistant_v2".to_string();
        registry.routes.push(duplicate);
    }
    let inputs = owner_inputs();
    let mut trace = Vec::new();
    let result = run_pipeline(
        &state,
        owner_control_lane(),
        &inputs,
        Timestamp::now(),
        &mut trace,
    )
    .await
    .expect("pipeline infra must not error");
    let grant = result.expect("tied authority-equivalent routes must resolve to a grant");
    assert_eq!(
        grant.route_id, "owner_telegram_main_assistant",
        "within-class selection must choose the lowest candidate id"
    );
    let (persisted, _, _) = state
        .store
        .find_task_grant_by_id(grant.id)
        .unwrap()
        .expect("selected grant must be persisted");
    assert_eq!(
        persisted.route_id, "owner_telegram_main_assistant",
        "the persisted grant must retain the selected route"
    );
    assert_eq!(state.store.count_task_grants().unwrap(), 1);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("route.ambiguous.escalated")
            .unwrap(),
        0,
        "an authority-equivalent tie must never escalate"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("route.ambiguous.class_selected")
            .unwrap(),
        1,
        "within-class selection must leave an audit breadcrumb"
    );
}

/// Two routes that tie but resolve to DIFFERENT authority classes must
/// escalate to the owner end-to-end (audit + immediate owner notification)
/// and never auto-pick one. The duplicate uses a different capability pack
/// whose composed authority tuple differs from the base route's.
#[tokio::test]
async fn tied_cross_class_routes_escalate_to_owner() {
    let (state, _server) = notification_state().await;
    {
        let mut registry = state.registry.write();
        let mut divergent = registry
            .routes
            .iter()
            .find(|r| r.id == "owner_telegram_main_assistant")
            .cloned()
            .expect("base owner route present in fixtures");
        divergent.id = "owner_telegram_main_assistant_divergent_pack".to_string();
        divergent.capability_pack = Some("plan_approval_pack".to_string());
        registry.routes.push(divergent);
    }
    let inputs = owner_inputs();
    let mut trace = Vec::new();
    let result = run_pipeline(
        &state,
        owner_control_lane(),
        &inputs,
        Timestamp::now(),
        &mut trace,
    )
    .await
    .expect("escalation is a handled denial, not an infra fault");
    assert!(
        result.is_none(),
        "a cross-class route tie must never auto-compose a grant"
    );
    assert_eq!(
        state.store.count_task_grants().unwrap(),
        0,
        "no grant may be persisted when authority classes conflict"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("route.ambiguous.escalated")
            .unwrap(),
        1,
        "cross-class tie must be audited as an escalation: {:?}",
        state.store.all_audit_event_jsons().unwrap()
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner.notified")
            .unwrap(),
        1,
        "cross-class tie must reach the owner via the escalation surface: {:?}",
        state.store.all_audit_event_jsons().unwrap()
    );
}

/// An invalid tied competitor must not be dropped before class construction:
/// doing so could turn an unsafe ambiguity into an apparently safe one-class
/// selection.
#[tokio::test]
async fn tied_route_with_missing_authority_metadata_escalates() {
    let (state, _server) = notification_state().await;
    {
        let mut registry = state.registry.write();
        let mut invalid = registry
            .routes
            .iter()
            .find(|r| r.id == "owner_telegram_main_assistant")
            .cloned()
            .expect("base owner route present in fixtures");
        invalid.id = "owner_telegram_missing_agent".to_string();
        invalid.agent = Some("missing_agent".to_string());
        registry.routes.push(invalid);
    }
    let inputs = owner_inputs();
    let mut trace = Vec::new();
    let result = run_pipeline(
        &state,
        owner_control_lane(),
        &inputs,
        Timestamp::now(),
        &mut trace,
    )
    .await
    .expect("invalid tied metadata escalates as a handled denial");

    assert!(result.is_none(), "invalid tied metadata must mint no grant");
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("route.ambiguous.escalated")
            .unwrap(),
        1,
        "invalid tied metadata must fail closed through escalation"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner.notified")
            .unwrap(),
        1,
        "invalid tied metadata must reach the owner"
    );
}

/// Composition failure is authority-relevant. The resolver must escalate
/// instead of dropping the failed competitor and selecting the remaining
/// valid class.
#[tokio::test]
async fn tied_route_composition_failure_escalates() {
    let (state, _server) = notification_state().await;
    {
        let mut registry = state.registry.write();
        let mut invalid = registry
            .routes
            .iter()
            .find(|r| r.id == "owner_telegram_main_assistant")
            .cloned()
            .expect("base owner route present in fixtures");
        invalid.id = "owner_telegram_unknown_action".to_string();
        let source_pack_id = invalid
            .capability_pack
            .as_ref()
            .expect("base owner route names a pack");
        let mut invalid_pack = registry
            .packs
            .get(source_pack_id)
            .cloned()
            .expect("base owner pack present in fixtures");
        invalid_pack.id = "owner_control_unknown_action_pack".to_string();
        invalid_pack
            .candidate_allowed_actions
            .push(ActionId::new("unknown.route.action"));
        invalid.capability_pack = Some(invalid_pack.id.clone());
        registry.packs.insert(invalid_pack.id.clone(), invalid_pack);
        registry.routes.push(invalid);
    }
    let inputs = owner_inputs();
    let mut trace = Vec::new();
    let result = run_pipeline(
        &state,
        owner_control_lane(),
        &inputs,
        Timestamp::now(),
        &mut trace,
    )
    .await
    .expect("composition failure escalates as a handled denial");

    assert!(result.is_none(), "failed composition must mint no grant");
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("route.ambiguous.escalated")
            .unwrap(),
        1,
        "composition failure must fail closed through escalation"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner.notified")
            .unwrap(),
        1,
        "composition failure must reach the owner"
    );
}

/// Rated egress is effective live authority even though AD-147's frozen
/// five-field class projection omits it. Differing egress sets must therefore
/// escalate rather than auto-select within the nominal class.
#[tokio::test]
async fn tied_routes_differing_only_in_egress_escalate() {
    let (state, _server) = notification_state().await;
    {
        let mut registry = state.registry.write();
        let mut broader = registry
            .routes
            .iter()
            .find(|route| route.id == "owner_telegram_main_assistant")
            .cloned()
            .expect("base owner route present in fixtures");
        broader.id = "a_owner_telegram_egress".to_string();
        let source_pack_id = broader
            .capability_pack
            .as_ref()
            .expect("base owner route names a pack");
        let mut broader_pack = registry
            .packs
            .get(source_pack_id)
            .cloned()
            .expect("base owner pack present in fixtures");
        broader_pack.id = "owner_control_search_egress_pack".to_string();
        broader_pack.allowed_egress_classes = vec![EgressClass::Search];
        broader.capability_pack = Some(broader_pack.id.clone());
        registry.packs.insert(broader_pack.id.clone(), broader_pack);
        registry.routes.push(broader);
    }
    let inputs = owner_inputs();
    let mut trace = Vec::new();
    let result = run_pipeline(
        &state,
        owner_control_lane(),
        &inputs,
        Timestamp::now(),
        &mut trace,
    )
    .await
    .expect("egress mismatch escalates as a handled denial");

    assert!(result.is_none(), "egress mismatch must mint no grant");
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("route.ambiguous.escalated")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner.notified")
            .unwrap(),
        1,
        "egress mismatch must reach the owner"
    );
}

/// The driver must persist the exact composed member that passed class
/// resolution. A live registry update after resolution must not change the
/// selected grant's authority.
#[tokio::test]
async fn selected_class_persists_composition_snapshot_across_registry_update() {
    let state = test_state();
    {
        let mut registry = state.registry.write();
        let mut duplicate = registry
            .routes
            .iter()
            .find(|route| route.id == "owner_telegram_main_assistant")
            .cloned()
            .expect("base owner route present in fixtures");
        duplicate.id = "owner_telegram_main_assistant_v2".to_string();
        registry.routes.push(duplicate);
    }
    let inputs = owner_inputs();
    let mut lane = owner_control_lane();
    lane.route_containment_guard = widen_selected_pack_after_resolution;
    let mut trace = Vec::new();
    let grant = run_pipeline(&state, lane, &inputs, Timestamp::now(), &mut trace)
        .await
        .expect("pipeline infrastructure must remain healthy")
        .expect("equivalent tie must select one composed snapshot");

    assert!(
        state
            .registry
            .read()
            .packs
            .get("owner_control_basic_pack")
            .unwrap()
            .allowed_egress_classes
            .contains(&EgressClass::Search),
        "route guard must prove the registry changed after class resolution"
    );
    assert!(
        grant.allowed_egress_classes.is_empty(),
        "returned grant must retain the pre-update composition snapshot"
    );
    let (persisted, _, _) = state
        .store
        .find_task_grant_by_id(grant.id)
        .unwrap()
        .expect("selected grant must be persisted");
    assert!(
        persisted.allowed_egress_classes.is_empty(),
        "persisted grant must retain the pre-update composition snapshot"
    );
}

/// If every tied route resolves to a pack that does not apply to the event,
/// the tie remains a silent non-match rather than an authority escalation.
#[tokio::test]
async fn tied_routes_with_no_applicable_pack_are_silent_non_match() {
    let state = test_state();
    {
        let mut registry = state.registry.write();
        let mut duplicate = registry
            .routes
            .iter()
            .find(|route| route.id == "owner_telegram_main_assistant")
            .cloned()
            .expect("base owner route present in fixtures");
        duplicate.id = "owner_telegram_main_assistant_v2".to_string();
        let pack_id = duplicate
            .capability_pack
            .clone()
            .expect("base owner route names a pack");
        registry.routes.push(duplicate);
        registry
            .packs
            .get_mut(&pack_id)
            .expect("base owner pack present")
            .applies_to
            .event_type = Some(EventType::EmailThreadSelected);
    }
    let inputs = owner_inputs();
    let mut trace = Vec::new();
    let result = run_pipeline(
        &state,
        owner_control_lane(),
        &inputs,
        Timestamp::now(),
        &mut trace,
    )
    .await
    .expect("non-applicable tie is a handled non-match");

    assert!(result.is_none());
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("route.ambiguous.not_applicable")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner.notified")
            .unwrap(),
        0,
        "a non-applicable tie must not notify the owner"
    );
}

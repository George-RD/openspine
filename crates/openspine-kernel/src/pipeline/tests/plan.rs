use super::*;
use crate::api::DispatchError;
use crate::telegram::TelegramConnector;
use openspine_schemas::action::{ActionId, ActionRequest};
use serde_json::json;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn proposed_plan_fixture() -> (AppState, MockServer, ActionRequest) {
    let telegram_server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {"message_id": 1, "date": 1, "chat": {"id": 555, "type": "private"}, "text": "ok"}
        })))
        .mount(&telegram_server)
        .await;
    let telegram = TelegramConnector::with_api_url(
        "test-token".to_string(),
        format!("{}/", telegram_server.uri()).parse().unwrap(),
    );
    let state = test_state_with_telegram(telegram);
    {
        let mut registry = state.registry.write();
        registry
            .routes
            .iter_mut()
            .find(|route| route.id == "owner_telegram_main_assistant")
            .unwrap()
            .capability_pack = Some("plan_approval_pack".to_string());
    }
    let grant = handle_owner_update(&state, &owner_update("plan proposal"))
        .await
        .unwrap()
        .expect("production plan pack must compose a grant");
    assert!(grant
        .allowed_actions
        .iter()
        .any(|action| action.as_str() == "plan.propose"));
    assert!(grant
        .approval_required_actions
        .iter()
        .any(|action| action.as_str() == "plan.execute"));
    let plan = openspine_schemas::plan::Plan {
        schema_version: 1,
        steps: vec![
            openspine_schemas::plan::PlanStep {
                action: ActionId::new("calendar.book"),
                arguments: json!({"time": "14:00"}),
                summary: "Book the meeting".to_string(),
            },
            openspine_schemas::plan::PlanStep {
                action: ActionId::new("data.scrub"),
                arguments: json!({"fields": ["ssn"]}),
                summary: "Scrub private data".to_string(),
            },
        ],
    };
    let result = crate::api::plan::dispatch_plan_preview(
        &state,
        &grant,
        &ActionId::new("plan.propose"),
        &crate::test_support::owner_surface_for(&state, 555),
        &plan,
    )
    .await
    .unwrap();
    assert_eq!(result["approval_offered"], true);
    let request = state.store.latest_action_request().unwrap().unwrap();
    let outbound = telegram_server.received_requests().await.unwrap();
    assert!(outbound.iter().any(|r| {
        r.body_json::<serde_json::Value>()
            .map(|body| {
                body.to_string()
                    .contains(&format!("approve_plan:{}", request.id))
            })
            .unwrap_or(false)
    }));
    (state, telegram_server, request)
}

#[tokio::test]
async fn plan_propose_approve_rederives_gate_and_resolves() {
    let (state, _telegram_server, request) = proposed_plan_fixture().await;
    assert_eq!(state.store.count_action_requests().unwrap(), 1);
    crate::pipeline::plan_approval::handle_plan_approval_callback(
        &state,
        &crate::test_support::owner_surface_for(&state, 555),
        "callback-id",
        request.id,
    )
    .await
    .unwrap();
    assert!(state
        .store
        .find_approval_for_request(request.id)
        .unwrap()
        .is_some());
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("plan.resolved")
            .unwrap(),
        1
    );
}

/// #202 E2E (spec #197 testing decision 7): the typed owner principal flows
/// unchanged from a *pipeline-composed* task grant, through `gate()`, into the
/// approval record and the audit actor — all tied to that one composed grant
/// id. Unlike `approval_actor_tests`, the grant here is genuinely composed by
/// `handle_owner_update` (inside `proposed_plan_fixture`), not a hand-built
/// fixture: this proves the composition seam wires owner -> grant.user ->
/// approved_by/actor for the same grant. (The Task-timer grant carries no wired
/// approval callback, so the owner-message plan-approval path is the real
/// composed-grant approval seam; the Task-object -> composed grant.user half is
/// covered by `task_board.rs`.)
#[tokio::test]
async fn typed_owner_principal_flows_from_composed_grant_through_gate_into_approval_and_audit() {
    let (state, _telegram_server, request) = proposed_plan_fixture().await;
    let owner = openspine_schemas::ids::PrincipalId::from(state.owner_principal_id);

    // The composed grant behind the request is issued to the owner principal
    // (AD-146), and the request is bound to that grant id.
    let (grant, _, _) = state
        .store
        .find_task_grant_by_id(request.task_grant_id)
        .unwrap()
        .expect("composed grant persisted");
    assert_eq!(
        grant.user, owner,
        "composed grant carries the owner PrincipalId (AD-146)"
    );
    assert_eq!(request.task_grant_id, grant.id);

    crate::pipeline::plan_approval::handle_plan_approval_callback(
        &state,
        &crate::test_support::owner_surface_for(&state, 555),
        "callback-id",
        request.id,
    )
    .await
    .unwrap();

    // ApprovalRecord: approver is the typed principal, tied to the request that
    // is bound to the composed grant (D-004, single-owner AD-146).
    let approval = state
        .store
        .find_approval_for_request(request.id)
        .unwrap()
        .expect("approval recorded");
    assert_eq!(
        approval.approved_by, owner,
        "approved_by is the verified owner principal (D-004)"
    );
    assert_eq!(
        approval.approved_by, grant.user,
        "the approver of record is the composed grant's principal (single owner)"
    );
    assert_eq!(approval.action_request_id, request.id);

    // Audit: the owner-authored approval event carries the actor AND is tied to
    // the composed grant id (D-003).
    let recorded = state
        .store
        .all_audit_event_jsons()
        .unwrap()
        .into_iter()
        .filter_map(|json| serde_json::from_str::<openspine_schemas::audit::AuditEvent>(&json).ok())
        .find(|event| event.kind.as_str() == "plan.approval_recorded")
        .expect("plan.approval_recorded audit event exists");
    assert_eq!(
        recorded.actor,
        Some(owner),
        "audit actor is the verified owner principal (D-003)"
    );
    assert_eq!(
        recorded.task_grant_id,
        Some(grant.id),
        "the approval audit event is tied to the composed grant id"
    );
}

#[tokio::test]
async fn tampered_plan_artifact_is_refused_at_approval_callback() {
    let (state, _telegram_server, request) = proposed_plan_fixture().await;
    let payload_ref = request.payload_ref.as_ref().unwrap();
    state
        .artifacts
        .put_tampered_for_test(&payload_ref.digest, b"tampered plan payload")
        .unwrap();
    crate::pipeline::plan_approval::handle_plan_approval_callback(
        &state,
        &crate::test_support::owner_surface_for(&state, 555),
        "callback-id",
        request.id,
    )
    .await
    .unwrap();
    assert!(state
        .store
        .find_approval_for_request(request.id)
        .unwrap()
        .is_none());
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("plan.resolved")
            .unwrap(),
        0
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("plan.approval_digest_mismatch")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn plan_proposal_budget_exhaustion_persists_no_request() {
    let state = test_state();
    let mut grant = super::approval::approval_fixture_grant();
    grant.limits.max_artifacts = 0;
    grant.approval_required_actions = vec![ActionId::new("plan.execute")];
    grant.allowed_actions = vec![ActionId::new("plan.execute")];
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    let pending = state.artifacts.put(b"pending").unwrap();
    state
        .store
        .insert_task_grant(
            &grant,
            &pending,
            &crate::test_support::owner_surface_for(&state, 555),
        )
        .unwrap();
    let plan = openspine_schemas::plan::Plan {
        schema_version: 1,
        steps: vec![openspine_schemas::plan::PlanStep {
            action: ActionId::new("calendar.book"),
            arguments: json!({"time": "14:00"}),
            summary: "Book the meeting".to_string(),
        }],
    };
    let result = crate::api::plan::dispatch_plan_preview(
        &state,
        &grant,
        &ActionId::new("plan.propose"),
        &crate::test_support::owner_surface_for(&state, 555),
        &plan,
    )
    .await;
    assert!(result.is_err());
    assert_eq!(state.store.count_action_requests().unwrap(), 0);
}

#[tokio::test]
async fn plan_preview_records_telegram_success_counter() {
    let (state, _telegram_server, _request) = proposed_plan_fixture().await;
    assert_eq!(
        state
            .store
            .connector_counter("telegram", "success")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn plan_preview_records_telegram_failure_counter_on_send_error() {
    let telegram_server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&telegram_server)
        .await;
    let telegram = TelegramConnector::with_api_url(
        "test-token".to_string(),
        format!("{}/", telegram_server.uri()).parse().unwrap(),
    );
    let state = test_state_with_telegram(telegram);
    {
        let mut registry = state.registry.write();
        registry
            .routes
            .iter_mut()
            .find(|route| route.id == "owner_telegram_main_assistant")
            .unwrap()
            .capability_pack = Some("plan_approval_pack".to_string());
    }
    let grant = handle_owner_update(&state, &owner_update("plan proposal"))
        .await
        .unwrap()
        .expect("production plan pack must compose a grant");
    let plan = openspine_schemas::plan::Plan {
        schema_version: 1,
        steps: vec![openspine_schemas::plan::PlanStep {
            action: ActionId::new("calendar.book"),
            arguments: json!({"time": "14:00"}),
            summary: "Book the meeting".to_string(),
        }],
    };
    let result = crate::api::plan::dispatch_plan_preview(
        &state,
        &grant,
        &ActionId::new("plan.propose"),
        &crate::test_support::owner_surface_for(&state, 555),
        &plan,
    )
    .await;
    assert!(
        matches!(result, Err(DispatchError::Connector(_))),
        "plan preview must classify Telegram send failure as Connector: {result:?}"
    );
    assert_eq!(
        state
            .store
            .connector_counter("telegram", "failure")
            .unwrap(),
        1
    );
}

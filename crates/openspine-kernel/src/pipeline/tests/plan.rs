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
        555,
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
        555,
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
        555,
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
        .insert_task_grant(&grant, &pending, 555)
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
        555,
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
        555,
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

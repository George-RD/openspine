//! End-to-end tests for the kernel's HTTP API (`POST /v1/actions`).
//!
//! Every test here builds a real [`AppState`] backed by the actual Lyra
//! fixtures and the in-memory SQLite store, runs the full Telegram-owner
//! pipeline to mint a task grant, and then exercises the axum router over a
//! real bound TCP port. The goal is to prove behavior that unit tests against
//! the pure gate function cannot: that the wire contract rejects overrides,
//! that the dispatcher sends Telegram replies to the *grant-bound* chat, and
//! that approval-required actions stop before any effect runs.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::serve;
use reqwest::Response;
use serde_json::{json, Value};
use tokio::task::JoinHandle;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::api::router;
use crate::pipeline::handle_owner_update;
use crate::pipeline::AppState;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::*;

pub(crate) async fn start_server(state: AppState) -> (SocketAddr, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(Arc::new(state));
    let handle = tokio::spawn(async move { serve(listener, app).await.unwrap() });
    (addr, handle)
}

pub(crate) async fn post_action(
    addr: SocketAddr,
    token: &str,
    action: &str,
    payload: Option<Value>,
) -> Response {
    let client = reqwest::Client::new();
    let mut body = json!({ "action": action });
    if let Some(p) = payload {
        body["payload"] = p;
    }
    client
        .post(format!("http://{}/v1/actions", addr))
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn email_read_inbox_is_denied_for_owner_control_grant() {
    let state = test_state();
    let grant = handle_owner_update(&state, &owner_update("check inbox"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    let (addr, handle) = start_server(state).await;

    let resp = post_action(addr, &grant.task_token, "email.read_inbox", None).await;
    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "deny");
    assert_eq!(body["decision"]["reason"], "explicit_deny");
    assert!(body.get("result").is_none());

    handle.abort();
    assert!(body.get("counterparty_deferral").is_none());
}

#[tokio::test]
async fn network_raw_egress_is_denied_for_owner_control_grant() {
    let state = test_state();
    let grant = handle_owner_update(&state, &owner_update("reach out"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    let (addr, handle) = start_server(state).await;

    let resp = post_action(addr, &grant.task_token, "network.raw_egress", None).await;
    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "deny");
    assert_eq!(body["decision"]["reason"], "explicit_deny");
    assert!(body.get("result").is_none());

    handle.abort();
}

#[tokio::test]
async fn counterparty_denial_returns_deferral_routes_and_audits() {
    let server = MockServer::start().await;
    let token = "test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{}/SendMessage", token)))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 99,
                "date": 0,
                "chat": {"id": 555, "type": "private"},
                "text": "sent"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let connector =
        TelegramConnector::with_api_url(token.to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);
    let store = state.store.clone();
    let grant = handle_owner_update(&state, &owner_update("send this"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    let (addr, handle) = start_server(state).await;

    let sentinel = "RULE_SENTINEL_POLICY_TEXT";
    let resp = post_action(
        addr,
        &grant.task_token,
        "email.send",
        Some(json!({"context": sentinel})),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(
        body,
        json!({
            "decision": {"outcome": "deny", "reason": "explicit_deny"},
            "counterparty_deferral": "I need to check on that — I'll get back to you"
        })
    );
    assert!(!body.to_string().contains(sentinel));
    assert!(!body.to_string().contains("policy"));
    assert!(!body.to_string().contains("EscalationNotice"));
    assert_eq!(
        store
            .count_audit_events_of_kind("action.escalated")
            .unwrap(),
        1
    );
    let escalated = store
        .all_audit_event_jsons()
        .unwrap()
        .into_iter()
        .map(|json| serde_json::from_str::<Value>(&json).unwrap())
        .find(|event| event["kind"] == "action.escalated")
        .expect("escalation audit row");
    assert_eq!(
        escalated["decision"],
        json!({"outcome": "deny", "reason": "explicit_deny"})
    );
    assert_eq!(escalated["task_grant_id"], grant.id.to_string());
    assert_eq!(escalated["reason"], "explicit_deny");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let request_body: Value = requests[0].body_json().unwrap();
    assert_eq!(request_body["chat_id"], 555);
    assert!(request_body["text"]
        .as_str()
        .unwrap()
        .contains("email.send"));
    assert!(request_body["text"]
        .as_str()
        .unwrap()
        .contains("explicit_deny"));

    handle.abort();
}

#[tokio::test]
async fn counterparty_escalation_failure_is_not_reported_as_success() {
    let server = MockServer::start().await;
    let token = "test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{}/SendMessage", token)))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "ok": false,
            "description": "telegram unavailable"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let connector =
        TelegramConnector::with_api_url(token.to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);
    let store = state.store.clone();
    let grant = handle_owner_update(&state, &owner_update("send this"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    let (addr, handle) = start_server(state).await;

    let resp = post_action(addr, &grant.task_token, "email.send", None).await;
    assert_eq!(resp.status(), 500);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "internal_error");
    assert_eq!(
        store
            .count_audit_events_of_kind("owner.notify_failed")
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("action.escalated")
            .unwrap(),
        0
    );

    handle.abort();
}

#[tokio::test]
async fn host_filesystem_read_and_write_are_denied_for_owner_control_grant() {
    let state = test_state();
    let grant = handle_owner_update(&state, &owner_update("touch host"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    let (addr, handle) = start_server(state).await;

    for action in ["filesystem.host_read", "filesystem.host_write"] {
        let resp = post_action(addr, &grant.task_token, action, None).await;
        assert_eq!(resp.status(), 200, "{} should be gated, not fail", action);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["decision"]["outcome"], "deny", "{}", action);
        assert_eq!(body["decision"]["reason"], "explicit_deny", "{}", action);
        assert!(body.get("result").is_none(), "{}", action);
    }

    handle.abort();
}

#[tokio::test]
async fn telegram_reply_is_sent_to_grant_bound_chat() {
    let server = MockServer::start().await;
    let token = "test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{}/SendMessage", token)))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": 555, "type": "private"},
                "text": "sent"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let connector =
        TelegramConnector::with_api_url(token.to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);
    let grant = handle_owner_update(&state, &owner_update("hello"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &grant.task_token,
        "telegram.reply:owner_channel",
        Some(json!({"text": "hello owner"})),
    )
    .await;
    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "allow");
    assert_eq!(body["result"]["sent"], true);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let request_body: Value = requests[0].body_json().unwrap();
    assert_eq!(request_body["chat_id"], 555);
    assert_eq!(request_body["text"], "hello owner");

    handle.abort();
}

#[tokio::test]
async fn telegram_reply_payload_rejects_chat_id_override() {
    let server = MockServer::start().await;
    let token = "test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{}/SendMessage", token)))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 2,
                "date": 0,
                "chat": {"id": 555, "type": "private"},
                "text": "sent"
            }
        })))
        .expect(0)
        .mount(&server)
        .await;

    let connector =
        TelegramConnector::with_api_url(token.to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);
    let grant = handle_owner_update(&state, &owner_update("hello"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &grant.task_token,
        "telegram.reply:owner_channel",
        Some(json!({"text": "hello", "chat_id": 999})),
    )
    .await;
    assert_eq!(resp.status(), 400);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 0);

    handle.abort();
}

/// D-068 scopes bad-request digest suppression to the authenticated API's
/// direct response boundary. A detached caller (durable workflow adapter)
/// has no direct response surface, so the same bad request MUST enter the
/// failure digest; the direct API caller MUST NOT duplicate it there.
#[tokio::test]
async fn bad_request_batches_for_detached_surface_only() {
    use crate::api::actions::{mediate_and_dispatch_action, FailureSurface};
    use openspine_schemas::action::ActionId;

    let state = test_state();
    let grant = handle_owner_update(&state, &owner_update("hello"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    let payload = json!({"text": "hello", "chat_id": 999});
    let baseline = state
        .store
        .count_audit_events_of_kind("failure.digest_batched")
        .unwrap();

    let direct = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("telegram.reply:owner_channel"),
        crate::api::dispatch_tests::OWNER_CHAT_ID,
        Some(&payload),
        FailureSurface::DirectResponse,
        None,
    )
    .await;
    assert!(matches!(
        direct,
        Err(crate::api::actions::DispatchError::BadRequest(_))
    ));
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("failure.digest_batched")
            .unwrap(),
        baseline,
        "direct API bad request must not be duplicated into the digest"
    );

    let detached = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("telegram.reply:owner_channel"),
        crate::api::dispatch_tests::OWNER_CHAT_ID,
        Some(&payload),
        FailureSurface::Detached,
        None,
    )
    .await;
    assert!(matches!(
        detached,
        Err(crate::api::actions::DispatchError::BadRequest(_))
    ));
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("failure.digest_batched")
            .unwrap(),
        baseline + 1,
        "detached bad request must enter the failure digest"
    );
}

#[tokio::test]
async fn approval_required_action_stops_before_dispatch() {
    let state = test_state();
    let grant = handle_owner_update(&state, &owner_update("enable something"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    let (addr, handle) = start_server(state).await;

    let resp = post_action(addr, &grant.task_token, "connector.enable", None).await;
    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "approval_required");
    assert_eq!(body["decision"]["approval_type"], "connector.enable");
    assert!(body.get("result").is_none());
    // The response body must not contain a stub "result" — if it did, the
    // dispatcher would have run the action before gate() returned Allow.
    assert!(!body.to_string().contains("stub"));

    handle.abort();
}

#[tokio::test]
async fn model_swap_propose_reaches_http_gate_and_audits_failure() {
    let state = test_state();
    let store = state.store.clone();
    let grant = handle_owner_update(&state, &owner_update("propose a model swap"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    let (addr, handle) = start_server(state).await;
    let response = post_action(
        addr,
        &grant.task_token,
        "artifact.propose",
        Some(json!({
            "kind": "model_swap",
            "yaml": "id: base\nversion: 1\nlifecycle_state: proposed\nrole: base\n",
        })),
    )
    .await;
    assert_eq!(response.status(), 400);
    assert_eq!(
        store
            .count_audit_events_of_kind("action.dispatch_failed")
            .unwrap(),
        1
    );
    handle.abort();
}

#[tokio::test]
async fn spend_dispatch_denial_precedes_connector_effect() {
    use crate::api::actions::{mediate_and_dispatch_action, FailureSurface};
    use crate::config::SpendCapConfig;
    use openspine_schemas::action::ActionId;

    let telegram_server = MockServer::start().await;
    let mut state =
        crate::test_support::fixtures::test_state_with_telegram(TelegramConnector::with_api_url(
            "test-token".to_string(),
            telegram_server.uri().parse().unwrap(),
        ));
    state.spend_cap = SpendCapConfig {
        model_calls_per_day: i64::MAX as u64,
        connector_calls_per_day: 0,
    };
    let mut grant = handle_owner_update(&state, &owner_update("hello"))
        .await
        .unwrap()
        .expect("owner grant");
    grant.workflow_id = "scheduled_internal".into();
    let result = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("telegram.reply:owner_channel"),
        crate::api::dispatch_tests::OWNER_CHAT_ID,
        Some(&json!({"text": "hello"})),
        FailureSurface::DirectResponse,
        None,
    )
    .await;
    assert!(matches!(
        result,
        Err(crate::api::actions::DispatchError::Resource(_))
    ));
    let requests = telegram_server.received_requests().await.unwrap();
    assert!(requests.iter().all(|request| {
        let body = request.body_json::<Value>().expect("Telegram request JSON");
        body["text"] != "hello"
    }));
}

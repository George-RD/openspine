//! Tests for the kernel HTTP client. Split out of `client.rs` to keep that
//! file under the repo's 500-line-per-file gate.

use super::KernelClient;
use openspine_schemas::action::{DenialReason, GateDecision};
use serde_json::{json, Value};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn task_view_json() -> Value {
    json!({
        "task_grant_id": "01J000000000000000",
        "agent_id": "main_assistant_agent",
        "workflow_id": "owner_control_conversation",
        "purpose": "owner_control_conversation",
        "allowed_actions": ["openspine.status.read", "telegram.reply:owner_channel"],
        "approval_required_actions": [],
        "denied_actions": ["email.read_inbox"],
        "is_worker": false,
        "output_channels": ["telegram.owner.reply"],
        "limits": {
            "max_model_calls": 8,
            "max_artifacts": 20,
            "max_runtime_seconds": 120
        },
        "expires_at": "2099-01-01T00:00:00Z",
        "pending_message": "hello",
        "selection_tokens": ["01J2RMVP6J4HJHKV0W2L7M3C6Q", "01J2RMVP6J4HJHKV0W2M7N4D7R"]
    })
}

/// (d) `GET /v1/task` carries `Authorization: Bearer <token>`.
#[tokio::test]
async fn get_task_sends_bearer_auth() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/task"))
        .and(header("Authorization", "Bearer secret-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_view_json()))
        .expect(1)
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), "secret-token".to_string());
    let view = client
        .get_task()
        .await
        .expect("should succeed with matching bearer");
    assert_eq!(view.agent_id, "main_assistant_agent");
    assert_eq!(view.limits.max_model_calls, 8);
    assert!(
        !view.is_worker,
        "root fixture must deserialize as non-worker"
    );
}

/// (Step 5) `GET /v1/task` deserializes the `selection_tokens` array bound to this grant.
#[tokio::test]
async fn get_task_deserializes_selection_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/task"))
        .and(header("Authorization", "Bearer secret-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_view_json()))
        .expect(1)
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), "secret-token".to_string());
    let view = client
        .get_task()
        .await
        .expect("should deserialize task view");
    assert_eq!(view.selection_tokens.len(), 2);
    assert_eq!(view.selection_tokens[0], "01J2RMVP6J4HJHKV0W2L7M3C6Q");
    assert_eq!(view.selection_tokens[1], "01J2RMVP6J4HJHKV0W2M7N4D7R");
}

/// (d) `POST /v1/actions` also carries the Bearer token.
#[tokio::test]
async fn submit_action_sends_bearer_auth() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .and(header("Authorization", "Bearer my-task-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "decision": {"outcome": "allow"},
            "result": {"status": "ok"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), "my-task-token".to_string());
    let outcome = client
        .submit_action("openspine.status.read", None, None)
        .await
        .expect("should succeed");
    assert_eq!(outcome.decision, GateDecision::Allow);
}

/// (d) `POST /v1/model/generate` also carries the Bearer token.
#[tokio::test]
async fn generate_sends_bearer_auth() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/model/generate"))
        .and(header("Authorization", "Bearer gen-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "decision": {"outcome": "allow"},
            "text": "hello from model"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), "gen-token".to_string());
    let outcome = client
        .generate("reply_to_owner", "hi", None, 12_000)
        .await
        .expect("should succeed");
    assert_eq!(outcome.decision, GateDecision::Allow);
    assert_eq!(outcome.text.as_deref(), Some("hello from model"));
}

/// (Step 5) `POST /v1/model/generate` includes `untrusted_context` in the JSON body.
#[tokio::test]
async fn generate_sends_untrusted_context_in_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/model/generate"))
        .and(header("Authorization", "Bearer gen-token"))
        .and(body_json(json!({
            "purpose": "reply_to_owner",
            "user_message": "hi",
            "untrusted_context": "some untrusted text",
            "max_tokens": 12_000
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "decision": {"outcome": "allow"},
            "text": "hello from model"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), "gen-token".to_string());
    let outcome = client
        .generate("reply_to_owner", "hi", Some("some untrusted text"), 12_000)
        .await
        .expect("should succeed");
    assert_eq!(outcome.decision, GateDecision::Allow);
    assert_eq!(outcome.text.as_deref(), Some("hello from model"));
}

/// (c) A `deny` response from `/v1/actions` is `Ok(ActionOutcome)`, never `Err`.
#[tokio::test]
async fn deny_decision_is_ok_not_err() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "decision": {"outcome": "deny", "reason": "explicit_deny"}
        })))
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), "t".to_string());
    let outcome = client
        .submit_action("email.read_inbox", None, None)
        .await
        .expect("deny is not a transport error — it is Ok at the HTTP layer");
    assert_eq!(
        outcome.decision,
        GateDecision::Deny {
            reason: DenialReason::ExplicitDeny
        }
    );
    assert!(outcome.counterparty_deferral.is_none());
    assert!(outcome.result.is_none());
}

#[tokio::test]
async fn counterparty_denial_preserves_only_canonical_deferral() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "decision": {"outcome": "deny", "reason": "explicit_deny"},
            "counterparty_deferral": "I need to check on that — I'll get back to you"
        })))
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), "t".to_string());
    let outcome = client
        .submit_action("email.send", None, None)
        .await
        .expect("counterparty denial is a structured outcome");
    assert_eq!(
        outcome.counterparty_deferral.as_deref(),
        Some("I need to check on that — I'll get back to you")
    );
    assert!(outcome.result.is_none());
}

/// (c) `approval_required` outcome is similarly `Ok`, not `Err`.
#[tokio::test]
async fn approval_required_is_ok_not_err() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "decision": {
                "outcome": "approval_required",
                "approval_type": "email.create_draft"
            }
        })))
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), "t".to_string());
    let outcome = client
        .submit_action("email.create_draft", None, None)
        .await
        .expect("approval_required is not an error");
    assert!(matches!(
        outcome.decision,
        GateDecision::ApprovalRequired { .. }
    ));
}

/// A `403` response from `GET /v1/task` returns `Err` (shell must exit non-zero).
#[tokio::test]
async fn unauthorized_get_task_returns_err() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/task"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"error": "unauthorized"})))
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), "bad-token".to_string());
    let result = client.get_task().await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("403"),
        "error message should mention 403: {msg}"
    );
}

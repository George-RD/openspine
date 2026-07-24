use super::*;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Shared task token used across tests.
const TOKEN: &str = "test-task-token";

fn allow_result(result: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "decision": {"outcome": "allow"},
        "result": result
    }))
}

fn allow_reply() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "decision": {"outcome": "allow"},
        "result": {"sent": true}
    }))
}

fn deny_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "decision": {"outcome": "deny", "reason": "explicit_deny"}
    }))
}

/// (a) `/status` submits `openspine.status.read` then replies via
/// `telegram.reply:owner_channel` with the result JSON.
#[tokio::test]
async fn status_command_submits_correct_action_then_replies() {
    let server = MockServer::start().await;

    // Expect status.read action with exact body
    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .and(header("Authorization", format!("Bearer {TOKEN}")))
        .and(body_json(serde_json::json!({
            "action": "openspine.status.read",
            "payload": null,
            "target": null
        })))
        .respond_with(allow_result(serde_json::json!({"status": "ok"})))
        .expect(1)
        .mount(&server)
        .await;

    // Expect the reply action (body contains telegram.reply action)
    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .and(header("Authorization", format!("Bearer {TOKEN}")))
        .and(body_json(serde_json::json!({
            "action": "telegram.reply:owner_channel",
            "payload": {"text": "{\n  \"status\": \"ok\"\n}"},
            "target": null
        })))
        .respond_with(allow_reply())
        .expect(1)
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), TOKEN.to_string());
    run(&client, "/status").await.expect("should succeed");
}

/// (b) Unknown text triggers `POST /v1/model/generate` then
/// `telegram.reply:owner_channel` with the model's text as payload.
#[tokio::test]
async fn freeform_calls_generate_then_replies_with_model_text() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/model/generate"))
        .and(header("Authorization", format!("Bearer {TOKEN}")))
        .and(body_json(serde_json::json!({
            "purpose": MODEL_PURPOSE,
            "user_message": "hello lyra",
            "max_tokens": MAX_TOKENS
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "decision": {"outcome": "allow"},
            "text": "Hello! How can I help?"
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .and(header("Authorization", format!("Bearer {TOKEN}")))
        .and(body_json(serde_json::json!({
            "action": "telegram.reply:owner_channel",
            "payload": {"text": "Hello! How can I help?"},
            "target": null
        })))
        .respond_with(allow_reply())
        .expect(1)
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), TOKEN.to_string());
    run(&client, "hello lyra").await.expect("should succeed");
}

/// (c) A `deny` gate outcome on the initiating action does NOT crash the
/// shell — it returns `Ok(())` and does NOT attempt the reply action.
#[tokio::test]
async fn deny_on_primary_action_exits_ok_no_reply_attempt() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .respond_with(deny_response())
        .expect(1) // only the primary action; no second call for reply
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), TOKEN.to_string());
    // Must return Ok — a deny is not a shell crash
    run(&client, "/status")
        .await
        .expect("deny outcome must not cause Err");
    // wiremock verifies expect(1) on drop — a second call would cause a mismatch
}

/// (c) Same guarantee for `approval_required`.
#[tokio::test]
async fn approval_required_on_primary_action_exits_ok_no_reply() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "decision": {
                "outcome": "approval_required",
                "approval_type": "setup.workflow.start"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), TOKEN.to_string());
    run(&client, "/setup")
        .await
        .expect("approval_required must not cause Err");
}

/// `/propose <kind>\n<yaml>` posts `artifact.propose` with
/// `{"kind": kind, "yaml": yaml}`.
#[tokio::test]
async fn propose_command_sends_correct_payload() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .and(body_json(serde_json::json!({
            "action": "artifact.propose",
            "payload": {"kind": "route", "yaml": "id: dark_mode_route\nversion: 1"},
            "target": null
        })))
        .respond_with(allow_result(serde_json::json!({
            "proposed": true,
            "action_request_id": "01JZZZZZZZZZZZZZZZZZZZZZZZ"
        })))
        .expect(1)
        .mount(&server)
        .await;

    // The reply mock (any body) — just needs to not error
    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .respond_with(allow_reply())
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), TOKEN.to_string());
    run(&client, "/propose route\nid: dark_mode_route\nversion: 1")
        .await
        .expect("propose should succeed");
}

/// A missing kind, or a body that is empty (or all whitespace) after
/// the kind line, never reaches the kernel — only the usage-text reply
/// is sent. Exercises both halves of `cmd_propose`'s
/// `kind.is_empty() || yaml.trim().is_empty()` guard independently, so
/// a regression that dropped either half would still redden this test.
#[tokio::test]
async fn propose_without_body_replies_usage_and_calls_nothing() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .and(body_json(serde_json::json!({
            "action": "telegram.reply:owner_channel",
            "payload": {"text": "Usage: /propose <route|agent|workflow|pack|policy>\n<yaml>"},
            "target": null
        })))
        .respond_with(allow_reply())
        .expect(2)
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), TOKEN.to_string());
    // Kind present ("route") but no YAML body on a second line —
    // exercises `yaml.trim().is_empty()`.
    run(&client, "/propose route")
        .await
        .expect("missing body must not cause Err");
    // An empty first line (no kind) with a non-empty body on the
    // second — exercises `kind.is_empty()` in isolation, proving the
    // guard does not rely solely on the body being empty too.
    run(&client, "/propose \nid: x\nversion: 1")
        .await
        .expect("missing kind must not cause Err");
    // wiremock's `.expect(2)` on the exact reply body above is verified
    // on drop — an `artifact.propose` call would 500 (unmocked) and
    // fail this test outright, and a differently-shaped reply would
    // fail the exact `body_json` match.
}

/// A `deny` on the MODEL generate call also exits `Ok(())`.
#[tokio::test]
async fn deny_on_model_generate_exits_ok() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/model/generate"))
        .respond_with(deny_response())
        .expect(1)
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), TOKEN.to_string());
    run(&client, "what is the weather")
        .await
        .expect("model deny must not cause Err");
}

#[tokio::test]
async fn export_command_sends_exact_payload() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .and(body_json(serde_json::json!({
            "action": "openspine.overlay.export",
            "payload": {"bundle_name": "nightly-1"},
            "target": null
        })))
        .respond_with(allow_result(serde_json::json!({
            "restart_required": true
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .respond_with(allow_reply())
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), TOKEN.to_string());
    run(&client, "/export nightly-1")
        .await
        .expect("export should succeed");
}

#[tokio::test]
async fn restore_command_sends_exact_payload() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .and(body_json(serde_json::json!({
            "action": "openspine.overlay.restore",
            "payload": {"bundle_name": "nightly-1"},
            "target": null
        })))
        .respond_with(allow_result(serde_json::json!({
            "restart_required": true
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .respond_with(allow_reply())
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), TOKEN.to_string());
    run(&client, "/restore nightly-1")
        .await
        .expect("restore should succeed");
}

#[tokio::test]
async fn export_without_name_replies_usage_and_calls_nothing() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .and(body_json(serde_json::json!({
            "action": "telegram.reply:owner_channel",
            "payload": {"text": "Usage: /export <bundle-name>"},
            "target": null
        })))
        .respond_with(allow_reply())
        .expect(1)
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), TOKEN.to_string());
    run(&client, "/export")
        .await
        .expect("missing name must not cause Err");
}

#[tokio::test]
async fn export_with_extra_args_replies_usage() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .and(body_json(serde_json::json!({
            "action": "telegram.reply:owner_channel",
            "payload": {"text": "Usage: /export <bundle-name>"},
            "target": null
        })))
        .respond_with(allow_reply())
        .expect(1)
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), TOKEN.to_string());
    run(&client, "/export nightly-1 extra")
        .await
        .expect("extra args must not cause Err");
}

#[tokio::test]
async fn restore_without_name_replies_usage() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/actions"))
        .and(body_json(serde_json::json!({
            "action": "telegram.reply:owner_channel",
            "payload": {"text": "Usage: /restore <bundle-name>"},
            "target": null
        })))
        .respond_with(allow_reply())
        .expect(1)
        .mount(&server)
        .await;

    let client = KernelClient::new(server.uri(), TOKEN.to_string());
    run(&client, "/restore")
        .await
        .expect("missing restore name must not cause Err");
}

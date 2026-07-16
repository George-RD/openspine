//! `main_assistant_agent` — deterministic command layer for Phase 1.
//!
//! Routes owner messages through a priority-ordered command dispatch before
//! falling back to a kernel-mediated model call. Every external effect goes
//! through `POST /v1/actions` on the kernel; the shell has no other I/O.
//!
//! Command dispatch (exact-prefix match, evaluated in order):
//!   `/status`      → action `openspine.status.read`
//!   `/setup`       → action `setup.workflow.start`  (stub)
//!   `/propose <kind>\n<yaml>` → action `artifact.propose` (implemented)
//!   (anything else)→ `POST /v1/model/generate`, then reply with model text

use crate::client::{KernelClient, ModelOutcome};
use anyhow::Result;
use openspine_schemas::action::GateDecision;
use serde_json::{json, Value};

const REPLY_ACTION: &str = "telegram.reply:owner_channel";
const MODEL_PURPOSE: &str = "reply_to_owner";
const MAX_TOKENS: u32 = 12_000;

// ── Public entry point ────────────────────────────────────────────────────────

/// Dispatch `message` through the deterministic command layer.
///
/// Returns `Ok(())` on success **or** on a gate deny/approval-required
/// outcome — those are logged and the shell exits 0 (the kernel already
/// recorded the audit row).  Only transport / `5xx` errors propagate as `Err`.
pub async fn run(client: &KernelClient, message: &str) -> Result<()> {
    if message == "/status" {
        return cmd_status(client).await;
    }
    if message == "/setup" {
        return cmd_setup(client).await;
    }
    if let Some(proposal) = message.strip_prefix("/propose ") {
        return cmd_propose(client, proposal).await;
    }
    cmd_freeform(client, message).await
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Send `telegram.reply:owner_channel` carrying `text`.
///
/// A deny or approval-required outcome on the reply itself is logged but
/// does not return an `Err` — the gate has already recorded it.
async fn send_reply(client: &KernelClient, text: &str) -> Result<()> {
    let payload = json!({ "text": text });
    let outcome = client
        .submit_action(REPLY_ACTION, Some(payload), None)
        .await?;
    match outcome.decision {
        GateDecision::Allow => {
            eprintln!("[openspine-shell] INFO: telegram reply sent");
        }
        GateDecision::Deny { reason } => {
            eprintln!("[openspine-shell] WARN: telegram reply denied: {reason:?}");
        }
        GateDecision::ApprovalRequired { ref approval_type } => {
            eprintln!("[openspine-shell] WARN: telegram reply needs approval: {approval_type}");
        }
        GateDecision::EffectSuppressed => {
            eprintln!("[openspine-shell] WARN: telegram reply effect suppressed");
        }
    }
    Ok(())
}

// ── Command handlers ──────────────────────────────────────────────────────────

async fn cmd_status(client: &KernelClient) -> Result<()> {
    let outcome = client
        .submit_action("openspine.status.read", None, None)
        .await?;
    match outcome.decision {
        GateDecision::Allow => {
            let result: Value = outcome.result.unwrap_or(json!({"status": "ok"}));
            let text =
                serde_json::to_string_pretty(&result).context("status result serialization")?;
            send_reply(client, &text).await
        }
        GateDecision::Deny { reason } => {
            eprintln!("[openspine-shell] WARN: openspine.status.read denied: {reason:?}");
            Ok(())
        }
        GateDecision::ApprovalRequired { ref approval_type } => {
            eprintln!(
                "[openspine-shell] WARN: openspine.status.read requires approval: {approval_type}"
            );
            Ok(())
        }
        GateDecision::EffectSuppressed => {
            eprintln!("[openspine-shell] WARN: status effect suppressed");
            Ok(())
        }
    }
}

async fn cmd_setup(client: &KernelClient) -> Result<()> {
    let outcome = client
        .submit_action("setup.workflow.start", None, None)
        .await?;
    match outcome.decision {
        GateDecision::Allow => {
            let text = stub_note(&outcome.result, "Setup workflow started.");
            send_reply(client, &text).await
        }
        GateDecision::Deny { reason } => {
            eprintln!("[openspine-shell] WARN: setup.workflow.start denied: {reason:?}");
            Ok(())
        }
        GateDecision::ApprovalRequired { ref approval_type } => {
            eprintln!(
                "[openspine-shell] WARN: setup.workflow.start requires approval: {approval_type}"
            );
            Ok(())
        }
        GateDecision::EffectSuppressed => {
            eprintln!("[openspine-shell] WARN: setup workflow effect suppressed");
            Ok(())
        }
    }
}

/// `/propose <kind>` on the first line, YAML as the remainder (5f). No
/// client-side kind validation beyond non-empty — the kernel owns it
/// (`artifact.propose`'s payload contract). A missing kind or empty body
/// never reaches the kernel at all.
async fn cmd_propose(client: &KernelClient, proposal_text: &str) -> Result<()> {
    let (kind, yaml) = match proposal_text.split_once('\n') {
        Some((kind, yaml)) => (kind.trim(), yaml),
        None => (proposal_text.trim(), ""),
    };
    if kind.is_empty() || yaml.trim().is_empty() {
        return send_reply(
            client,
            "Usage: /propose <route|agent|workflow|pack|policy>\n<yaml>",
        )
        .await;
    }
    let payload = json!({ "kind": kind, "yaml": yaml });
    let outcome = client
        .submit_action("artifact.propose", Some(payload), None)
        .await?;
    match outcome.decision {
        GateDecision::Allow => {
            let text = stub_note(&outcome.result, "Proposal submitted for review.");
            send_reply(client, &text).await
        }
        GateDecision::Deny { reason } => {
            eprintln!("[openspine-shell] WARN: artifact.propose denied: {reason:?}");
            Ok(())
        }
        GateDecision::ApprovalRequired { ref approval_type } => {
            eprintln!(
                "[openspine-shell] WARN: artifact.propose requires approval: {approval_type}"
            );
            Ok(())
        }
        GateDecision::EffectSuppressed => {
            eprintln!("[openspine-shell] WARN: artifact proposal effect suppressed");
            Ok(())
        }
    }
}

async fn cmd_freeform(client: &KernelClient, message: &str) -> Result<()> {
    let model_outcome: ModelOutcome = client
        .generate(MODEL_PURPOSE, message, None, MAX_TOKENS)
        .await?;
    match model_outcome.decision {
        GateDecision::Allow => {
            let text = model_outcome.text.unwrap_or_default();
            if text.is_empty() {
                eprintln!("[openspine-shell] WARN: model returned empty text; skipping reply");
                return Ok(());
            }
            send_reply(client, &text).await
        }
        GateDecision::Deny { reason } => {
            eprintln!("[openspine-shell] WARN: model.generate denied: {reason:?}");
            Ok(())
        }
        GateDecision::ApprovalRequired { ref approval_type } => {
            eprintln!("[openspine-shell] WARN: model.generate requires approval: {approval_type}");
            Ok(())
        }
        GateDecision::EffectSuppressed => {
            eprintln!("[openspine-shell] WARN: model.generate effect suppressed");
            Ok(())
        }
    }
}

/// Extract the `note` string from a stub response, or fall back to `default`.
fn stub_note(result: &Option<Value>, default: &str) -> String {
    result
        .as_ref()
        .and_then(|r| r.get("note"))
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_string()
}

// ── Context import needed in tests ───────────────────────────────────────────
use anyhow::Context as _;

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
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
}

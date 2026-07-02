//! HTTP client for the kernel API.
//!
//! The shell holds exactly one `TASK_TOKEN` and sends it as a Bearer
//! credential on every request. All effectful actions go through
//! `POST /v1/actions`; the shell has no other network I/O.

use anyhow::{bail, Context, Result};
use openspine_schemas::action::GateDecision;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Response DTOs ─────────────────────────────────────────────────────────────

/// Resource limits carried in the task-grant view. All fields reflect the
/// wire contract; some are not consumed in Phase 1 but must be parsed.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct TaskLimits {
    pub max_model_calls: u32,
    pub max_artifacts: u32,
    pub max_runtime_seconds: u32,
}

/// Redacted view of the calling task grant — returned by `GET /v1/task`.
/// Never includes the raw `task_token`. All fields reflect the wire contract;
/// only `agent_id` and `workflow_id` are consumed in Phase 1.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct TaskView {
    pub task_grant_id: String,
    pub agent_id: String,
    pub workflow_id: String,
    pub purpose: String,
    pub allowed_actions: Vec<String>,
    pub approval_required_actions: Vec<String>,
    pub denied_actions: Vec<String>,
    pub output_channels: Vec<String>,
    pub limits: TaskLimits,
    pub expires_at: String,
    /// The owner's original message text for this task, fetched
    /// in-process over this authenticated call rather than passed via
    /// CLI arg/env — argv and env are visible to a host operator via
    /// `ps`/`docker inspect`, which would otherwise leak private content
    /// outside the encrypted-artifact containment boundary.
    pub pending_message: String,
    /// Build plan Step 5 (PRD §15): the selection token(s) this grant may
    /// spend. Empty for every agent that has no selection-flow concept
    /// (e.g. `main_assistant_agent`). Defaulted so an older kernel that
    /// predates this field doesn't break deserialization.
    #[serde(default)]
    pub selection_tokens: Vec<String>,
}

/// Gate outcome of `POST /v1/actions`.
/// HTTP 200 is always returned; deny/approval outcomes are in the body.
#[derive(Debug, Clone, Deserialize)]
pub struct ActionOutcome {
    pub decision: GateDecision,
    pub result: Option<Value>,
}

/// Gate outcome of `POST /v1/model/generate`.
/// `text` is populated when `decision` is `Allow`.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelOutcome {
    pub decision: GateDecision,
    pub text: Option<String>,
}

// ── Private request bodies ────────────────────────────────────────────────────

#[derive(Serialize)]
struct ActionBody<'a> {
    action: &'a str,
    payload: Option<&'a Value>,
    target: Option<&'a Value>,
}

#[derive(Serialize)]
struct GenerateBody<'a> {
    purpose: &'a str,
    user_message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    untrusted_context: Option<&'a str>,
    max_tokens: u32,
}

// ── KernelClient ─────────────────────────────────────────────────────────────

/// Thin HTTP wrapper around the kernel API.
///
/// Constructed once at startup from `KERNEL_ENDPOINT` / `TASK_TOKEN`;
/// every method adds `Authorization: Bearer <token>` automatically.
pub struct KernelClient {
    http: Client,
    base_url: String,
    token: String,
}

impl KernelClient {
    pub fn new(base_url: String, token: String) -> Self {
        // Normalise the endpoint: trim any trailing slash so that
        // format!("{}/v1/task", base_url) never yields "//v1/task".
        let base_url = base_url.trim_end_matches('/').to_string();
        Self {
            http: Client::new(),
            base_url,
            token,
        }
    }

    /// `GET /v1/task` — fetches the redacted task-grant view.
    ///
    /// Returns `Err` on transport failure or `403` (bad/expired token, D-032).
    pub async fn get_task(&self) -> Result<TaskView> {
        let url = format!("{}/v1/task", self.base_url);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("GET /v1/task: transport error")?;

        if resp.status() == reqwest::StatusCode::FORBIDDEN {
            bail!("task token rejected by kernel (403 Forbidden)");
        }
        if !resp.status().is_success() {
            bail!("GET /v1/task: unexpected status {}", resp.status());
        }
        resp.json::<TaskView>()
            .await
            .context("GET /v1/task: response deserialization failed")
    }

    /// `POST /v1/actions` — the only way the shell causes external effects.
    ///
    /// The kernel always responds with HTTP 200; deny/approval outcomes are
    /// encoded in the body. `Err` is returned only for transport errors or
    /// `403` (expired grant, D-032).
    pub async fn submit_action(
        &self,
        action: &str,
        payload: Option<Value>,
        target: Option<Value>,
    ) -> Result<ActionOutcome> {
        let url = format!("{}/v1/actions", self.base_url);
        let body = ActionBody {
            action,
            payload: payload.as_ref(),
            target: target.as_ref(),
        };
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .context("POST /v1/actions: transport error")?;

        if resp.status() == reqwest::StatusCode::FORBIDDEN {
            bail!("task token rejected by kernel on action submit (403)");
        }
        if !resp.status().is_success() {
            bail!("POST /v1/actions: unexpected status {}", resp.status());
        }
        resp.json::<ActionOutcome>()
            .await
            .context("POST /v1/actions: response deserialization failed")
    }

    /// `POST /v1/model/generate` — kernel-mediated model invocation.
    ///
    /// The kernel gates this internally as `model.generate:approved_provider`
    /// before calling any provider. The gate outcome is in the response body.
    pub async fn generate(
        &self,
        purpose: &str,
        user_message: &str,
        untrusted_context: Option<&str>,
        max_tokens: u32,
    ) -> Result<ModelOutcome> {
        let url = format!("{}/v1/model/generate", self.base_url);
        let body = GenerateBody {
            purpose,
            user_message,
            untrusted_context,
            max_tokens,
        };
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .context("POST /v1/model/generate: transport error")?;

        if resp.status() == reqwest::StatusCode::FORBIDDEN {
            bail!("task token rejected by kernel on generate (403)");
        }
        if !resp.status().is_success() {
            bail!(
                "POST /v1/model/generate: unexpected status {}",
                resp.status()
            );
        }
        resp.json::<ModelOutcome>()
            .await
            .context("POST /v1/model/generate: response deserialization failed")
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use openspine_schemas::action::DenialReason;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn task_view_json() -> serde_json::Value {
        serde_json::json!({
            "task_grant_id": "01J000000000000000",
            "agent_id": "main_assistant_agent",
            "workflow_id": "owner_control_conversation",
            "purpose": "owner_control_conversation",
            "allowed_actions": ["openspine.status.read", "telegram.reply:owner_channel"],
            "approval_required_actions": [],
            "denied_actions": ["email.read_inbox"],
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
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
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
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
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
            .and(body_json(serde_json::json!({
                "purpose": "reply_to_owner",
                "user_message": "hi",
                "untrusted_context": "some untrusted text",
                "max_tokens": 12_000
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
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
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
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
        assert!(outcome.result.is_none());
    }

    /// (c) `approval_required` outcome is similarly `Ok`, not `Err`.
    #[tokio::test]
    async fn approval_required_is_ok_not_err() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/actions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
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
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(serde_json::json!({"error": "unauthorized"})),
            )
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
}

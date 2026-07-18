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
/// Structured outcome a worker reports back to the kernel.
#[derive(Debug, Clone, Serialize)]
pub struct WorkerResultBody {
    pub outcome: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub offered_slots: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub requests: Vec<serde_json::Value>,
}

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
    /// Missing in older kernel views means no report authority (fail closed).
    #[serde(default)]
    pub allowed_actions: Vec<String>,
    pub approval_required_actions: Vec<String>,
    pub denied_actions: Vec<String>,
    /// Explicit kernel identity marker; permission membership alone is not
    /// sufficient because root owner grants may also carry worker actions.
    #[serde(default)]
    pub is_worker: bool,
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
    /// Canonical counterparty-safe refusal, when the kernel routed an
    /// escalation. Policy text and owner-only escalation details are never
    /// represented here.
    /// This is a dormant transport contract until a shell counterparty-send
    /// producer ships; keep it deserializable without exposing owner-only
    /// escalation details.
    #[allow(dead_code)]
    pub counterparty_deferral: Option<String>,
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
    /// `POST /v1/actions` with action `worker.report_result` — the worker's
    /// ONLY outbound channel (AD-035 reply chokepoint). The kernel records a
    /// durable `worker.result` bus event; the master relays it through its own
    /// separately-gated reply path. The shell must call this exactly once after
    /// its task work completes (or fails), so the commissioned dispatch row
    /// is flipped `terminal` and never stranded.
    ///
    /// Returns `Err` on transport failure or `403` (expired/unknown grant),
    /// or if the kernel rejects the result (a `4xx` body). A `200` with a
    /// non-`allow`-equivalent body is surfaced as `Err` so the caller can
    /// propagate the failure instead of silently stranding the row.
    pub async fn report_worker_result(&self, body: &WorkerResultBody) -> Result<()> {
        let url = format!("{}/v1/actions", self.base_url);
        let action_body = ActionBody {
            action: "worker.report_result",
            payload: Some(&serde_json::to_value(body)?),
            target: None,
        };
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.token)
            .json(&action_body)
            .send()
            .await
            .context("POST /v1/actions worker.report_result: transport error")?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .context("POST /v1/actions worker.report_result: read response body")?;
        if status == reqwest::StatusCode::FORBIDDEN {
            bail!("task token rejected by kernel on worker.report_result (403)");
        }
        if !status.is_success() {
            bail!("POST /v1/actions worker.report_result: kernel returned {status}: {text}");
        }
        // HTTP 200 is *always* returned for a gate decision; a non-Allow
        // outcome (deny / approval-required / effect-suppressed) means the
        // kernel *refused* to record the result. Swallow that and we silently
        // strand the dispatch row — surface it so the caller fails loudly.
        let parsed: ActionOutcome = serde_json::from_str(&text).context(
            "POST /v1/actions worker.report_result: action outcome deserialization failed",
        )?;
        match parsed.decision {
            GateDecision::Allow => Ok(()),
            other => bail!("worker.report_result rejected by kernel gate: {other:?}"),
        }
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;

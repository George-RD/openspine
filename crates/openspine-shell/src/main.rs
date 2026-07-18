//! `openspine-shell` — the contained per-task worker process.
//!
//! Implements `implement-telegram-owner-control-slice` (4d) and
//! `implement-selected-thread-email-preview-slice` (Step 5): fetches its
//! task-grant view from the kernel, then runs whichever agent command
//! layer the grant names (`main_assistant_agent` or `email_reply_drafter`)
//! before exiting.
//!
//! Every external effect goes through `POST /v1/actions` on the kernel —
//! this process has no other I/O.  It is invoked once per task; a single
//! owner message is processed and the process exits (per-task containment
//! model of both `ProcessDriver` and `DockerDriver`).

mod agents;
mod client;

use anyhow::{Context, Result};
use clap::Parser;

use client::{KernelClient, TaskView, WorkerResultBody};

// ── CLI ───────────────────────────────────────────────────────────────────────

/// OpenSpine contained per-task worker.
///
/// Invoked by the kernel's sandbox driver once per owner-message task.
/// Reads its grant from `GET /v1/task` and routes the message through the
/// deterministic command layer; every effect goes back through the kernel API.
#[derive(Parser, Debug)]
#[command(name = "openspine-shell")]
struct Cli {
    /// Kernel HTTP endpoint, e.g. `http://127.0.0.1:7777` or
    /// `http://kernel:7777` inside the compose-internal network.
    /// Falls back to the `KERNEL_ENDPOINT` environment variable.
    #[arg(long, env = "KERNEL_ENDPOINT")]
    kernel: String,

    /// Per-task bearer token minted by the kernel at grant issuance.
    /// Falls back to the `TASK_TOKEN` environment variable.
    #[arg(long, env = "TASK_TOKEN")]
    task: String,
}

fn should_report_worker_result(task_view: &TaskView) -> bool {
    task_view.is_worker
        && task_view
            .allowed_actions
            .iter()
            .any(|action| action == "worker.report_result")
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    eprintln!("[openspine-shell] INFO: starting; kernel={}", cli.kernel);
    match run(cli).await {
        Ok(()) => {
            eprintln!("[openspine-shell] INFO: task complete, exiting 0");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("[openspine-shell] ERROR: {e:#}");
            std::process::exit(1);
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    let client = KernelClient::new(cli.kernel, cli.task);

    // Fetch the redacted task-grant view.  A 403 here means the token is
    // bad or already expired — we log and exit non-zero (transport error
    // semantics per the contract doc's Errors section).
    let task_view = client
        .get_task()
        .await
        .context("failed to fetch task grant from kernel — exiting non-zero")?;

    eprintln!(
        "[openspine-shell] INFO: grant fetched; agent={} workflow={}",
        task_view.agent_id, task_view.workflow_id
    );

    // Route to the agent implementation selected by the grant. Phases 1-2
    // ship `main_assistant_agent` and `email_reply_drafter`; other agent
    // ids are rejected here so a misconfigured grant fails loudly rather
    // than silently doing nothing.
    let agent_result = match task_view.agent_id.as_str() {
        "main_assistant_agent" => {
            agents::main_assistant::run(&client, &task_view.pending_message).await
        }
        "email_reply_drafter" => {
            agents::email_reply_drafter::run(&client, &task_view.selection_tokens).await
        }
        other => Err(anyhow::anyhow!(
            "unsupported agent_id '{}' — only main_assistant_agent and email_reply_drafter are implemented",
            other
        )),
    };

    // AD-035 reply chokepoint: a *commissioned* worker — signalled by
    // `is_worker` in the task view, which the kernel mints only for a
    // sub-grant of a master — MUST report its terminal outcome back through
    // the kernel's gated path exactly once when that effective action is
    // present. Root owner grants (including `main_assistant_agent`) may also
    // carry `worker.report_result`, so permission membership is not an
    // identity test by itself. The kernel rejects commissions lacking this
    // authority; the shell check remains fail-closed defense in depth.
    if should_report_worker_result(&task_view) {
        let outcome = if agent_result.is_err() {
            "failed"
        } else {
            "completed"
        };
        if let Err(e) = client
            .report_worker_result(&WorkerResultBody {
                outcome,
                offered_slots: Vec::new(),
                requests: Vec::new(),
            })
            .await
        {
            eprintln!("[openspine-shell] ERROR: worker.report_result failed: {e:#}");
            return if let Err(agent_err) = agent_result {
                Err(agent_err)
            } else {
                Err(e)
            };
        }
    }

    agent_result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task_view(is_worker: bool, allowed_actions: &[&str]) -> TaskView {
        TaskView {
            task_grant_id: "grant".to_string(),
            agent_id: "main_assistant_agent".to_string(),
            workflow_id: "workflow".to_string(),
            purpose: "purpose".to_string(),
            allowed_actions: allowed_actions
                .iter()
                .map(|action| (*action).to_string())
                .collect(),
            approval_required_actions: Vec::new(),
            denied_actions: Vec::new(),
            is_worker,
            output_channels: Vec::new(),
            limits: client::TaskLimits {
                max_model_calls: 1,
                max_artifacts: 1,
                max_runtime_seconds: 1,
            },
            expires_at: "2099-01-01T00:00:00Z".to_string(),
            pending_message: "pending".to_string(),
            selection_tokens: Vec::new(),
        }
    }

    #[test]
    fn report_requires_worker_identity_and_effective_permission() {
        assert!(should_report_worker_result(&task_view(
            true,
            &["worker.report_result"]
        )));
        assert!(!should_report_worker_result(&task_view(
            false,
            &["worker.report_result"]
        )));
        assert!(!should_report_worker_result(&task_view(true, &[])));
    }

    #[test]
    fn missing_allowed_actions_deserializes_as_no_report_authority() {
        let view: TaskView = serde_json::from_value(serde_json::json!({
            "task_grant_id": "grant",
            "agent_id": "main_assistant_agent",
            "workflow_id": "workflow",
            "purpose": "purpose",
            "approval_required_actions": [],
            "denied_actions": [],
            "is_worker": true,
            "output_channels": [],
            "limits": {
                "max_model_calls": 1,
                "max_artifacts": 1,
                "max_runtime_seconds": 1
            },
            "expires_at": "2099-01-01T00:00:00Z",
            "pending_message": "pending",
            "selection_tokens": []
        }))
        .unwrap();
        assert!(!should_report_worker_result(&view));
    }
}

#[cfg(test)]
mod run_tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_json, body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn allow_response(result: serde_json::Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "decision": {"outcome": "allow"},
            "counterparty_deferral": null,
            "result": result
        }))
    }

    #[tokio::test]
    async fn worker_without_report_action_exits_without_report_post() {
        let server = MockServer::start().await;
        let task_view = json!({
            "task_grant_id": "01J000000000000000",
            "agent_id": "main_assistant_agent",
            "workflow_id": "owner_control_conversation",
            "purpose": "owner_control_conversation",
            "approval_required_actions": [],
            "denied_actions": [],
            "is_worker": true,
            "output_channels": [],
            "limits": {
                "max_model_calls": 8,
                "max_artifacts": 20,
                "max_runtime_seconds": 120
            },
            "expires_at": "2099-01-01T00:00:00Z",
            "pending_message": "/status",
            "selection_tokens": []
        });
        Mock::given(method("GET"))
            .and(path("/v1/task"))
            .and(header("Authorization", "Bearer worker-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(task_view))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/actions"))
            .and(body_json(json!({
                "action": "openspine.status.read",
                "payload": null,
                "target": null
            })))
            .respond_with(allow_response(json!({"status": "ok"})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/actions"))
            .and(body_string_contains("telegram.reply:owner_channel"))
            .respond_with(allow_response(json!({"sent": true})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/actions"))
            .and(body_json(json!({
                "action": "worker.report_result",
                "payload": {"outcome": "completed"},
                "target": null
            })))
            .respond_with(allow_response(json!({"recorded": true})))
            .expect(0)
            .mount(&server)
            .await;

        run(Cli {
            kernel: server.uri(),
            task: "worker-token".to_string(),
        })
        .await
        .expect("worker without report authority still completes its task");
    }
}

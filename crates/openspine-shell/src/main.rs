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

use client::KernelClient;

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
    match task_view.agent_id.as_str() {
        "main_assistant_agent" => {
            agents::main_assistant::run(&client, &task_view.pending_message).await
        }
        "email_reply_drafter" => {
            agents::email_reply_drafter::run(&client, &task_view.selection_tokens).await
        }
        other => {
            anyhow::bail!(
                "unsupported agent_id '{}' — only main_assistant_agent and email_reply_drafter are implemented",
                other
            )
        }
    }
}

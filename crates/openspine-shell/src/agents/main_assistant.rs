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
//!   `/export <bundle-name>` → action `openspine.overlay.export`
//!   `/restore <bundle-name>` → action `openspine.overlay.restore`
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
    if let Some(name) = message.strip_prefix("/export ") {
        return cmd_overlay_bundle(client, "openspine.overlay.export", name).await;
    }
    if message == "/export" {
        return cmd_overlay_bundle(client, "openspine.overlay.export", "").await;
    }
    if let Some(name) = message.strip_prefix("/restore ") {
        return cmd_overlay_bundle(client, "openspine.overlay.restore", name).await;
    }
    if message == "/restore" {
        return cmd_overlay_bundle(client, "openspine.overlay.restore", "").await;
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

async fn cmd_overlay_bundle(client: &KernelClient, action: &str, bundle_name: &str) -> Result<()> {
    let bundle_name = bundle_name.trim();
    if bundle_name.is_empty() || bundle_name.split_whitespace().nth(1).is_some() {
        let usage = if action.ends_with("export") {
            "Usage: /export <bundle-name>"
        } else {
            "Usage: /restore <bundle-name>"
        };
        return send_reply(client, usage).await;
    }
    let payload = json!({ "bundle_name": bundle_name });
    let outcome = client.submit_action(action, Some(payload), None).await?;
    match outcome.decision {
        GateDecision::Allow => {
            let text = stub_note(
                &outcome.result,
                "Overlay operation staged; restart required.",
            );
            send_reply(client, &text).await
        }
        GateDecision::Deny { reason } => {
            eprintln!("[openspine-shell] WARN: {action} denied: {reason:?}");
            Ok(())
        }
        GateDecision::ApprovalRequired { ref approval_type } => {
            eprintln!("[openspine-shell] WARN: {action} requires approval: {approval_type}");
            Ok(())
        }
        GateDecision::EffectSuppressed => {
            eprintln!("[openspine-shell] WARN: {action} effect suppressed");
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

#[cfg(test)]
mod tests;

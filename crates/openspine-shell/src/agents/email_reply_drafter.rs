//! `email_reply_drafter` — Phase 2 selected-thread email drafting (build
//! plan Step 5, PRD §21.1 steps 10-15).
//!
//! Every effect goes through `POST /v1/actions`/`POST /v1/model/generate`
//! on the kernel, same as `main_assistant`; this agent has no other I/O.
//! `email.create_draft` itself is never called here — it is
//! approval-required (D-034), and digest-bound approval is
//! `implement-digest-bound-draft-approval`'s concern (a later change).
//! This slice's output is a preview only (design.md 6: "Ensure no email
//! send occurs").

use anyhow::{bail, Context as _, Result};
use openspine_schemas::action::GateDecision;
use serde_json::{json, Value};

use crate::client::KernelClient;

const READ_THREAD_ACTION: &str = "email.read_thread:selected_no_attachments";
const PREVIEW_ACTION: &str = "lyra.ui.preview";
const MODEL_PURPOSE: &str = "draft_reply_for_selected_email_thread";
const MAX_TOKENS: u32 = 12_000;

/// One message from the fetched thread, as returned by
/// `email.read_thread:selected_no_attachments` (kernel-side shape:
/// `{"thread_id": ..., "messages": [{"from", "subject", "body_text"}, ...]}`).
struct ThreadMessage {
    from: String,
    subject: String,
    body_text: String,
}

/// Dispatch the selected-thread draft workflow: read the bounded thread,
/// draft a reply via the model gateway (thread content wrapped as
/// untrusted data by the kernel — PRD §13), then preview it to the owner.
///
/// Same `Ok(())`-on-deny/approval-required contract as `main_assistant`:
/// a gate() outcome other than `Allow` is logged and the shell exits 0 —
/// the kernel already recorded the audit row. Only transport/`5xx` errors
/// propagate as `Err`.
pub async fn run(client: &KernelClient, selection_tokens: &[String]) -> Result<()> {
    let Some(token_id) = selection_tokens.first() else {
        bail!("email_reply_drafter task grant carries no selection_tokens — nothing to read");
    };

    let Some(messages) = read_selected_thread(client, token_id).await? else {
        return Ok(()); // denied/approval-required — already logged, already audited
    };

    let untrusted_context = format_thread_for_model(&messages);
    let Some(draft) = draft_reply(client, &untrusted_context).await? else {
        return Ok(());
    };

    preview_draft(client, &messages, &draft).await
}

/// `email.read_thread:selected_no_attachments` — returns `Ok(None)` for a
/// non-`Allow` gate decision (already logged), `Ok(Some(messages))` on
/// success.
async fn read_selected_thread(
    client: &KernelClient,
    token_id: &str,
) -> Result<Option<Vec<ThreadMessage>>> {
    let payload = json!({ "selection_token_id": token_id });
    let outcome = client
        .submit_action(READ_THREAD_ACTION, Some(payload), None)
        .await?;
    match outcome.decision {
        GateDecision::Allow => {
            let result = outcome.result.context(
                "email.read_thread:selected_no_attachments allowed but returned no result",
            )?;
            Ok(Some(parse_thread_messages(&result)?))
        }
        GateDecision::Deny { reason } => {
            eprintln!("[openspine-shell] WARN: email.read_thread denied: {reason:?}");
            Ok(None)
        }
        GateDecision::ApprovalRequired { ref approval_type } => {
            eprintln!(
                "[openspine-shell] WARN: email.read_thread requires approval: {approval_type}"
            );
            Ok(None)
        }
    }
}

fn parse_thread_messages(result: &Value) -> Result<Vec<ThreadMessage>> {
    let messages = result["messages"]
        .as_array()
        .context("email.read_thread result missing a \"messages\" array")?;
    messages
        .iter()
        .map(|m| {
            Ok(ThreadMessage {
                from: m["from"].as_str().unwrap_or_default().to_string(),
                subject: m["subject"].as_str().unwrap_or_default().to_string(),
                body_text: m["body_text"].as_str().unwrap_or_default().to_string(),
            })
        })
        .collect()
}

/// Render the fetched thread as plain text handed to the kernel as
/// `untrusted_context` — this text is never treated as trusted by the
/// kernel's model gateway (`build_prompt_with_untrusted_context`); it is
/// merely constructed here in a model-readable shape.
fn format_thread_for_model(messages: &[ThreadMessage]) -> String {
    messages
        .iter()
        .map(|m| {
            format!(
                "From: {}\nSubject: {}\n\n{}",
                m.from, m.subject, m.body_text
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

/// One drafted reply.
struct Draft {
    subject: String,
    body: String,
}

async fn draft_reply(client: &KernelClient, untrusted_context: &str) -> Result<Option<Draft>> {
    let instruction = "Draft a reply to the email thread below on the owner's behalf. \
        Reply with the reply body text only — no subject line, no headers, no commentary.";
    let outcome = client
        .generate(
            MODEL_PURPOSE,
            instruction,
            Some(untrusted_context),
            MAX_TOKENS,
        )
        .await?;
    match outcome.decision {
        GateDecision::Allow => {
            let body = outcome.text.unwrap_or_default();
            if body.is_empty() {
                eprintln!(
                    "[openspine-shell] WARN: model returned an empty draft; skipping preview"
                );
                return Ok(None);
            }
            Ok(Some(Draft {
                subject: "Re: your email".to_string(),
                body,
            }))
        }
        GateDecision::Deny { reason } => {
            eprintln!("[openspine-shell] WARN: model.generate denied: {reason:?}");
            Ok(None)
        }
        GateDecision::ApprovalRequired { ref approval_type } => {
            eprintln!("[openspine-shell] WARN: model.generate requires approval: {approval_type}");
            Ok(None)
        }
    }
}

async fn preview_draft(
    client: &KernelClient,
    messages: &[ThreadMessage],
    draft: &Draft,
) -> Result<()> {
    let subject = messages
        .first()
        .filter(|m| !m.subject.is_empty())
        .map(|m| format!("Re: {}", m.subject))
        .unwrap_or_else(|| draft.subject.clone());
    let payload = json!({ "subject": subject, "body": draft.body });
    let outcome = client
        .submit_action(PREVIEW_ACTION, Some(payload), None)
        .await?;
    match outcome.decision {
        GateDecision::Allow => {
            eprintln!("[openspine-shell] INFO: draft preview sent to owner");
        }
        GateDecision::Deny { reason } => {
            eprintln!("[openspine-shell] WARN: lyra.ui.preview denied: {reason:?}");
        }
        GateDecision::ApprovalRequired { ref approval_type } => {
            eprintln!("[openspine-shell] WARN: lyra.ui.preview requires approval: {approval_type}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn thread_result() -> Value {
        json!({
            "thread_id": "thread-1",
            "messages": [{
                "from": "alice@example.com",
                "subject": "Invoice question",
                "body_text": "Could you confirm the invoice total?",
            }],
        })
    }

    #[tokio::test]
    async fn full_flow_reads_drafts_and_previews() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/actions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "decision": {"outcome": "allow"},
                "result": thread_result(),
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/model/generate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "decision": {"outcome": "allow"},
                "text": "Yes, the total is $500.",
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/actions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "decision": {"outcome": "allow"},
                "result": {"sent": true},
            })))
            .mount(&server)
            .await;

        let client = KernelClient::new(server.uri(), "test-token".to_string());
        run(&client, &["01J000000000000000".to_string()])
            .await
            .expect("full flow should succeed");
    }

    #[tokio::test]
    async fn no_selection_tokens_is_an_error() {
        let client = KernelClient::new("http://localhost:0".to_string(), "t".to_string());
        let err = run(&client, &[]).await.unwrap_err();
        assert!(err.to_string().contains("no selection_tokens"));
    }

    #[tokio::test]
    async fn denied_read_thread_stops_without_drafting() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/actions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "decision": {"outcome": "deny", "reason": "grant_expired"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = KernelClient::new(server.uri(), "t".to_string());
        run(&client, &["01J000000000000000".to_string()])
            .await
            .expect("a deny is not an error");
    }

    #[test]
    fn parse_thread_messages_extracts_fields() {
        let messages = parse_thread_messages(&thread_result()).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].from, "alice@example.com");
        assert_eq!(messages[0].subject, "Invoice question");
        assert_eq!(
            messages[0].body_text,
            "Could you confirm the invoice total?"
        );
    }

    #[test]
    fn format_thread_for_model_includes_all_fields() {
        let messages = parse_thread_messages(&thread_result()).unwrap();
        let formatted = format_thread_for_model(&messages);
        assert!(formatted.contains("alice@example.com"));
        assert!(formatted.contains("Invoice question"));
        assert!(formatted.contains("Could you confirm the invoice total?"));
    }

    #[tokio::test]
    async fn empty_draft_skips_preview_without_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/actions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "decision": {"outcome": "allow"},
                "result": thread_result(),
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/model/generate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "decision": {"outcome": "allow"},
                "text": "",
            })))
            .mount(&server)
            .await;

        let client = KernelClient::new(server.uri(), "t".to_string());
        run(&client, &["01J000000000000000".to_string()])
            .await
            .expect("empty draft is not an error");
    }
}

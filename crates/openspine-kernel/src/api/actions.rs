//! `POST /v1/actions` — the only way the shell may cause an external
//! effect (build plan 4a/4b/4d).

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use jiff::Timestamp;
use openspine_gate::gate;
use openspine_schemas::action::{ActionId, ActionRequest, GateDecision};
use openspine_schemas::digest::canonical_json;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::selection::SelectionTokenType;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::str::FromStr;
use ulid::Ulid;

use super::{authenticate, internal_error};
use crate::pipeline::AppState;

#[derive(Debug, Deserialize)]
pub(super) struct ActionRequestBody {
    action: String,
    #[serde(default)]
    #[allow(dead_code)] // see the comment on `dispatch_allowed_action` below
    target: Option<Value>,
    #[serde(default)]
    payload: Option<Value>,
}

/// The wire-contract-mandated shape of `telegram.reply:owner_channel`'s
/// payload. `deny_unknown_fields` matters here beyond style: the reply
/// always targets the grant's `bound_chat_id` (channel binding by
/// construction — the contract defines no field to override it), and this
/// makes that guarantee enforced rather than merely assumed. A shell that
/// tried to smuggle e.g. `"chat_id"` in here gets a hard parse failure
/// instead of the field being silently dropped by default serde behavior.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TelegramReplyPayload {
    text: String,
}

/// `email.read_thread:selected_no_attachments`'s payload (build plan Step
/// 5): the shell names which of *its own grant's* selection tokens to
/// consume — it can never mint or alter one (PRD §15), only spend a token
/// the kernel already bound to it (see `GET /v1/task`'s `selection_tokens`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadThreadPayload {
    selection_token_id: String,
}

/// `lyra.ui.preview`'s payload (build plan Step 5): a draft summary shown
/// to the owner. Distinct action id from `telegram.reply:owner_channel`
/// (which `email_reply_drafter` is denied) — the kernel, not the agent,
/// controls exactly what a preview dispatch does.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewPayload {
    subject: String,
    body: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ActionResponseBody {
    decision: GateDecision,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
}

pub(super) async fn post_actions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ActionRequestBody>,
) -> Result<Json<ActionResponseBody>, (StatusCode, Json<Value>)> {
    let (grant, _pending_ref, bound_chat_id) = authenticate(&state, &headers).await?;
    let now = Timestamp::now();
    let action = ActionId::new(body.action);

    let payload_ref = match &body.payload {
        Some(value) => {
            let bytes = canonical_json(value).into_bytes();
            Some(state.artifacts.put(&bytes).map_err(internal_error)?)
        }
        None => None,
    };

    // Step 4 has no action that consumes a typed `target_ref`/`target_digest`
    // (the wire contract carries `target` generically for a future action —
    // Phase 2/3's connector dispatch — that actually needs one); translating
    // it here would be a conversion with no real caller to verify it against.
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: action.clone(),
        target_ref: None,
        payload_ref: payload_ref.clone(),
        target_digest: None,
        requested_at: now,
        schema_version: 1,
    };

    let outcome = gate(&grant, &request, &state.store, now);
    state
        .store
        .append_audit(
            "action.gated",
            Some(&action),
            Some(&outcome.decision),
            None,
            Some(grant.id),
            &[],
            payload_ref.as_slice(),
        )
        .map_err(internal_error)?;

    let GateDecision::Allow = outcome.decision else {
        return Ok(Json(ActionResponseBody {
            decision: outcome.decision,
            result: None,
        }));
    };

    match dispatch_allowed_action(
        &state,
        &grant,
        &action,
        bound_chat_id,
        body.payload.as_ref(),
    )
    .await
    {
        Ok(result) => Ok(Json(ActionResponseBody {
            decision: GateDecision::Allow,
            result: Some(result),
        })),
        Err(err) => {
            let (status, message) = match &err {
                DispatchError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
                DispatchError::Internal(cause) => {
                    tracing::error!(error = %cause, "action dispatch failed");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal_error".to_string(),
                    )
                }
            };
            let _ = state.store.append_audit(
                "action.dispatch_failed",
                Some(&action),
                None,
                Some(&message),
                Some(grant.id),
                &[],
                &[],
            );
            Err((status, Json(json!({"error": message}))))
        }
    }
}

/// Distinguishes a shell contract violation (bad request shape — its own
/// mistake, `400`) from a genuine kernel/infrastructure failure (`500`).
/// Both are audited via `action.dispatch_failed` either way (see
/// [`post_actions`]) so "why didn't Lyra reply" stays answerable from
/// `audit_log` alone even when the dispatch itself failed.
enum DispatchError {
    BadRequest(String),
    Internal(anyhow::Error),
}

/// Telegram hard-caps a single message at 4096 UTF-16 code units — a
/// model-drafted body long enough to exceed that would otherwise turn a
/// successful draft into a failed `send_reply` call (`500`, from the
/// *kernel's* side, after everything upstream genuinely succeeded).
/// Truncates by actual UTF-16 unit count (`char::len_utf16`), not `char`
/// count — a `char` can be up to 2 UTF-16 units (e.g. many emoji), so
/// counting `char`s alone under-truncates for unit-count limits like
/// Telegram's.
const TELEGRAM_MAX_MESSAGE_UTF16_UNITS: usize = 4000;

fn truncate_for_telegram(text: &str) -> String {
    let mut units = 0usize;
    for (idx, ch) in text.char_indices() {
        let w = ch.len_utf16();
        if units + w > TELEGRAM_MAX_MESSAGE_UTF16_UNITS {
            let mut truncated = text[..idx].to_string();
            truncated.push_str("… [truncated]");
            return truncated;
        }
        units += w;
    }
    text.to_string()
}

/// Run the effect of one `gate()`-allowed action. Only reached after
/// `Allow` — a deny/approval-required decision never calls this.
///
/// `openspine.status.read`, `telegram.reply:owner_channel`,
/// `email.read_thread:selected_no_attachments`, and `lyra.ui.preview` are
/// real; `workflow.invoke:approved`, `artifact.propose`, and
/// `setup.workflow.start` are specified stubs (`tasks.md`: "Do not
/// implement real behavior for these three — a stub response is the
/// specified deliverable"). Any other allowed action (e.g.
/// `memory.read:owner_preferences_limited`, which a capability pack can
/// grant but no kernel-side subsystem yet exists for) falls through to the
/// same honest stub shape rather than a 500 — an *authorized* action must
/// never fail the request just because its kernel-side implementation
/// doesn't exist yet.
async fn dispatch_allowed_action(
    state: &AppState,
    grant: &TaskGrant,
    action: &ActionId,
    bound_chat_id: i64,
    payload: Option<&Value>,
) -> Result<Value, DispatchError> {
    match action.0.as_str() {
        "openspine.status.read" => Ok(json!({"status": "ok"})),
        "telegram.reply:owner_channel" => {
            let payload = payload.ok_or_else(|| {
                DispatchError::BadRequest(
                    "telegram.reply:owner_channel requires a payload".to_string(),
                )
            })?;
            let reply: TelegramReplyPayload =
                serde_json::from_value(payload.clone()).map_err(|_| {
                    DispatchError::BadRequest(
                        "telegram.reply:owner_channel payload must be exactly {\"text\": string}"
                            .to_string(),
                    )
                })?;
            state
                .telegram
                .send_reply(bound_chat_id, &reply.text)
                .await
                .map_err(DispatchError::Internal)?;
            Ok(json!({"sent": true}))
        }
        "email.read_thread:selected_no_attachments" => {
            dispatch_read_selected_thread(state, grant, payload).await
        }
        "lyra.ui.preview" => {
            let payload = payload.ok_or_else(|| {
                DispatchError::BadRequest("lyra.ui.preview requires a payload".to_string())
            })?;
            let preview: PreviewPayload = serde_json::from_value(payload.clone()).map_err(|_| {
                DispatchError::BadRequest(
                    "lyra.ui.preview payload must be exactly {\"subject\": string, \"body\": string}"
                        .to_string(),
                )
            })?;
            let text = truncate_for_telegram(&format!(
                "Draft preview\nSubject: {}\n\n{}",
                preview.subject, preview.body
            ));
            state
                .telegram
                .send_reply(bound_chat_id, &text)
                .await
                .map_err(DispatchError::Internal)?;
            Ok(json!({"sent": true}))
        }
        "workflow.invoke:approved" => Ok(json!({
            "stub": true,
            "note": "workflow invocation is a Step 4 stub; no workflow execution engine exists yet",
        })),
        "artifact.propose" => Ok(json!({
            "stub": true,
            "note": "artifact proposal is a Step 4 stub; proposed artifacts are not yet persisted or reviewed",
        })),
        "setup.workflow.start" => Ok(json!({
            "stub": true,
            "note": "the setup workflow is a Step 4 stub; no setup wizard exists yet",
        })),
        other => Ok(json!({
            "stub": true,
            "note": format!("{other} has no Step 4 kernel-side implementation yet"),
        })),
    }
}

/// `email.read_thread:selected_no_attachments`'s real implementation
/// (build plan Step 5): validate the shell's named selection token is
/// bound to *this* grant, atomically consume it (PRD §15 single-use), then
/// fetch the bounded, attachment-free thread from Gmail. Every validation
/// failure here is the shell's own contract violation (a foreign, unknown,
/// expired, wrong-type, or already-used token) — `400`, not `500`; only an
/// actual Gmail-connector failure after a valid consume is `500`.
async fn dispatch_read_selected_thread(
    state: &AppState,
    grant: &TaskGrant,
    payload: Option<&Value>,
) -> Result<Value, DispatchError> {
    let payload = payload.ok_or_else(|| {
        DispatchError::BadRequest(
            "email.read_thread:selected_no_attachments requires a payload".to_string(),
        )
    })?;
    let request: ReadThreadPayload = serde_json::from_value(payload.clone()).map_err(|_| {
        DispatchError::BadRequest(
            "email.read_thread:selected_no_attachments payload must be exactly \
             {\"selection_token_id\": string}"
                .to_string(),
        )
    })?;
    let token_id = Ulid::from_str(&request.selection_token_id).map_err(|_| {
        DispatchError::BadRequest("selection_token_id is not a valid id".to_string())
    })?;

    if !grant.selection_tokens.contains(&token_id) {
        return Err(DispatchError::BadRequest(
            "selection_token_id is not bound to this task grant".to_string(),
        ));
    }

    let token = state
        .store
        .find_selection_token(token_id)
        .map_err(|err| DispatchError::Internal(err.into()))?
        .ok_or_else(|| DispatchError::BadRequest("unknown selection token".to_string()))?;

    if token.token_type != SelectionTokenType::EmailThreadSelection {
        return Err(DispatchError::BadRequest(
            "selection token is not an email thread selection".to_string(),
        ));
    }
    if token.expires_at <= Timestamp::now() {
        return Err(DispatchError::BadRequest(
            "selection token has expired".to_string(),
        ));
    }

    // Atomic — see `Store::try_consume_selection_token`'s doc comment on
    // why this must happen in one statement, before the Gmail call.
    let consumed = state
        .store
        .try_consume_selection_token(token_id)
        .map_err(|err| DispatchError::Internal(err.into()))?;
    if !consumed {
        return Err(DispatchError::BadRequest(
            "selection token has already been used".to_string(),
        ));
    }

    let gmail = state.gmail.as_ref().ok_or_else(|| {
        DispatchError::Internal(anyhow::anyhow!(
            "selection token exists but no gmail connector is configured"
        ))
    })?;
    let thread = gmail
        .fetch_thread(&token.target_id)
        .await
        .map_err(|err| DispatchError::Internal(err.into()))?;

    Ok(json!({
        "thread_id": thread.thread_id,
        "messages": thread.messages.iter().map(|m| json!({
            "from": m.from,
            "subject": m.subject,
            "body_text": m.body_text,
        })).collect::<Vec<_>>(),
    }))
}

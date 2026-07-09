//! `POST /v1/actions` — the only way the shell may cause an external
//! effect (build plan 4a/4b/4d).

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use jiff::Timestamp;
use openspine_gate::gate;
use openspine_schemas::action::{ActionId, ActionRequest, GateDecision};
use openspine_schemas::digest::{canonical_json, digest_of};
use openspine_schemas::event::{TargetRef, TargetRefKind};
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::selection::SelectionTokenType;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::str::FromStr;
use ulid::Ulid;

use super::telegram_truncate::{truncate_for_telegram, truncate_with_notice};
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
pub(super) struct TelegramReplyPayload {
    pub(super) text: String,
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
pub(super) struct PreviewPayload {
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

    let outcome = gate(&grant, &request, &state.store, &state.action_catalog, now);
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
#[derive(Debug)]
pub(crate) enum DispatchError {
    BadRequest(String),
    Internal(anyhow::Error),
}

/// Run the effect of one `gate()`-allowed action. Only reached after
/// `Allow` — a deny/approval-required decision never calls this.
///
/// `openspine.status.read`, `telegram.reply:owner_channel`,
/// `email.read_thread:selected_no_attachments`, `lyra.ui.preview`, and
/// `artifact.propose` (5c: validate, persist, and ask the owner to
/// approve activation) are real; `workflow.invoke:approved` and
/// `setup.workflow.start` remain specified stubs (`tasks.md`: "Do not
/// implement real behavior for these — a stub response is the specified
/// deliverable"). Any other allowed action (e.g.
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
    let id = action.0.as_str();
    match state.action_handlers.lookup(id) {
        Some(handler) => handler(state, grant, bound_chat_id, payload).await,
        None => Ok(json!({
            "stub": true,
            "note": format!("{id} has no Step 4 kernel-side implementation yet"),
        })),
    }
}

/// `lyra.ui.preview`'s real implementation (build plan Step 5, extended by
/// Step 6 / D-043, hardened by D-045): shows the draft to the owner AND, in
/// the same dispatch, proposes it for approval — the two must never drift
/// apart (D-043's whole rationale: a separate propose action could let
/// "what was shown" and "what was proposed" diverge). D-045 extends that
/// guarantee to truncation: `propose_draft_creation` always binds approval
/// to the *full* `preview.body`, so if the message shown to the owner had
/// to be cut short, no approval may be proposed for it at all — the owner
/// must never be able to tap Approve on content they were not shown in
/// full. If proposing fails for any other reason (no Gmail connector, no
/// selection token on this grant, the thread no longer resolves, no
/// non-owner correspondent found, artifact budget exhausted), the preview
/// is still shown — the owner sees the draft but the message carries no
/// approval button, an honest reflection of "propose failed" rather than a
/// silently-broken button.
pub(super) async fn dispatch_lyra_preview(
    state: &AppState,
    grant: &TaskGrant,
    bound_chat_id: i64,
    preview: &PreviewPayload,
) -> Result<Value, DispatchError> {
    let full = format!(
        "Draft preview\nSubject: {}\n\n{}",
        preview.subject, preview.body
    );
    let text = truncate_for_telegram(&full);
    let was_truncated = text != full;

    if was_truncated {
        let _ = state.store.append_audit(
            "draft.proposal_failed",
            Some(&ActionId::new("email.create_draft")),
            None,
            Some("preview_truncated"),
            Some(grant.id),
            &[],
            &[],
        );
        state
            .connectors
            .telegram()
            .send_reply(bound_chat_id, &truncate_with_notice(&full))
            .await
            .map_err(DispatchError::Internal)?;
        return Ok(json!({"sent": true}));
    }

    match propose_draft_creation(state, grant, preview).await {
        Ok(action_request_id) => {
            state
                .connectors
                .telegram()
                .send_reply_with_approval_button(bound_chat_id, &text, action_request_id)
                .await
                .map_err(DispatchError::Internal)?;
        }
        Err(reason) => {
            let _ = state.store.append_audit(
                "draft.proposal_failed",
                Some(&ActionId::new("email.create_draft")),
                None,
                Some(reason),
                Some(grant.id),
                &[],
                &[],
            );
            state
                .connectors
                .telegram()
                .send_reply(bound_chat_id, &text)
                .await
                .map_err(DispatchError::Internal)?;
        }
    }
    Ok(json!({"sent": true}))
}

/// D-043: derive the target (D-042, never trusting anything from the
/// shell for it), store the payload artifact, and persist the pending
/// `email.create_draft` [`ActionRequest`] (D-040/D-041) that a later
/// `callback_query` approval (D-044) will be bound to.
async fn propose_draft_creation(
    state: &AppState,
    grant: &TaskGrant,
    preview: &PreviewPayload,
) -> Result<Ulid, &'static str> {
    let gmail = state.connectors.gmail().ok_or("no_gmail_connector")?;
    let token_id = grant
        .selection_tokens
        .first()
        .copied()
        .ok_or("no_selection_token_on_grant")?;
    let token = state
        .store
        .find_selection_token(token_id)
        .map_err(|_| "selection_token_lookup_failed")?
        .ok_or("selection_token_not_found")?;
    let thread = gmail
        .fetch_thread(&token.target_id)
        .await
        .map_err(|_| "gmail_thread_fetch_failed")?;
    let target = crate::gmail::newest_non_owner_recipient(&thread, gmail.mailbox_address())
        .ok_or("no_non_owner_recipient_found")?;

    // D-046: the draft-proposal payload is a shell-initiated artifact put
    // — counts against `max_artifacts` the same way `model.generate`'s
    // payload snapshot does.
    if !state
        .store
        .try_count_artifact_put(grant.id, grant.limits.max_artifacts)
        .map_err(|_| "artifact_budget_check_failed")?
    {
        return Err("artifact_budget_exhausted");
    }
    let payload_bytes =
        canonical_json(&json!({ "subject": preview.subject, "body": preview.body }));
    let payload_ref = state
        .artifacts
        .put(payload_bytes.as_bytes())
        .map_err(|_| "artifact_store_failed")?;
    // D-041: recipients as a list, not a bare string — so a future
    // reply-all/Cc addition widens this shape without changing what the
    // field name itself means to an already-approved digest.
    let target_digest = digest_of(&json!({
        "thread_id": token.target_id,
        "connector": "gmail_primary",
        "account_role": "owner_mailbox",
        "recipients": [target.recipient],
    }));

    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: ActionId::new("email.create_draft"),
        target_ref: Some(TargetRef {
            kind: TargetRefKind::EmailThread,
            id: Some(token.target_id.clone()),
        }),
        payload_ref: Some(payload_ref),
        target_digest: Some(target_digest),
        requested_at: Timestamp::now(),
        schema_version: 1,
    };
    state
        .store
        .insert_action_request(&request)
        .map_err(|_| "action_request_persist_failed")?;
    Ok(request.id)
}

/// `email.read_thread:selected_no_attachments`'s real implementation
/// (build plan Step 5): validate the shell's named selection token is
/// bound to *this* grant, atomically consume it (PRD §15 single-use), then
/// fetch the bounded, attachment-free thread from Gmail. Every validation
/// failure here is the shell's own contract violation (a foreign, unknown,
/// expired, wrong-type, or already-used token) — `400`, not `500`; only an
/// actual Gmail-connector failure after a valid consume is `500`.
pub(super) async fn dispatch_read_selected_thread(
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

    let gmail = state.connectors.gmail().ok_or_else(|| {
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

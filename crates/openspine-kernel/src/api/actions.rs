use super::authenticate;
use super::proposal::propose_draft_creation;
use super::telegram_truncate::{truncate_for_telegram, truncate_with_notice};
use crate::failure_surfacing::{batch_failure, FailureClass};
use crate::pipeline::AppState;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use jiff::Timestamp;
use openspine_gate::{gate, ActionOrigin};
use openspine_schemas::action::{ActionId, ActionRequest, GateDecision};
use openspine_schemas::digest::canonical_json;
use openspine_schemas::escalation::{surface_denial, EscalationEvent};
use openspine_schemas::grant::TaskGrant;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::str::FromStr;
use std::sync::Arc;
use ulid::Ulid;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActionRequestBody {
    action: String,
    #[serde(default)]
    #[allow(dead_code)] // see the comment on `dispatch_allowed_action` below
    target: Option<Value>,
    #[serde(default)]
    payload: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TelegramReplyPayload {
    pub(super) text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadThreadPayload {
    selection_token_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PreviewPayload {
    pub(super) subject: String,
    pub(super) body: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ActionResponseBody {
    decision: GateDecision,
    #[serde(skip_serializing_if = "Option::is_none")]
    counterparty_deferral: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
}

pub(super) async fn post_actions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ActionRequestBody>,
) -> Result<Json<ActionResponseBody>, (StatusCode, Json<Value>)> {
    let (grant, _pending_ref, bound_chat_id) = authenticate(&state, &headers).await?;
    let action = ActionId::new(body.action);
    let (decision, counterparty_deferral, result) = mediate_and_dispatch_action(
        &state,
        &grant,
        action,
        bound_chat_id,
        body.payload.as_ref(),
        FailureSurface::DirectResponse,
    )
    .await
    .map_err(|err| match &err {
        DispatchError::BadRequest(message) => {
            (StatusCode::BAD_REQUEST, Json(json!({"error": message})))
        }
        DispatchError::Connector(cause) | DispatchError::Resource(cause) => {
            tracing::error!(error = %cause, "action dispatch failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal_error"})),
            )
        }
    })?;
    Ok(Json(ActionResponseBody {
        decision,
        counterparty_deferral,
        result,
    }))
}

/// Shared non-HTTP mediation boundary used by both HTTP actions and durable
/// workflow adapters. It is the single path that builds the request, calls
/// `gate()`, emits the gate/failure audit events, and dispatches the concrete
/// handler selected by the action registry.
pub(crate) async fn mediate_and_dispatch_action(
    state: &AppState,
    grant: &TaskGrant,
    action: ActionId,
    bound_chat_id: i64,
    payload: Option<&Value>,
    surface: FailureSurface,
) -> Result<(GateDecision, Option<String>, Option<Value>), DispatchError> {
    let now = Timestamp::now();
    let payload_ref = match payload {
        Some(value) => Some(
            state
                .artifacts
                .put(canonical_json(value).as_bytes())
                .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?,
        ),
        None => None,
    };
    let selection_token_id = payload
        .and_then(|value| value.get("selection_token_id"))
        .and_then(Value::as_str)
        .and_then(|value| Ulid::from_str(value).ok());
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: action.clone(),
        target_ref: None,
        payload_ref: payload_ref.clone(),
        target_digest: None,
        selection_token_id,
        requested_at: now,
        schema_version: 1,
    };
    let outcome = gate(
        grant,
        &request,
        ActionOrigin::Shell,
        &state.store,
        &state.action_catalog,
        &state.connectors,
        now,
    );
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
        .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?;

    let decision = outcome.decision;
    if !matches!(decision, GateDecision::Allow) {
        if state.action_catalog.is_counterparty_facing(&action) {
            if let Some((deferral, notice)) = surface_denial(grant, &action, &decision, None, now) {
                let event = EscalationEvent::from_denial(&notice);
                // AD-133: route through the reusable kernel machinery. It
                // resolves the persisted task's bound owner chat itself;
                // the owner-only reason never returns to the worker.
                crate::escalation::route_escalation(state, grant, &event)
                    .await
                    .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?;

                return Ok((decision, Some(deferral.text.to_string()), None));
            }
        }

        // Non-counterparty denials retain the ordinary typed enum outcome.
        return Ok((decision, None, None));
    }

    match dispatch_allowed_action(state, grant, &action, bound_chat_id, payload).await {
        Ok(result) => Ok((GateDecision::Allow, None, Some(result))),
        Err(err) => {
            let digest_class = match &err {
                DispatchError::Resource(_) => FailureClass::Resource,
                DispatchError::Connector(_) => FailureClass::Connector,
                DispatchError::BadRequest(_) => FailureClass::Connector,
            };
            let digest_summary = match &err {
                DispatchError::BadRequest(msg) => msg.clone(),
                DispatchError::Connector(cause) | DispatchError::Resource(cause) => {
                    tracing::error!(error = %cause, "action dispatch failed");
                    format!("{action}: {cause}")
                }
            };
            state
                .store
                .append_audit(
                    "action.dispatch_failed",
                    Some(&action),
                    None,
                    None,
                    Some(grant.id),
                    &[],
                    &[],
                )
                .map_err(|audit_err| DispatchError::Resource(anyhow::Error::new(audit_err)))?;
            let suppress_batch = matches!(err, DispatchError::BadRequest(_))
                && surface == FailureSurface::DirectResponse;
            if !suppress_batch {
                batch_failure(
                    state,
                    digest_class,
                    &format!("{action} dispatch failed"),
                    &digest_summary,
                )
                .map_err(|batch_err| DispatchError::Resource(anyhow::Error::new(batch_err)))?;
            }
            Err(err)
        }
    }
}

#[derive(Debug)]
pub(crate) enum DispatchError {
    BadRequest(String),
    Connector(anyhow::Error),
    Resource(anyhow::Error),
}

/// How a mediation caller surfaces dispatch failures to the owner. D-068:
/// an authenticated API caller receives bad requests directly in its typed
/// response, so they are not duplicated into the failure digest. Detached
/// callers (durable workflow adapters) have no direct response surface, so
/// every failure class enters the failure lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailureSurface {
    DirectResponse,
    Detached,
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
/// full.
///
/// A `Resource`-class `ProposalError` is fatal: it is returned as a typed
/// `DispatchError::Resource` and the outer `post_actions` layer audits and
/// batches it exactly once (`post_actions` already does this for every
/// returned `Resource`/`Connector` error, so this arm must not batch a
/// Resource error itself or it would be double-counted). A `Connector`-class
/// error returns `Ok(sent:true)` (an honest "propose failed, no approval
/// button" rather than a broken button), so it is batched here — and only
/// once the durable digest write succeeds does the preview get shown; if
/// that write fails it escalates to a typed `Resource` error (PI parent
/// note: a Resource failure must never be reported as a successful preview).
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
    if text != full {
        state
            .store
            .append_audit(
                "draft.proposal_failed",
                Some(&ActionId::new("email.create_draft")),
                None,
                Some("preview_truncated"),
                Some(grant.id),
                &[],
                &[],
            )
            .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?;
        let send_result = state
            .connectors
            .telegram()
            .send_reply(bound_chat_id, &truncate_with_notice(&full))
            .await;
        if let Err(counter_err) = crate::failure_surfacing::record_connector_outcome(
            &state.store,
            "telegram",
            send_result.is_ok(),
        ) {
            tracing::error!(error = %counter_err, "failed to persist Telegram counter");
            if let Err(surface_err) = crate::failure_surfacing::batch_failure(
                state,
                crate::failure_surfacing::FailureClass::Resource,
                "Telegram counter persistence failed",
                "Telegram counter persistence failed",
            ) {
                tracing::error!(error = %surface_err, "counter failure surface append failed");
            }
        }
        send_result.map_err(DispatchError::Connector)?;
        return Ok(json!({"sent": true}));
    }
    match propose_draft_creation(state, grant, preview).await {
        Ok(action_request_id) => {
            let send_result = state
                .connectors
                .telegram()
                .send_reply_with_approval_button(bound_chat_id, &text, action_request_id)
                .await;
            if let Err(counter_err) = crate::failure_surfacing::record_connector_outcome(
                &state.store,
                "telegram",
                send_result.is_ok(),
            ) {
                tracing::error!(error = %counter_err, "failed to persist Telegram counter");
                if let Err(surface_err) = crate::failure_surfacing::batch_failure(
                    state,
                    crate::failure_surfacing::FailureClass::Resource,
                    "Telegram counter persistence failed",
                    "Telegram counter persistence failed",
                ) {
                    tracing::error!(error = %surface_err, "counter failure surface append failed");
                }
            }
            send_result.map_err(DispatchError::Connector)?;
        }
        Err(err) => {
            // Resource-class propose failures are fatal. Return the typed
            // error and let the outer `post_actions` layer audit + batch it
            // exactly once — do NOT batch here, or the failure is counted
            // twice (this is the `gmail_failure_surface_record_failed` trap
            // the parent required us to remove).
            if err.failure_class() == FailureClass::Resource {
                return Err(DispatchError::Resource(anyhow::Error::new(err)));
            }
            // Connector-class propose failures return `Ok(sent:true)`, so the
            // outer layer never sees an error to batch. Surface them durably
            // here, and only continue to show the preview once the digest
            // write succeeds. If the write fails, escalate to a typed
            // Resource error (the outer layer batches that store failure).
            batch_failure(
                state,
                FailureClass::Connector,
                "lyra.ui.preview proposal failed",
                &err.to_string(),
            )
            .map_err(|surface_err| DispatchError::Resource(anyhow::Error::new(surface_err)))?;
            state
                .store
                .append_audit(
                    "draft.proposal_failed",
                    Some(&ActionId::new("email.create_draft")),
                    None,
                    None,
                    Some(grant.id),
                    &[],
                    &[],
                )
                .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?;
            let send_result = state
                .connectors
                .telegram()
                .send_reply(bound_chat_id, &text)
                .await;
            if let Err(counter_err) = crate::failure_surfacing::record_connector_outcome(
                &state.store,
                "telegram",
                send_result.is_ok(),
            ) {
                tracing::error!(error = %counter_err, "failed to persist Telegram counter");
                if let Err(surface_err) = crate::failure_surfacing::batch_failure(
                    state,
                    crate::failure_surfacing::FailureClass::Resource,
                    "Telegram counter persistence failed",
                    "Telegram counter persistence failed",
                ) {
                    tracing::error!(error = %surface_err, "counter failure surface append failed");
                }
            }
            send_result.map_err(DispatchError::Connector)?;
            return Ok(json!({"sent": true}));
        }
    }
    Ok(json!({"sent": true}))
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
    _grant: &TaskGrant,
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

    // gate() (in post_actions) has already validated token possession,
    // grant binding, type, and expiry. Re-read the token here only to obtain
    // the target id the Gmail fetch needs (D-055.1: validation now lives in
    // the pure gate, not dispatch).
    let token = state
        .store
        .find_selection_token(token_id)
        .map_err(|err| DispatchError::Resource(err.into()))?
        .ok_or_else(|| DispatchError::BadRequest("unknown selection token".to_string()))?;

    // Atomic single-use consume, post-allow (D-050 / D-055.3). A failed
    // consume is a denial, never a re-ask.
    let consumed = state
        .store
        .try_consume_selection_token(token_id)
        .map_err(|err| DispatchError::Resource(err.into()))?;
    if !consumed {
        return Err(DispatchError::BadRequest(
            "selection token has already been used".to_string(),
        ));
    }

    let gmail = state.connectors.gmail().ok_or_else(|| {
        DispatchError::Connector(anyhow::anyhow!(
            "selection token exists but no gmail connector is configured"
        ))
    })?;
    let thread_result = gmail.fetch_thread(&token.target_id).await;
    crate::failure_surfacing::record_connector_outcome(
        &state.store,
        "gmail",
        thread_result.is_ok(),
    )
    .map_err(|err| DispatchError::Resource(err.into()))?;
    let thread = thread_result.map_err(|err| DispatchError::Connector(err.into()))?;

    Ok(json!({
        "thread_id": thread.thread_id,
        "messages": thread.messages.iter().map(|m| json!({
            "from": m.from,
            "subject": m.subject,
            "body_text": m.body_text,
        })).collect::<Vec<_>>(),
    }))
}

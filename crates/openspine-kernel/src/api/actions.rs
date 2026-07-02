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
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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

    match dispatch_allowed_action(&state, &action, bound_chat_id, body.payload.as_ref()).await {
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

/// Run the effect of one `gate()`-allowed action. Only reached after
/// `Allow` — a deny/approval-required decision never calls this.
///
/// `openspine.status.read` and `telegram.reply:owner_channel` are real;
/// `workflow.invoke:approved`, `artifact.propose`, and `setup.workflow.start`
/// are specified stubs (`tasks.md`: "Do not implement real behavior for
/// these three — a stub response is the specified deliverable"). Any other
/// allowed action (e.g. `memory.read:owner_preferences_limited`, which the
/// capability pack can grant but Step 4 implements no memory subsystem
/// for) falls through to the same honest stub shape rather than a 500 —
/// an *authorized* action must never fail the request just because its
/// kernel-side implementation doesn't exist yet.
async fn dispatch_allowed_action(
    state: &AppState,
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

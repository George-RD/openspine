//! Webhook ingest endpoint (AD-134): the headless lane's real ingress.
//!
//! `POST /v1/webhooks/{channel_account}` is the *only* kernel endpoint that
//! authenticates by HMAC signature rather than a bearer task token — a verified
//! webhook is an ordinary event source, not a shell session. The body is
//! forwarded verbatim to [`crate::pipeline::headless::run_headless_hook`],
//! which verifies the signature, drives the full governed pipeline, and either
//! completes silently (no-approval) or escalates (approval-required).

use std::str::FromStr;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::{Json, Router};
use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::pipeline::headless::{run_headless_hook, HeadlessHookOutcome, HeadlessHookRequest};
use crate::pipeline::AppState;
#[derive(Debug, Deserialize)]
pub(crate) struct WebhookIngestBody {
    /// Raw payload bytes (UTF-8/JSON); decoded into the event's raw ref.
    payload: String,
    /// `sha256=` HMAC hex tag over the bound MAC preimage.
    signature: String,
    /// Caller-supplied idempotency key; replay-protected per route.
    idempotency_key: String,
    /// RFC3339 signing time; must fall inside the verifier replay window.
    signed_at: String,
    /// The action the route's composed authority is permitted to run.
    action: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct WebhookIngestResponse {
    outcome: &'static str,
    grant_id: Option<String>,
}

pub(crate) async fn post_webhook(
    State(state): State<Arc<AppState>>,
    Path(channel_account): Path<String>,
    _headers: HeaderMap,
    Json(body): Json<WebhookIngestBody>,
) -> Result<(StatusCode, Json<WebhookIngestResponse>), (StatusCode, Json<Value>)> {
    if state.webhook_verifier.is_key_unset() {
        // Fail-closed: a production ingress without a configured HMAC key
        // must never accept webhooks (an empty key is forgeable).
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "webhook ingress is not configured"})),
        ));
    }
    let signed_at = Timestamp::from_str(&body.signed_at).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid signed_at (expect RFC3339)"})),
        )
    })?;
    let request = HeadlessHookRequest {
        payload: body.payload.into_bytes(),
        signature: body.signature,
        idempotency_key: body.idempotency_key,
        signed_at,
        channel_account,
        action: ActionId::new(&body.action),
    };
    let outcome = run_headless_hook(&state, request, Timestamp::now())
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "headless webhook failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal_error"})),
            )
        })?;
    let (status, outcome_str, grant_id) = match outcome {
        HeadlessHookOutcome::Completed(id) => (StatusCode::OK, "completed", Some(id.to_string())),
        HeadlessHookOutcome::Escalated(id) => (StatusCode::OK, "escalated", Some(id.to_string())),
        HeadlessHookOutcome::Dropped => (StatusCode::OK, "dropped", None),
        HeadlessHookOutcome::Rejected(_) => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(WebhookIngestResponse {
                    outcome: "rejected",
                    grant_id: None,
                }),
            ));
        }
    };
    Ok((
        status,
        Json(WebhookIngestResponse {
            outcome: outcome_str,
            grant_id,
        }),
    ))
}

/// Register the webhook ingest route on the kernel `Router`.
pub fn webhook_routes() -> Router<Arc<AppState>> {
    Router::new().route(
        "/v1/webhooks/{channel_account}",
        axum::routing::post(post_webhook),
    )
}

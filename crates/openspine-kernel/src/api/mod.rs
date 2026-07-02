//! The kernel's axum HTTP API (build plan 4a/4b/4c) — the *only* interface
//! the sandboxed shell ever talks to. See `docs/kernel-http-contract.md`
//! for the authoritative wire contract this module implements exactly;
//! `openspine-shell/src/client.rs` is the other side of the same contract.
//!
//! Every endpoint except `GET /v1/status` requires
//! `Authorization: Bearer <task_token>`; a missing, unknown, or expired
//! token is `403 {"error":"unauthorized"}` (D-032: "every kernel API
//! request without a valid, unexpired task token gets 403"), and the
//! kernel audits the rejection itself — callers never need to audit an
//! auth failure themselves.
//!
//! Split by endpoint to stay under the 500-line-per-file convention:
//! this file holds routing + the shared auth/error helpers every endpoint
//! uses; [`task`], [`actions`], and [`generate`] hold one endpoint each.

mod actions;
mod generate;
mod task;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod dispatch_tests;

#[cfg(test)]
mod generate_tests;

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use jiff::Timestamp;
use openspine_gate::GateContext;
use openspine_schemas::approval::ApprovalRecord;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::selection::SelectionToken;
use serde_json::{json, Value};
use ulid::Ulid;

use crate::pipeline::AppState;
use crate::store::Store;

/// [`Store`] backs [`GateContext`] directly. A DB read failure here is
/// treated as "no approval / no token found" (the safe failure mode — it
/// denies or re-asks rather than incorrectly authorizing something), not
/// propagated as an error; `gate()` itself is infallible by design and
/// this impl preserves that.
impl GateContext for Store {
    fn approval_for_request(&self, action_request_id: Ulid) -> Option<ApprovalRecord> {
        match self.find_approval_for_request(action_request_id) {
            Ok(record) => record,
            Err(err) => {
                tracing::warn!(error = %err, "approval lookup failed, treating as none");
                None
            }
        }
    }

    fn find_selection_token(&self, id: Ulid) -> Option<SelectionToken> {
        match self.find_selection_token(id) {
            Ok(token) => token,
            Err(err) => {
                tracing::warn!(error = %err, "selection token lookup failed, treating as none");
                None
            }
        }
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/status", get(get_status))
        .route("/v1/task", get(task::get_task))
        .route("/v1/actions", post(actions::post_actions))
        .route("/v1/model/generate", post(generate::post_model_generate))
        .with_state(state)
}

pub(crate) fn unauthorized() -> (StatusCode, Json<Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(json!({"error": "unauthorized"})),
    )
}

pub(crate) fn internal_error(err: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    tracing::error!(error = %err, "kernel API internal error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": "internal_error"})),
    )
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

/// Resolve and validate the calling task grant from `Authorization: Bearer
/// <task_token>`. Every rejection path is audited before returning `403`
/// (contract: "an audit row is still appended by the kernel").
pub(crate) async fn authenticate(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(TaskGrant, ArtifactRef, i64), (StatusCode, Json<Value>)> {
    let Some(token) = bearer_token(headers) else {
        let _ = state.store.append_audit(
            "auth.rejected",
            None,
            None,
            Some("missing_token"),
            None,
            &[],
            &[],
        );
        return Err(unauthorized());
    };

    let found = state
        .store
        .find_task_grant_by_token(token)
        .map_err(internal_error)?;
    let Some((grant, pending_ref, bound_chat_id)) = found else {
        let _ = state.store.append_audit(
            "auth.rejected",
            None,
            None,
            Some("unknown_token"),
            None,
            &[],
            &[],
        );
        return Err(unauthorized());
    };

    if grant.is_expired(Timestamp::now()) {
        let _ = state.store.append_audit(
            "auth.rejected",
            None,
            None,
            Some("expired_token"),
            Some(grant.id),
            &[],
            &[],
        );
        return Err(unauthorized());
    }

    Ok((grant, pending_ref, bound_chat_id))
}

// ---- GET /v1/status -------------------------------------------------------

async fn get_status(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "uptime_seconds": state.started_at.elapsed().as_secs(),
    }))
}

//! `GET /v1/task` — the redacted task-grant view (build plan 4a/4d).

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Serialize;
use serde_json::Value;
use ulid::Ulid;

use super::{authenticate, internal_error};
use crate::pipeline::AppState;

#[derive(Debug, Serialize)]
pub(super) struct TaskLimitsBody {
    max_model_calls: u32,
    max_artifacts: u32,
    max_runtime_seconds: u64,
}

#[derive(Debug, Serialize)]
pub(super) struct TaskViewBody {
    task_grant_id: String,
    agent_id: String,
    workflow_id: String,
    purpose: String,
    allowed_actions: Vec<String>,
    approval_required_actions: Vec<String>,
    denied_actions: Vec<String>,
    output_channels: Vec<String>,
    limits: TaskLimitsBody,
    expires_at: String,
    pending_message: String,
    /// Build plan Step 5: which selection token(s) this grant may spend
    /// (PRD §15 — "only usable inside matching task grant"). Empty for
    /// every Phase 1 grant; populated by `pipeline::handle_thread_selection`
    /// for a selected-thread email task.
    selection_tokens: Vec<String>,
}

pub(super) async fn get_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<TaskViewBody>, (StatusCode, Json<Value>)> {
    let (grant, pending_ref, _bound_chat_id) = authenticate(&state, &headers).await?;
    let pending_bytes = state.artifacts.get(&pending_ref).map_err(internal_error)?;
    let pending_message = String::from_utf8_lossy(&pending_bytes).into_owned();

    Ok(Json(TaskViewBody {
        task_grant_id: grant.id.to_string(),
        agent_id: grant.agent_id,
        workflow_id: grant.workflow_id,
        purpose: grant.purpose,
        allowed_actions: grant.allowed_actions.into_iter().map(|a| a.0).collect(),
        approval_required_actions: grant
            .approval_required_actions
            .into_iter()
            .map(|a| a.0)
            .collect(),
        denied_actions: grant.denied_actions.into_iter().map(|a| a.0).collect(),
        output_channels: grant.output_channels,
        limits: TaskLimitsBody {
            max_model_calls: grant.limits.max_model_calls,
            max_artifacts: grant.limits.max_artifacts,
            max_runtime_seconds: grant.limits.max_runtime_seconds,
        },
        expires_at: grant.expires_at.to_string(),
        pending_message,
        selection_tokens: grant.selection_tokens.iter().map(Ulid::to_string).collect(),
    }))
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_channels: Option<Vec<String>>,
    limits: TaskLimitsBody,
    expires_at: String,
    pending_message: String,
    selection_tokens: Vec<String>,
}

pub(super) async fn get_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<TaskViewBody>, (StatusCode, Json<Value>)> {
    let (grant, pending_ref, _bound_chat_id) = authenticate(&state, &headers).await?;
    let pending_bytes = state.artifacts.get(&pending_ref).map_err(internal_error)?;
    let pending_message = String::from_utf8_lossy(&pending_bytes).into_owned();

    let effective_actions: Vec<String> = grant
        .allowed_actions
        .iter()
        .filter(|a| grant.effectively_allows(a))
        .map(|a| a.0.clone())
        .collect();
    let effective_approval_required: Vec<String> = grant
        .approval_required_actions
        .iter()
        .filter(|a| grant.effectively_approval_required(a))
        .map(|a| a.0.clone())
        .collect();
    let effective_denied: Vec<String> = grant
        .denied_actions
        .iter()
        .filter(|a| !grant.effectively_allows(a))
        .map(|a| a.0.clone())
        .collect();
    Ok(Json(TaskViewBody {
        task_grant_id: grant.id.to_string(),
        agent_id: grant.agent_id,
        workflow_id: grant.workflow_id,
        purpose: grant.purpose,
        allowed_actions: effective_actions,
        approval_required_actions: effective_approval_required,
        denied_actions: effective_denied,
        output_channels: if grant.parent_grant_id.is_some() {
            None
        } else {
            Some(grant.output_channels)
        },
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

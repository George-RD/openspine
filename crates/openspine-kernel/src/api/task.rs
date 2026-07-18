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
    /// Worker grants receive an explicit empty list. The shell wire contract
    /// requires this field, while the empty value preserves the worker's
    /// structural no-direct-egress boundary.
    output_channels: Option<Vec<String>>,
    /// Explicit identity marker: only commissioned sub-grants are workers.
    is_worker: bool,
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
            Some(Vec::new())
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
        is_worker: grant.parent_grant_id.is_some(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::extract::State;
    use axum::http::header::{HeaderValue, AUTHORIZATION};

    use crate::store::worker_dispatch::record_worker_commissioned;
    use crate::store::Store;
    use crate::telegram::TelegramConnector;
    use crate::test_support::fixtures::build_state_with_store;

    use jiff::Timestamp;
    use openspine_schemas::action::ActionId;
    use openspine_schemas::artifact::Lifecycle;
    use openspine_schemas::briefcase::{Briefcase, CounterpartyRef, TaskClass, TaskShape};
    use openspine_schemas::digest::Digest;
    use openspine_schemas::grant::{GrantLimits, GrantMode, TaskGrant};
    use openspine_schemas::worker::WorkerIdentity;
    use ulid::Ulid;

    /// A worker sub-grant: a real `parent_grant_id` (so `get_task` treats it
    /// as a commissioned worker) but no parent row need exist — the grant is
    /// stored as opaque `grant_json`, with no foreign-key constraint.
    fn worker_grant() -> TaskGrant {
        let now = Timestamp::now();
        let mut g = TaskGrant {
            id: Ulid::new(),
            schema_version: 1,
            lifecycle_state: Lifecycle::Active,
            user: "owner".to_string(),
            purpose: "w".to_string(),
            issued_by: "kernel".to_string(),
            issued_at: now,
            expires_at: now + std::time::Duration::from_secs(600),
            event_id: Ulid::new(),
            route_id: "owner_telegram_main_assistant".to_string(),
            agent_id: "main_assistant_agent".to_string(),
            workflow_id: "owner_control_conversation".to_string(),
            capability_pack_id: "owner_control_basic_pack".to_string(),
            authority_sources: vec![],
            selection_tokens: vec![],
            allowed_actions: vec![ActionId::new("worker.report_result")],
            approval_required_actions: vec![],
            denied_actions: vec![],
            allowed_egress_classes: vec![],
            output_channels: vec![],
            limits: GrantLimits {
                max_model_calls: 8,
                max_artifacts: 20,
                max_runtime_seconds: 120,
            },
            task_token: "worker-unit-token".to_string(),
            root_grant_id: Ulid::new(),
            parent_grant_id: Some(Ulid::new()),
            mode: GrantMode::Live,
            chain: vec![],
            caveat_mac: String::new(),
            thread_id: None,
        };
        g.root_grant_id = g.id;
        g
    }

    fn briefcase() -> Briefcase {
        Briefcase {
            schema_version: 1,
            task_shape: TaskShape {
                route_id: "owner_telegram_main_assistant".to_string(),
                workflow_id: "owner_control_conversation".to_string(),
                counterparty: CounterpartyRef::Unresolved {
                    channel: "worker".to_string(),
                    identifier: "worker-1".to_string(),
                },
            },
            source_snapshot_id: Digest::parse(format!("sha256:{}", "0".repeat(64))).unwrap(),
            depth: 1,
            tier: openspine_schemas::briefcase::RelationshipTier::Stranger,
            class: TaskClass::Conversation,
            sections: vec![],
            top_up_log: vec![],
        }
    }

    #[tokio::test]
    async fn get_task_emits_empty_output_channels_for_worker_grant() {
        let store = Store::open_in_memory().unwrap();
        let state = Arc::new(build_state_with_store(
            store,
            TelegramConnector::new("test-token".to_string()),
            None,
        ));
        let grant = worker_grant();
        let pending = state.artifacts.put(b"w").unwrap();
        let token_ref = state.artifacts.put(b"worker-token").unwrap();
        record_worker_commissioned(
            &state.store,
            grant.parent_grant_id.unwrap(),
            &grant,
            &pending,
            &token_ref,
            state.owner_user_id,
            &briefcase(),
            "task-view-dispatch",
            &Digest::parse(format!("sha256:{}", "1".repeat(64))).unwrap(),
            &WorkerIdentity {
                owner: "owner".to_string(),
                conversation: "task-view".to_string(),
                task: grant.id.to_string(),
            },
            "telegram_owner_bot",
        )
        .unwrap();

        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", grant.task_token)).unwrap(),
        );
        let res = get_task(State(state.clone()), headers).await;
        let body = match res {
            Ok(Json(b)) => b,
            Err((_, j)) => panic!("get_task failed: {j:?}"),
        };
        assert!(body.is_worker);
        assert_eq!(
            body.output_channels,
            Some(vec![]),
            "worker grant must serialize output_channels as [] (not null) \
             so openspine-shell's TaskView (Vec<String>) deserializes"
        );
    }
}

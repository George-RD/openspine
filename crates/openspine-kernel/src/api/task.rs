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
use openspine_authority::{project_catalog, CatalogEntryStatus, CatalogView};
use openspine_schemas::action::ToolDescriptor;

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
    /// The capability-derived tool catalog (spec #209, IT3): the exact set of
    /// model-consumable tool descriptors this grant carries, projected fresh
    /// from `(grant, action_catalog)` by `openspine_authority::project_catalog`.
    /// Denied and ungranted actions are structurally absent — never a name,
    /// description, or schema. The catalog is advisory (attenuation before
    /// inference); `gate()` remains the sole enforcement point.
    catalog: CatalogViewBody,
}

/// Wire projection of a [`CatalogView`]. The projection types in
/// `openspine-authority` deliberately carry no serde derives (#211 ruling);
/// serialization belongs here at the HTTP boundary (#212 ruling). This DTO
/// reuses the already-serializable, kernel-owned [`ToolDescriptor`] verbatim
/// and adds only the grant-derived `action_id` and projection `status`.
#[derive(Debug, Serialize)]
pub(super) struct CatalogToolBody {
    /// The action id a worker POSTs to `/v1/actions` to invoke this tool.
    action_id: String,
    /// `"callable"` (directly invocable) or `"requires_owner_approval"`
    /// (proposable; the effect pauses for owner approval via the gate flow).
    status: &'static str,
    /// The kernel-owned, model-facing descriptor (name, description, parameter
    /// JSON Schema, presentation flags).
    descriptor: ToolDescriptor,
}

/// The projected tool surface served on `GET /v1/task`. An empty `tools` list
/// is a valid catalog (an empty grant projects no tools).
#[derive(Debug, Serialize)]
pub(super) struct CatalogViewBody {
    tools: Vec<CatalogToolBody>,
}

impl CatalogViewBody {
    /// Serialize a projected [`CatalogView`] into its wire form, preserving the
    /// projection's grant-derived order.
    ///
    /// `effective_allowed` / `effective_approval` are the chain-effective id
    /// sets already computed for the sibling `allowed_actions` /
    /// `approval_required_actions` fields (`TaskGrant::effectively_allows` /
    /// `effectively_approval_required`). A `CatalogEntry` is emitted only if its
    /// id survives that same attenuation, so a chain-denied action can never
    /// leak a name, description, or schema — keeping the catalog exactly
    /// consistent with the grant's redacted permission lists (#212).
    fn from_view(
        view: &CatalogView,
        effective_allowed: &[String],
        effective_approval: &[String],
    ) -> Self {
        let tools = view
            .entries()
            .iter()
            .filter(|entry| {
                let effective = match entry.status {
                    CatalogEntryStatus::Callable => effective_allowed,
                    CatalogEntryStatus::RequiresOwnerApproval => effective_approval,
                };
                effective.iter().any(|id| id == &entry.action_id.0)
            })
            .map(|entry| CatalogToolBody {
                action_id: entry.action_id.0.clone(),
                status: match entry.status {
                    CatalogEntryStatus::Callable => "callable",
                    CatalogEntryStatus::RequiresOwnerApproval => "requires_owner_approval",
                },
                descriptor: entry.descriptor.clone(),
            })
            .collect();
        Self { tools }
    }
}

pub(super) async fn get_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<TaskViewBody>, (StatusCode, Json<Value>)> {
    let (grant, pending_ref, _owner_surface) = authenticate(&state, &headers).await?;
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
    // Project the model-consumable tool catalog fresh from this grant (spec
    // #209, IT3). No persisted session, no cached catalog: it is recomputed
    // per request from `(grant, action_catalog)`, filtered to the same
    // chain-effective ids as the redacted permission lists above.
    let catalog = CatalogViewBody::from_view(
        &project_catalog(&grant, &state.action_catalog),
        &effective_actions,
        &effective_approval_required,
    );
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
        catalog,
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
    /// stored as opaque `grant_json`, with no foreign-key constraint. The
    /// bearer `task_token` and the `allowed_actions` are caller-chosen so a
    /// test can persist several distinguishable grants in one store.
    fn worker_grant(task_token: &str, allowed_actions: &[&str]) -> TaskGrant {
        let now = Timestamp::now();
        let mut g = TaskGrant {
            persona_id: None,
            id: Ulid::new(),
            schema_version: 1,
            lifecycle_state: Lifecycle::Active,
            user: Ulid::new().into(),
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
            allowed_actions: allowed_actions.iter().map(|a| ActionId::new(*a)).collect(),
            approval_required_actions: vec![],
            denied_actions: vec![],
            allowed_egress_classes: vec![],
            output_channels: vec![],
            limits: GrantLimits {
                max_model_calls: 8,
                max_artifacts: 20,
                max_runtime_seconds: 120,
            },
            task_token: task_token.to_string(),
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

    /// Persist a worker sub-grant so `authenticate` resolves its bearer
    /// `task_token`. `dispatch_key` and `digest_fill` are caller-chosen so
    /// several grants can be commissioned into one store without collision.
    fn commission(state: &AppState, grant: &TaskGrant, dispatch_key: &str, digest_fill: &str) {
        let pending = state.artifacts.put(b"w").unwrap();
        let token_ref = state.artifacts.put(b"worker-token").unwrap();
        record_worker_commissioned(
            &state.store,
            grant.parent_grant_id.unwrap(),
            grant,
            &pending,
            &token_ref,
            &state.telegram_owner_surface(),
            &briefcase(),
            dispatch_key,
            &Digest::parse(format!("sha256:{}", digest_fill.repeat(64))).unwrap(),
            &WorkerIdentity {
                owner: "owner".to_string(),
                conversation: "task-view".to_string(),
                task: grant.id.to_string(),
            },
            "telegram_owner_bot",
        )
        .unwrap();
    }

    /// Drive the real `get_task` handler for a bearer token, returning the
    /// projected view (or panicking on the `403`/`500` error path).
    async fn fetch(state: &Arc<AppState>, token: &str) -> TaskViewBody {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        match get_task(State(state.clone()), headers).await {
            Ok(Json(b)) => b,
            Err((_, j)) => panic!("get_task failed: {j:?}"),
        }
    }

    #[tokio::test]
    async fn get_task_emits_empty_output_channels_for_worker_grant() {
        let state = Arc::new(build_state_with_store(
            Store::open_in_memory().unwrap(),
            TelegramConnector::new("test-token".to_string()),
            None,
        ));
        let grant = worker_grant("worker-unit-token", &["worker.report_result"]);
        commission(&state, &grant, "task-view-dispatch", "1");

        let body = fetch(&state, &grant.task_token).await;
        assert!(body.is_worker);
        assert_eq!(
            body.output_channels,
            Some(vec![]),
            "worker grant must serialize output_channels as [] (not null) \
             so openspine-shell's TaskView (Vec<String>) deserializes"
        );
    }

    /// IT3 / invariant I2 (spec #209): a commissioned worker's `/v1/task`
    /// catalog contains exactly its grant-derived tools, and a privileged
    /// action the grant never carried is structurally absent — no name,
    /// description, or schema — asserted positively. The canonical catalog
    /// *does* carry descriptors for those privileged ids, so their absence is
    /// grant-derivation, not a missing descriptor.
    #[tokio::test]
    async fn worker_catalog_is_exactly_grant_derived_and_privileged_actions_are_absent() {
        let state = Arc::new(build_state_with_store(
            Store::open_in_memory().unwrap(),
            TelegramConnector::new("test-token".to_string()),
            None,
        ));
        let grant = worker_grant("worker-i2-token", &["worker.report_result"]);
        commission(&state, &grant, "i2-dispatch", "1");

        let body = fetch(&state, &grant.task_token).await;

        let ids: Vec<&str> = body
            .catalog
            .tools
            .iter()
            .map(|t| t.action_id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["worker.report_result"],
            "a worker sees exactly its one granted, described tool"
        );
        assert_eq!(body.catalog.tools[0].status, "callable");

        // Privileged ids the worker grant never carried — each carries a
        // descriptor in the canonical catalog, yet must be structurally absent.
        for privileged in [
            "worker.commission",
            "openspine.overlay.export",
            "email.send",
        ] {
            assert!(
                !ids.contains(&privileged),
                "privileged {privileged} must be structurally absent: {ids:?}"
            );
        }

        // Every projected tool corresponds to an action the grant allows.
        let allowed: Vec<String> = grant.allowed_actions.iter().map(|a| a.0.clone()).collect();
        for tool in &body.catalog.tools {
            assert!(
                allowed.contains(&tool.action_id),
                "catalog tool {} is not in the grant's allowed_actions",
                tool.action_id
            );
        }
    }

    /// IT3 step-up (spec #209 D7): the catalog is recomputed per grant from
    /// `(grant, action_catalog)` — there is no persisted session store and no
    /// cross-turn cached catalog. A higher-authority session and an attenuated
    /// one (the downstream shape of an identity step-up) project different
    /// catalogs; the higher one carries a tool absent from the lower. The
    /// verification→grant chain itself is unit-proved by the IT2 pure-projection
    /// tests and the identity-resolution tests; this proves the seam recomputes
    /// with no shared session state.
    #[tokio::test]
    async fn catalog_is_recomputed_per_grant_with_no_shared_session_state() {
        let state = Arc::new(build_state_with_store(
            Store::open_in_memory().unwrap(),
            TelegramConnector::new("test-token".to_string()),
            None,
        ));
        let high = worker_grant(
            "high-token",
            &["worker.report_result", "openspine.status.read"],
        );
        let low = worker_grant("low-token", &["worker.report_result"]);
        commission(&state, &high, "high-dispatch", "1");
        commission(&state, &low, "low-dispatch", "2");

        let high_first = fetch(&state, "high-token").await;
        let low_view = fetch(&state, "low-token").await;
        let high_second = fetch(&state, "high-token").await;

        let ids = |b: &TaskViewBody| {
            b.catalog
                .tools
                .iter()
                .map(|t| t.action_id.clone())
                .collect::<Vec<_>>()
        };
        let status_read = "openspine.status.read".to_string();

        // Two grants -> two different catalogs.
        assert_ne!(ids(&high_first), ids(&low_view));
        assert!(ids(&high_first).contains(&status_read));
        assert!(
            !ids(&low_view).contains(&status_read),
            "the higher-authority tool must be absent from the attenuated catalog"
        );

        // Recompute, not cache: the higher grant's catalog is identical across
        // two fetches and unaffected by the interleaved lower-grant fetch.
        assert_eq!(
            ids(&high_first),
            ids(&high_second),
            "each request recomputes the catalog fresh; no persisted/shared catalog"
        );
    }
}

//! Authenticated worker briefcase boundary.
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use jiff::Timestamp;
use openspine_gate::{gate, ActionOrigin};
use openspine_schemas::action::{ActionId, ActionRequest, GateDecision};
use openspine_schemas::briefcase::{
    SectionKind, TopUpDecision, TopUpOutcome, TopUpPolicy, TopUpRequest,
};
use serde_json::Value;
use ulid::Ulid;

use super::{authenticate, internal_error};
use crate::briefcase::SourcePool;
use crate::pipeline::AppState;

fn client_error(err: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": err.to_string()})),
    )
}

pub(crate) async fn get_briefcase(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let (grant, _, _) = authenticate(&state, &headers).await?;
    let worker_id = state
        .store
        .primary_worker_id(grant.id)
        .map_err(internal_error)?;
    let view = crate::briefcase_visibility::view_for_worker(&state.store, grant.id, worker_id)
        .map_err(internal_error)?;
    serde_json::to_value(view).map(Json).map_err(internal_error)
}
pub(crate) async fn post_topup(
    State(state): State<Arc<AppState>>,
    Path(briefcase_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<TopUpRequest>,
) -> Result<Json<TopUpDecision>, (StatusCode, Json<Value>)> {
    if request.justification.len() > 200 {
        return Err(client_error("justification exceeds 200 character limit"));
    }
    if request.section_key.len() > TopUpRequest::MAX_SECTION_KEY_BYTES {
        return Err(client_error("section key exceeds 128 byte limit"));
    }
    let (grant, _, _) = authenticate(&state, &headers).await?;
    // Validate the path id against the authenticated grant's briefcase: the
    // briefcase is keyed by task-grant id, so a mismatched id is simply not
    // this token's briefcase.
    let requested_id = Ulid::from_string(&briefcase_id).map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "unknown briefcase"})),
        )
    })?;
    if requested_id != grant.id {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "briefcase not found for this token"})),
        ));
    }
    let briefcase = state
        .store
        .find_briefcase(grant.id)
        .map_err(internal_error)?
        .ok_or_else(|| internal_error("briefcase missing"))?;
    let growth_ceiling = briefcase.depth + 2;
    let policy = TopUpPolicy::new([
        (
            (briefcase.tier, briefcase.class, SectionKind::Preference),
            growth_ceiling,
        ),
        (
            (briefcase.tier, briefcase.class, SectionKind::Skill),
            growth_ceiling,
        ),
    ])
    .with_max_total_sections([((briefcase.tier, briefcase.class), growth_ceiling)]);
    let sources = SourcePool {
        learned: state.store.list_learned_sources().map_err(internal_error)?,
    };

    // D-055: the top-up mutation is a classified, gate-visible action. The
    // kernel composes a real task-grant action request and runs the single
    // authority gate BEFORE any briefcase mutation. There is no
    // special-case around the gate — an ungranted/denied top-up never
    // reaches `apply_top_up_for_grant_atomic`.
    let topup_request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: ActionId::new("briefcase.topup"),
        target_ref: None,
        payload_ref: None,
        target_digest: None,
        selection_token_id: None,
        requested_at: Timestamp::now(),
        schema_version: 1,
    };
    let outcome = gate(
        &grant,
        &topup_request,
        ActionOrigin::Shell,
        &state.store,
        &state.action_catalog,
        &state.connectors,
        Timestamp::now(),
    );
    state
        .store
        .append_audit(
            "briefcase.topup.gated",
            Some(&topup_request.action),
            Some(&outcome.decision),
            None,
            Some(grant.id),
            &[],
            &[],
        )
        .map_err(internal_error)?;

    // Denials are durable decisions too: record the denied request and its
    // audit row atomically, so replaying the same request id is rejected.
    if !matches!(outcome.decision, GateDecision::Allow) {
        let denied = state
            .store
            .mutate_briefcase_and_audit(grant.id, |briefcase| {
                // Inside the transaction: reject a replayed request id so a
                // second identical denial cannot append a duplicate decision
                // (spec: "Replayed top-up" → no second decision recorded).
                if briefcase
                    .top_up_log
                    .iter()
                    .any(|prior| prior.request.request_id == request.request_id)
                {
                    return Err(crate::briefcase::BriefcaseKernelError::Schema(
                        openspine_schemas::briefcase::BriefcaseError::TopUpReplay(
                            request.request_id,
                        ),
                    ));
                }
                let decision = TopUpDecision {
                    request: request.for_persistence(),
                    outcome: TopUpOutcome::Denied {
                        reason: format!("gate decision: {:?}", outcome.decision),
                    },
                    source_digest: None,
                };
                briefcase.record_top_up_decision(decision.clone());
                let audit = crate::store::briefcase_support::BriefcaseAudit {
                    kind: "briefcase.topup.denied".to_string(),
                    action: Some(topup_request.action.clone()),
                    decision: Some(outcome.decision.clone()),
                    reason: Some("gate denied top-up".to_string()),
                    task_grant_id: Some(grant.id),
                    target_refs: vec![],
                    payload_refs: vec![],
                };
                Ok::<_, crate::briefcase::BriefcaseKernelError>((decision, audit))
            })
            .map_err(|error| match error {
                store_error @ crate::briefcase::BriefcaseKernelError::Store(_) => {
                    internal_error(store_error)
                }
                other => client_error(other),
            })?;
        return Ok(Json(denied));
    }

    // On allow, mutate the briefcase and chain the audit row atomically
    // inside one store transaction, then return the recorded decision.
    crate::briefcase::apply_top_up_for_grant_atomic(
        &state.store,
        grant.id,
        &request,
        &policy,
        &sources,
        &topup_request.action,
    )
    .map(Json)
    .map_err(|error| match error {
        store_error @ crate::briefcase::BriefcaseKernelError::Store(_) => {
            internal_error(store_error)
        }
        other => client_error(other),
    })
}

#[allow(dead_code)]
fn _worker_id_for_grant(grant_id: Ulid) -> Ulid {
    grant_id
}

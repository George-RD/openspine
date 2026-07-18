//! Kernel-owned plan approval callback and resolution (AD-011/D-011).
use jiff::Timestamp;
use openspine_gate::{gate, ActionOrigin};
use openspine_schemas::action::{ActionRequest, GateDecision};
use openspine_schemas::approval::{ApprovalDecision, ApprovalRecord, TimeoutBehavior};
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::plan::Plan;
use ulid::Ulid;

use super::post_approval::resolve_post_approval_handler;
use super::{notify_owner_best_effort, AppState};

const APPROVAL_TTL: std::time::Duration = std::time::Duration::from_secs(300);

/// Loads and re-derives the content-addressed plan before ApprovalRecord
/// persistence. A changed artifact cannot reach gate or resolution.
pub(super) async fn handle_plan_approval_callback(
    state: &AppState,
    chat_id: i64,
    callback_query_id: &str,
    action_request_id: Ulid,
) -> anyhow::Result<()> {
    crate::spend::guard_connector(state, true).await?;
    let answer_result = crate::api::connector_breaker::call_with_connector_preflight(
        state,
        "telegram",
        None,
        state
            .connectors
            .telegram()
            .answer_callback_query(callback_query_id),
    )
    .await;
    crate::failure_surfacing::record_callback_ack(
        state,
        answer_result.is_ok(),
        answer_result
            .as_ref()
            .err()
            .map(|e| e.to_string())
            .as_deref(),
    );
    let Some(request) = state.store.find_action_request(action_request_id)? else {
        state.store.append_audit(
            "plan.approval_unknown_request",
            None,
            None,
            Some("action_request_id not found"),
            None,
            &[],
            &[],
        )?;
        notify_owner_best_effort(state, chat_id, "That plan approval is no longer valid.").await;
        return Ok(());
    };
    let Some((grant, _, bound_chat_id)) =
        state.store.find_task_grant_by_id(request.task_grant_id)?
    else {
        state.store.append_audit(
            "plan.approval_grant_missing",
            Some(&request.action),
            None,
            Some("task grant not found"),
            None,
            &[],
            &[],
        )?;
        notify_owner_best_effort(state, chat_id, "The task behind that plan is gone.").await;
        return Ok(());
    };
    if request.action.as_str() != "plan.execute" {
        state.store.append_audit(
            "plan.approval_wrong_action",
            Some(&request.action),
            None,
            Some("approve_plan callback requires plan.execute"),
            Some(grant.id),
            &[],
            &[],
        )?;
        return Ok(());
    }
    if bound_chat_id != chat_id || !state.store.try_consume_action_request(action_request_id)? {
        state.store.append_audit(
            "plan.approval_refused",
            Some(&request.action),
            None,
            Some("channel mismatch or request already consumed"),
            Some(grant.id),
            &[],
            &[],
        )?;
        return Ok(());
    }
    let (Some(payload_ref), Some(target_digest)) =
        (request.payload_ref.as_ref(), request.target_digest.as_ref())
    else {
        state.store.append_audit(
            "plan.approval_malformed_request",
            Some(&request.action),
            None,
            Some("plan request has no payload or target digest"),
            Some(grant.id),
            &[],
            &[],
        )?;
        notify_owner_best_effort(state, chat_id, "That plan proposal is malformed.").await;
        return Ok(());
    };
    let bytes = match state.artifacts.get(payload_ref) {
        Ok(bytes) => bytes,
        Err(_) => {
            state.store.append_audit(
                "plan.approval_digest_mismatch",
                Some(&request.action),
                None,
                Some("plan artifact unavailable or content digest mismatch"),
                Some(grant.id),
                &[],
                &[],
            )?;
            notify_owner_best_effort(state, chat_id, "That plan changed and cannot be approved.")
                .await;
            return Ok(());
        }
    };
    let plan: Plan = match serde_json::from_slice(&bytes) {
        Ok(plan) => plan,
        Err(_) => {
            state.store.append_audit(
                "plan.approval_malformed_plan",
                Some(&request.action),
                None,
                Some("plan artifact is not valid Plan JSON"),
                Some(grant.id),
                &[],
                &[],
            )?;
            notify_owner_best_effort(state, chat_id, "That plan is malformed.").await;
            return Ok(());
        }
    };
    let derived_digest = plan.digest();
    if derived_digest != payload_ref.digest {
        state.store.append_audit(
            "plan.approval_digest_mismatch",
            Some(&request.action),
            None,
            Some("kernel re-derived plan digest differs from request payload"),
            Some(grant.id),
            &[],
            &[],
        )?;
        notify_owner_best_effort(state, chat_id, "That plan changed and cannot be approved.").await;
        return Ok(());
    }
    let now = Timestamp::now();
    let approval = ApprovalRecord {
        id: Ulid::new(),
        schema_version: 1,
        action_request_id: request.id,
        approved_by: state.owner_user_id.to_string(),
        approved_at: now,
        approved_payload_digest: derived_digest,
        approved_target_digest: target_digest.clone(),
        expires_at: now + APPROVAL_TTL,
        decision: ApprovalDecision::Approved,
        timeout_behavior: TimeoutBehavior::DoNothing,
        approval_channel: "telegram_inline".to_string(),
    };
    state.store.insert_approval(&approval)?;
    state.store.append_audit(
        "plan.approval_recorded",
        Some(&request.action),
        None,
        None,
        Some(grant.id),
        &[],
        &[],
    )?;
    match gate(
        &grant,
        &request,
        ActionOrigin::Shell,
        &state.store,
        &state.action_catalog,
        &state.connectors,
        now,
    )
    .decision
    {
        GateDecision::Allow => {
            resolve_post_approval_handler(&request.action)(state, &grant, &request, chat_id).await
        }
        decision => {
            state.store.append_audit(
                "plan.approval_gate_denied",
                Some(&request.action),
                Some(&decision),
                None,
                Some(grant.id),
                &[],
                &[],
            )?;
            notify_owner_best_effort(state, chat_id, "That plan approval was refused.").await;
            Ok(())
        }
    }
}

/// Records and announces an approved plan without inventing step execution.
pub(super) async fn resolve_approved_plan(
    state: &AppState,
    grant: &TaskGrant,
    request: &ActionRequest,
    chat_id: i64,
) -> anyhow::Result<()> {
    let Some(payload_ref) = request.payload_ref.as_ref() else {
        notify_owner_best_effort(state, chat_id, "Approved plan payload is missing.").await;
        return Ok(());
    };
    let bytes = state
        .artifacts
        .get(payload_ref)
        .map_err(anyhow::Error::from)?;
    let plan: Plan = serde_json::from_slice(&bytes)?;
    if plan.digest() != payload_ref.digest {
        state.store.append_audit(
            "plan.resolution_refused",
            Some(&request.action),
            None,
            Some("plan digest changed before resolution"),
            Some(grant.id),
            &[],
            &[],
        )?;
        notify_owner_best_effort(state, chat_id, "Approved plan changed; resolution refused.")
            .await;
        return Ok(());
    }
    state.store.append_audit(
        "plan.resolved",
        Some(&request.action),
        None,
        Some(plan.digest().as_str()),
        Some(grant.id),
        &[],
        std::slice::from_ref(payload_ref),
    )?;
    notify_owner_best_effort(
        state,
        chat_id,
        "Plan approved and recorded. Its steps were not executed.",
    )
    .await;
    Ok(())
}

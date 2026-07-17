//! Plan proposal endpoint dispatch and digest-bound owner preview.
use jiff::Timestamp;
use openspine_schemas::action::{ActionId, ActionRequest};
use openspine_schemas::digest::{canonical_json, digest_of};
use openspine_schemas::grant::TaskGrant;
use serde_json::{json, Value};
use ulid::Ulid;

use super::actions::DispatchError;
use super::telegram_truncate::{truncate_for_telegram, truncate_with_notice};
use crate::pipeline::AppState;

/// Persists canonical plan bytes before presenting a complete question with a
/// plan-specific callback. Truncated previews never create pending authority.
pub(crate) async fn dispatch_plan_preview(
    state: &AppState,
    grant: &TaskGrant,
    bound_chat_id: i64,
    plan: &openspine_schemas::plan::Plan,
) -> Result<Value, DispatchError> {
    let target_digest = digest_of(&json!({"kind": "plan"}));
    let question = openspine_schemas::plan::PlanApprovalQuestion::new(
        "Plan approval — does this complete plan work?",
        plan,
        target_digest,
    );
    let full = question.question.clone();
    if truncate_for_telegram(&full) != full {
        state
            .store
            .append_audit(
                "plan.proposal_refused",
                Some(&ActionId::new("plan.execute")),
                None,
                Some("preview_truncated"),
                Some(grant.id),
                &[],
                &[],
            )
            .map_err(|err| DispatchError::Resource(err.into()))?;
        let send_result = state
            .connectors
            .telegram()
            .send_reply(bound_chat_id, &truncate_with_notice(&full))
            .await;
        crate::failure_surfacing::record_connector_outcome_or_batch(
            state,
            "telegram",
            send_result.is_ok(),
        );
        send_result.map_err(DispatchError::Connector)?;
        return Ok(json!({"sent": true, "approval_offered": false}));
    }
    // D-046/D-050: a shell-initiated artifact put draws from the same
    // per-grant budget as `artifact.propose` (mirrors artifact_propose.rs).
    if !state
        .store
        .try_count_artifact_put(grant.id, grant.limits.max_artifacts)
        .map_err(|err| DispatchError::Resource(err.into()))?
    {
        return Err(DispatchError::BadRequest(
            "plan.propose budget exhausted for this task".to_string(),
        ));
    }
    let plan_json =
        serde_json::to_value(plan).map_err(|err| DispatchError::Resource(err.into()))?;
    let payload_ref = state
        .artifacts
        .put(canonical_json(&plan_json).as_bytes())
        .map_err(|err| DispatchError::Resource(err.into()))?;
    if payload_ref.digest != question.plan_digest {
        state
            .store
            .append_audit(
                "plan.proposal_refused",
                Some(&ActionId::new("plan.execute")),
                None,
                Some("persisted plan digest differs from question digest"),
                Some(grant.id),
                &[],
                &[],
            )
            .map_err(|err| DispatchError::Resource(err.into()))?;
        return Err(DispatchError::Resource(anyhow::anyhow!(
            "plan payload digest mismatch"
        )));
    }
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: ActionId::new("plan.execute"),
        target_ref: None,
        payload_ref: Some(payload_ref),
        target_digest: Some(question.target_digest),
        selection_token_id: None,
        requested_at: Timestamp::now(),
        schema_version: 1,
    };
    state
        .store
        .insert_action_request(&request)
        .map_err(|err| DispatchError::Resource(err.into()))?;
    let send_result = state
        .connectors
        .telegram()
        .send_reply_with_plan_approval_button(bound_chat_id, &full, request.id)
        .await;
    crate::failure_surfacing::record_connector_outcome_or_batch(
        state,
        "telegram",
        send_result.is_ok(),
    );
    send_result.map_err(DispatchError::Connector)?;
    Ok(json!({"sent": true, "approval_offered": true}))
}

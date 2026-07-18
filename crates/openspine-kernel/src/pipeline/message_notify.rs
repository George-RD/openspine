//! Owner-facing notification effects and durable outcome recording.

use jiff::Timestamp;
use openspine_gate::{gate, ActionOrigin};
use openspine_schemas::action::{ActionId, ActionRequest, GateDecision};
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::grant::{GrantLimits, TaskGrant};
use ulid::Ulid;

use super::AppState;
use crate::store::failure_surfacing_types::DetailReceipt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotifyOutcome {
    Sent,
    GateUnavailable,
    GateAuditFailed,
    GateDenied,
    AttemptAuditFailed,
    SendFailed,
    DeadLetterPersistFailed,
    OutcomeAuditFailed,
}

pub(crate) async fn notify_owner_with_digest(
    state: &AppState,
    chat_id: i64,
    text: &str,
    digest_item_ids: &[Ulid],
    detail: Option<&DetailReceipt>,
) -> NotifyOutcome {
    let now = Timestamp::now();
    let Some(notify_grant) = kernel_notify_grant() else {
        record_notify_skipped(state, "notify grant unavailable (HMAC key unset)");
        tracing::warn!("OPENSPINE_GRANT_HMAC_KEY unset; refusing owner.notify (fail-closed)");
        return NotifyOutcome::GateUnavailable;
    };
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: notify_grant.id,
        action: ActionId::new("owner.notify"),
        target_ref: None,
        payload_ref: None,
        target_digest: None,
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        skill_attribution: None,
        requested_at: now,
        schema_version: 1,
    };
    let outcome = gate(
        &notify_grant,
        &request,
        ActionOrigin::Kernel,
        &state.store,
        &state.action_catalog,
        &state.connectors,
        now,
    );
    if let Err(err) = state.store.append_audit(
        "action.gated",
        Some(&request.action),
        Some(&outcome.decision),
        None,
        Some(outcome.audit.task_grant_id),
        &[],
        &[],
    ) {
        tracing::error!(error = %err, "owner.notify gate audit failed; suppressing effect");
        record_notify_skipped(state, &format!("gate audit append failed: {err}"));
        return NotifyOutcome::GateAuditFailed;
    }
    let GateDecision::Allow = outcome.decision else {
        tracing::warn!(decision = ?outcome.decision, "owner.notify denied by gate");
        return NotifyOutcome::GateDenied;
    };
    if let Err(err) = state.store.append_audit(
        "owner.notify_attempted",
        Some(&request.action),
        Some(&outcome.decision),
        None,
        Some(outcome.audit.task_grant_id),
        &[],
        &[],
    ) {
        tracing::error!(error = %err, "owner.notify attempt audit failed; suppressing effect");
        record_notify_skipped(state, &format!("attempt audit append failed: {err}"));
        return NotifyOutcome::AttemptAuditFailed;
    }
    if let Err(err) = state.store.reserve_daily_connector_call(
        &crate::store::spend::utc_day(Timestamp::now()),
        i64::MAX as u64,
    ) {
        tracing::error!(error = %err, "immediate-lane daily connector reservation failed");
        if let Err(audit_err) = state.store.append_audit(
            "spend.immediate_reservation_failed",
            Some(&request.action),
            None,
            None,
            Some(outcome.audit.task_grant_id),
            &[],
            &[],
        ) {
            tracing::error!(error = %audit_err, "failed to audit immediate reservation failure");
        }
    }
    let send_result = crate::api::connector_breaker::call_with_connector(
        state,
        "telegram",
        &request.action,
        &notify_grant,
        state.connectors.telegram().send_reply(chat_id, text),
    )
    .await;
    match send_result {
        Ok(()) => {
            let result = if digest_item_ids.is_empty() {
                state
                    .store
                    .record_notify_success(outcome.audit.task_grant_id, detail)
            } else {
                state.store.record_notify_success_and_resolve(
                    outcome.audit.task_grant_id,
                    digest_item_ids,
                    detail,
                )
            };
            match result {
                Ok(()) => NotifyOutcome::Sent,
                Err(err) => {
                    tracing::error!(error = %err, "owner notification succeeded but outcome audit failed");
                    if let Err(surface_err) = crate::failure_surfacing::batch_failure(
                        state,
                        crate::failure_surfacing::FailureClass::Resource,
                        "Telegram notification outcome persistence failed",
                        &format!("Telegram notification outcome persistence failed: {err}"),
                    ) {
                        tracing::error!(error = %surface_err, "notification outcome failure surface append failed");
                    }
                    NotifyOutcome::OutcomeAuditFailed
                }
            }
        }
        Err(err) => {
            // D-012: persist the owner-facing message as an encrypted
            // artifact, not plaintext, so the DLQ row carries only its
            let text_ref = match state.artifacts.put(text.as_bytes()) {
                Ok(ref_) => ref_.digest.to_string(),
                Err(put_err) => {
                    let reason =
                        format!("artifact persistence failed; notification send error: {err:?}");
                    if let Err(digest_err) = crate::failure_surfacing::batch_failure(
                        state,
                        crate::failure_surfacing::FailureClass::Connector,
                        "owner notification artifact persistence unavailable",
                        &reason,
                    ) {
                        tracing::error!(error = %digest_err, reason = %reason, "could not batch dead-letter persistence failure");
                    }
                    if let Err(audit_err) = state.store.append_audit(
                        "owner.dead_letter_persist_failed",
                        Some(&ActionId::new("owner.notify")),
                        None,
                        None,
                        Some(outcome.audit.task_grant_id),
                        &[],
                        &[],
                    ) {
                        tracing::error!(error = %audit_err, reason = %reason, "could not record dead-letter persistence failure");
                    }
                    tracing::error!(error = %put_err, reason = %reason, "could not encrypt dead-letter text; no retry enqueued");
                    return NotifyOutcome::DeadLetterPersistFailed;
                }
            };
            if let Err(record_err) = state.store.record_notify_failure_with_digest(
                chat_id,
                &text_ref,
                outcome.audit.task_grant_id,
                &format!("{err:?}"),
                digest_item_ids,
                detail,
            ) {
                tracing::error!(error = %record_err, send_error = ?err, "owner notification failure could not be durably recorded");
                if let Err(surface_err) = crate::failure_surfacing::batch_failure(
                    state,
                    crate::failure_surfacing::FailureClass::Resource,
                    "Telegram notification failure persistence failed",
                    &format!("Telegram notification failure persistence failed: {record_err}"),
                ) {
                    tracing::error!(error = %surface_err, "notification failure surface append failed");
                }
                return NotifyOutcome::DeadLetterPersistFailed;
            }
            NotifyOutcome::SendFailed
        }
    }
}

/// Mandatory owner delivery for security escalations. Routes through the
/// same truthful [`notify_owner_with_digest`] helper as courtesy
/// notifications, so required delivery also records the `owner.notify_attempted`
/// audit, updates Telegram counters, persists an encrypted dead-letter on a
/// send failure, and only counts `Sent` as success. Unlike the courtesy path,
/// a missing grant key, gate denial, or any non-`Sent` outcome (including a
/// durable but failed `SendFailed`) is returned as an error: required
/// delivery must never be silently downgraded to success (the escalation
/// path that calls this depends on the error to avoid recording a false
/// `action.escalated`).
pub(crate) async fn notify_owner_required_outcome(
    state: &AppState,
    chat_id: i64,
    text: &str,
) -> NotifyOutcome {
    notify_owner_with_digest(state, chat_id, text, &[], None).await
}

pub(crate) async fn notify_owner_required(
    state: &AppState,
    chat_id: i64,
    text: &str,
) -> Result<(), crate::store::StoreError> {
    match notify_owner_required_outcome(state, chat_id, text).await {
        NotifyOutcome::Sent => Ok(()),
        other => Err(crate::store::StoreError::OwnerNotificationFailed(format!(
            "required owner notification did not reach Sent: {other:?}"
        ))),
    }
}

/// Record a durable `owner.notify_skipped` row for any pre-send outcome that
/// never reaches the connector (AD-138: no failed effect without a durable
/// record AND an owner-visible surface). Best-effort: a broken store cannot
/// be made durable by more store calls, so failures are only traced.
fn record_notify_skipped(state: &AppState, reason: &str) {
    if let Err(err) = state.store.append_audit(
        "owner.notify_skipped",
        Some(&ActionId::new("owner.notify")),
        None,
        Some(reason),
        None,
        &[],
        &[],
    ) {
        tracing::error!(error = %err, skip_reason = reason, "could not durably record owner.notify_skipped");
    }
}

/// Compatibility wrapper for notifications with no digest batch metadata.
pub(crate) async fn notify_owner_best_effort(state: &AppState, chat_id: i64, text: &str) {
    let _ = notify_owner_with_digest(state, chat_id, text, &[], None).await;
}

/// Synthetic grant for kernel-origin `owner.notify` (D-055.2). `gate()` with
/// `ActionOrigin::Kernel` auto-allows only the trusted-origin set. Returns
/// `None` when the HMAC key is unavailable — callers must skip the effect
/// (fail-closed), not present an unsealed grant to `gate()`.
fn kernel_notify_grant() -> Option<TaskGrant> {
    let key = crate::grant_hmac_key()?;
    let now = Timestamp::now();
    let mut grant = TaskGrant {
        id: Ulid::new(),
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user: "kernel".to_string(),
        purpose: "owner-notify".to_string(),
        issued_by: "kernel".to_string(),
        issued_at: now,
        expires_at: now + std::time::Duration::from_secs(60),
        event_id: Ulid::new(),
        route_id: "kernel_notification".to_string(),
        agent_id: "kernel".to_string(),
        workflow_id: "kernel_notification".to_string(),
        capability_pack_id: "kernel".to_string(),
        authority_sources: vec![],
        selection_tokens: vec![],
        allowed_actions: vec![],
        approval_required_actions: vec![],
        denied_actions: vec![],
        allowed_egress_classes: vec![],
        output_channels: vec![],
        limits: GrantLimits {
            max_model_calls: 0,
            max_artifacts: 0,
            max_runtime_seconds: 0,
        },
        task_token: String::new(),
        root_grant_id: Ulid::nil(),
        parent_grant_id: None,
        mode: openspine_schemas::grant::GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
        persona_id: None,
    };
    grant.seal_root(&key);
    Some(grant)
}

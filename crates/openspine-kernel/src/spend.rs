//! AD-143 global spend kill-switch gate.
//!
//! Enforces the kernel-wide per-day spend cap at the lane boundary: it blocks
//! grant composition/dispatch on any non-immediate lane once the cap is hit,
//! and routes the first breach of the day to the immediate owner-notification
//! lane (AD-138). The immediate (owner-control) lane is never blocked — it is
//! the channel the breach alert itself rides, and its usage keeps being
//! counted by the ledger (see `store::spend`).
//!
//! Every actual usage boundary performs a fail-closed atomic reservation
//! ([`crate::store::spend::reserve_daily_model_call`] /
//! [`crate::store::spend::reserve_daily_connector_call`]) that also records the
//! one-time daily breach on denial, so the required immediate owner
//! notification fires exactly once even when the cap is crossed between
//! admission and the actual call.

use crate::model_gateway::{GatewayError, ProviderClient, ResolvedPrompt};
use crate::pipeline::AppState;
use crate::store::spend::utc_day;
use jiff::Timestamp;

/// Which admission lane a call is attempting. Only `Immediate` (the
/// owner-control notification lane) survives a global cap breach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpendLane {
    Immediate,
    NonImmediate,
}

impl SpendLane {
    /// Classify an event [`Lane`](openspine_schemas::event::Lane) per AD-143/AD-138:
    /// only the owner-control lane is the immediate notification lane; every
    /// other lane (proactive, headless, external communication, scheduled, …)
    /// is paused on breach.
    pub(crate) fn from_event_lane(lane: openspine_schemas::event::Lane) -> Self {
        match lane {
            openspine_schemas::event::Lane::OwnerControl => SpendLane::Immediate,
            _ => SpendLane::NonImmediate,
        }
    }

    /// Derive admission lane from trusted grant provenance (F2).
    /// Only `owner_control_conversation` workflow is the immediate lane.
    pub(crate) fn from_grant(grant: &openspine_schemas::grant::TaskGrant) -> Self {
        if grant.workflow_id == "owner_control_conversation" {
            SpendLane::Immediate
        } else {
            SpendLane::NonImmediate
        }
    }
}

/// Error from a counted model invocation: either the provider call failed, the
/// ledger write failed (fail-closed), or the global cap denied the call.
pub(crate) enum SpendModelError {
    Denied,
    Ledger(crate::store::StoreError),
    Provider(GatewayError),
}

fn breach_message(day: &str, current_day: &str) -> String {
    if day == current_day {
        format!(
            "Daily spend cap breached on {day}; proactive and headless activity is paused until UTC midnight."
        )
    } else {
        format!(
            "Daily spend cap breach on {day} was recovered after restart; today is {current_day} and current-day admission is governed by today's cap."
        )
    }
}

/// Claim the daily breach alert outbox row and attempt to notify the owner.
/// The durable row is consumed exactly once; failures are already represented
/// by the failure-surfacing DLQ and must not rearm duplicate alerts.
pub(crate) async fn try_drain_breach_alert(state: &AppState, day: &str) {
    let claimed = match state.store.claim_daily_breach_alert(day) {
        Ok(c) => c,
        Err(err) => {
            tracing::error!(error = %err, "failed to claim daily breach alert");
            return;
        }
    };
    if !claimed {
        return;
    }
    let current_day = utc_day(Timestamp::now());
    let message = breach_message(day, &current_day);
    match crate::pipeline::notify_owner_required_outcome(state, state.owner_user_id, &message).await
    {
        crate::pipeline::NotifyOutcome::Sent | crate::pipeline::NotifyOutcome::SendFailed => {
            if let Err(err) = state.store.complete_daily_breach_alert(day) {
                tracing::error!(error = %err, "failed to complete daily breach alert");
            }
        }
        outcome => {
            tracing::warn!(
                ?outcome,
                "daily breach notification not durably delivered; rearming"
            );
            if let Err(rearm_err) = state.store.rearm_daily_breach_alert(day) {
                tracing::error!(error = %rearm_err, "failed to rearm daily breach alert");
            }
        }
    }
}
/// Startup recovery: reset stuck in-flight alerts and drain every pending day.
pub(crate) async fn recover_pending_breach_alerts(state: &AppState) {
    if let Err(err) = state.store.reset_inflight_breach_alerts() {
        tracing::error!(error = %err, "failed to reset inflight breach alerts");
    }
    let days = match state.store.pending_daily_breach_alert_days() {
        Ok(days) => days,
        Err(err) => {
            tracing::error!(error = %err, "failed to enumerate pending breach alerts");
            return;
        }
    };
    for day in days {
        try_drain_breach_alert(state, &day).await;
    }
}

/// Returns `Ok(true)` if a non-immediate dispatch/compose may proceed.
///
/// Evaluated atomically in the store (read + one-time breach mark under one
/// lock): when the global cap is reached on a non-immediate lane, records the
/// first breach of the day and emits the immediate owner notification. The
/// immediate lane is always admitted.
pub(crate) async fn admit_spend(
    state: &AppState,
    _lane: SpendLane,
    now: Timestamp,
) -> Result<bool, crate::store::StoreError> {
    if matches!(_lane, SpendLane::Immediate) {
        return Ok(true);
    }
    let cap = &state.spend_cap;
    let day = utc_day(now);
    let (admitted, _first_breach) = state.store.check_and_mark_daily_breach(
        &day,
        cap.model_calls_per_day,
        cap.connector_calls_per_day,
    )?;
    if !admitted {
        try_drain_breach_alert(state, &day).await;
    }
    Ok(admitted)
}

/// Counted model invocation: atomically reserves one global model-call unit
/// for the UTC day (fail-closed, recording the one-time breach on denial) and
/// only then performs the provider call. Shared by every kernel model
/// invocation — production `model.generate` and model-swap golden sets — so no
/// provider spend escapes the ledger (AD-143: accounting spans ALL model calls,
/// not only production-lane ones).
pub(crate) async fn counted_model_generate(
    state: &AppState,
    _lane: SpendLane,
    provider: &ProviderClient,
    prompt: &ResolvedPrompt,
) -> Result<String, SpendModelError> {
    let cap = if matches!(_lane, SpendLane::Immediate) {
        i64::MAX as u64
    } else {
        state.spend_cap.model_calls_per_day
    };
    let day = utc_day(Timestamp::now());
    let (allowed, _first_breach) = state
        .store
        .reserve_daily_model_call(&day, cap)
        .map_err(SpendModelError::Ledger)?;
    if !allowed {
        try_drain_breach_alert(state, &day).await;
        return Err(SpendModelError::Denied);
    }
    provider
        .generate(prompt)
        .await
        .map_err(SpendModelError::Provider)
}

/// Reserve one connector-call unit before an external send. `immediate` is
/// cap-exempt but reservation errors are durably audited for owner effects.
pub(crate) async fn guard_connector(state: &AppState, immediate: bool) -> anyhow::Result<()> {
    let cap = if immediate {
        i64::MAX as u64
    } else {
        state.spend_cap.connector_calls_per_day
    };
    let day = utc_day(Timestamp::now());
    let (allowed, _first_breach) = match state.store.reserve_daily_connector_call(&day, cap) {
        Ok(outcome) => outcome,
        Err(err) => {
            if immediate {
                if let Err(audit_err) = state.store.append_audit(
                    "spend.immediate_reservation_failed",
                    None,
                    None,
                    Some(&err.to_string()),
                    None,
                    &[],
                    &[],
                ) {
                    tracing::error!(error = %audit_err, "failed to audit immediate spend reservation");
                }
            }
            return Err(anyhow::Error::from(err));
        }
    };
    if allowed {
        return Ok(());
    }
    if immediate {
        return Ok(());
    }
    try_drain_breach_alert(state, &day).await;
    anyhow::bail!("daily connector spend cap exceeded")
}

/// Reserve one connector-call unit before an external send. Grant-bound
/// dispatches derive the immediate exemption from trusted grant provenance.
pub(crate) async fn guard_connector_for(
    state: &AppState,
    grant: &openspine_schemas::grant::TaskGrant,
) -> anyhow::Result<()> {
    let immediate = grant.workflow_id == "owner_control_conversation";
    guard_connector(state, immediate).await
}

#[cfg(test)]
mod tests {
    use super::breach_message;

    #[test]
    fn prior_day_recovery_message_is_truthful() {
        let message = breach_message("2026-07-16", "2026-07-17");
        assert!(message.contains("2026-07-16"));
        assert!(message.contains("today is 2026-07-17"));
        assert!(!message.contains("paused until UTC midnight"));
    }
}

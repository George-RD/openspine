//! Worker result consumer (AD-035 / AD-030 / D-073).
//!
//! Worker output is a durable `worker.result` bus event. The master-side
//! consumer resolves the commissioning parent grant and relays through that
//! grant's separately gated reply action; it never synthesizes a kernel-origin
//! `owner.notify` effect.
//!
//! Durability contract (closes the final worker-runtime blockers):
//! * Every relay is fenced by a marker keyed on the `worker.result` event id,
//!   claimed in a `BEGIN IMMEDIATE` transaction *before* the provider call — so
//!   a restarted consumer reuses the same event-id handoff key and never
//!   advances the checkpoint past an event it has not confirmed (marker check
//!   before send).
//! * The `delivered`/`skipped`/`dead_letter` marker and the consumer checkpoint
//!   advance are committed in the SAME `BEGIN IMMEDIATE` transaction, after the
//!   confirmed handoff. A crash before the post-send commit leaves the marker
//!   `attempting`/`pending` and the checkpoint UNADVANCED, so the next run
//!   re-drives the event rather than silently skipping it.
//! * A pre-existing terminal marker (`delivered`/`skipped`/`dead_letter`) makes
//!   the pre-send check skip the send entirely, so an event already handled on
//!   a prior run is never relayed twice on replay (idempotent replay).
//! * Transient relay errors retry up to 5 attempts (durable attempt counter);
//!   the 5th failure becomes a dead-letter row + audit receipt and only then
//!   advances the checkpoint. A non-final failure stops the replay loop so a
//!   later event can never be checkpointed over a still-pending earlier one.
//! * Residual crash window (provider accepted the send, process died before
//!   the marker commit): delivery-unknown, MAY resend — exactly-once is NOT
//!   claimed (D-071). The marker guarantees no *silent skip*, not no *possible
//!   resend*; receiver-side idempotency is out of scope (Telegram SendMessage
//!   has no client idempotency key), matching nerve/failure-surfacing policy.
//! * Startup recovery of dispatched workers is receipt-guarded (see
//!   `store::worker_dispatch`): `pending_worker_dispatches` selects only
//!   `state='dispatched'` rows, so a result-recorded (terminal) row is excluded
//!   and never rerun; a re-driven dispatched row converges on the D-083
//!   terminal flip, which rejects a second `worker.result` for that dispatch.
use openspine_schemas::action::{ActionId, GateDecision};
use openspine_schemas::audit::{AuditEvent, AuditKind};
use openspine_schemas::event_bus::{ConsumerCheckpoint, EventSubscriptionFilter};
use openspine_schemas::worker::{WorkerOutcome, WorkerResult};

use crate::api::actions::{mediate_and_dispatch_action, FailureSurface};
use crate::pipeline::AppState;
use crate::store::event_bus::PersistedConsumerState;
use crate::store::worker_dispatch::worker_parent_grant;
use crate::store::worker_result_relay::WorkerRelayClaim;

const CONSUMER_ID: &str = "worker_result_consumer";
const RELAY_ACTION: &str = "telegram.reply:owner_channel";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelayOutcome {
    Delivered,
    /// A malformed or structurally unauthorized event was durably recorded as
    /// skipped; it is safe to advance without pretending an external send.
    TerminalSkip,
}

fn worker_result_filter() -> EventSubscriptionFilter {
    EventSubscriptionFilter::kinds([AuditKind::from_static("worker.result")])
}

/// Load the checkpoint without a fallback on parse/load failure. Resetting to
/// zero would replay prior events and duplicate owner sends; a corrupt state
/// must surface rather than silently reset.
fn load_checkpoint(state: &AppState) -> anyhow::Result<ConsumerCheckpoint> {
    let expected_filter = worker_result_filter();
    match state.store.load_consumer_checkpoint(CONSUMER_ID) {
        Ok(Some(saved)) if saved.filter == expected_filter => Ok(saved.checkpoint),
        Ok(Some(_)) => Err(anyhow::anyhow!(
            "worker result consumer checkpoint filter mismatch"
        )),
        Ok(None) => Ok(ConsumerCheckpoint::default()),
        Err(err) => Err(anyhow::anyhow!(
            "worker result consumer checkpoint load failed: {err}"
        )),
    }
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn worker_result_summary(payload_json: &str) -> String {
    match serde_json::from_str::<WorkerResult>(payload_json) {
        Ok(result) => {
            let outcome = match result.outcome {
                WorkerOutcome::Completed => "completed",
                WorkerOutcome::Failed => "failed",
                WorkerOutcome::Awaiting => "awaiting",
            };
            let mut parts = vec![format!("Worker result: {outcome}")];
            if !result.offered_slots.is_empty() {
                let slots = result
                    .offered_slots
                    .iter()
                    .take(8)
                    .map(|slot| {
                        format!(
                            "{} ({})",
                            bounded_text(&slot.id, 64),
                            bounded_text(&slot.label, 128)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                parts.push(format!("slots: {slots}"));
            }
            if !result.requests.is_empty() {
                let requests = result
                    .requests
                    .iter()
                    .take(8)
                    .map(|request| {
                        let kind = bounded_text(&request.kind, 128);
                        request
                            .detail_ref
                            .as_ref()
                            .map(|detail| format!("{kind} [{}]", detail.digest.as_str()))
                            .unwrap_or(kind)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                parts.push(format!("requests: {requests}"));
            }
            bounded_text(&parts.join("; "), 2048)
        }
        Err(_) => "worker result received".to_string(),
    }
}
/// Resolve the relay context (chat_id, text) for a worker.result event.
/// Returns None for TerminalSkip (structural denial that should not be
/// surfaced as a dead-letter).
fn resolve_relay_context(
    state: &AppState,
    event: &AuditEvent,
) -> anyhow::Result<Option<(i64, String)>> {
    let Some(worker_grant_id) = event.task_grant_id else {
        return Ok(None);
    };
    let Some(parent_grant_id) = worker_parent_grant(&state.store, worker_grant_id)? else {
        return Ok(None);
    };
    let Some((parent_grant, _pending_ref, bound_chat_id)) =
        state.store.find_task_grant_by_id(parent_grant_id)?
    else {
        return Ok(None);
    };
    let _ = parent_grant; // used only to verify the grant exists
    let text = event
        .payload_json
        .as_deref()
        .map(worker_result_summary)
        .unwrap_or_else(|| "worker result received".to_string());
    Ok(Some((bound_chat_id, text)))
}

/// Relay through the master agent's own gated reply path. Errors before a
/// confirmed handoff are returned so the caller retains the checkpoint for
/// retry; terminal structural skips are durably audited by the caller before
/// acknowledgement.
async fn relay_one(state: &AppState, event: &AuditEvent) -> anyhow::Result<RelayOutcome> {
    let Some(worker_grant_id) = event.task_grant_id else {
        state.store.append_audit(
            "worker.result.relay_skipped",
            None,
            None,
            Some("event has no task_grant_id"),
            None,
            &[],
            &[],
        )?;
        return Ok(RelayOutcome::TerminalSkip);
    };
    let Some(parent_grant_id) = worker_parent_grant(&state.store, worker_grant_id)? else {
        state.store.append_audit(
            "worker.result.relay_skipped",
            None,
            None,
            Some("worker has no parent grant"),
            Some(worker_grant_id),
            &[],
            &[],
        )?;
        return Ok(RelayOutcome::TerminalSkip);
    };
    let Some((parent_grant, _pending_ref, bound_chat_id)) =
        state.store.find_task_grant_by_id(parent_grant_id)?
    else {
        state.store.append_audit(
            "worker.result.relay_skipped",
            None,
            None,
            Some("parent grant not found"),
            Some(parent_grant_id),
            &[],
            &[],
        )?;
        return Ok(RelayOutcome::TerminalSkip);
    };

    let text = event
        .payload_json
        .as_deref()
        .map(worker_result_summary)
        .unwrap_or_else(|| "worker result received".to_string());
    let payload = serde_json::json!({"text": text});
    match mediate_and_dispatch_action(
        state,
        &parent_grant,
        ActionId::new(RELAY_ACTION),
        bound_chat_id,
        Some(&payload),
        FailureSurface::Detached,
    )
    .await
    {
        Ok((GateDecision::Allow, _, Some(_))) => Ok(RelayOutcome::Delivered),
        Ok((GateDecision::Allow, _, None)) => Err(anyhow::anyhow!(
            "worker result relay dispatch failed after gate allow"
        )),
        Ok((decision, _, _)) => {
            state.store.append_audit(
                "worker.result.relay_denied",
                Some(&ActionId::new(RELAY_ACTION)),
                Some(&decision),
                Some("master grant not authorized for worker result relay"),
                Some(parent_grant_id),
                &[],
                &[],
            )?;
            Ok(RelayOutcome::TerminalSkip)
        }
        Err(err) => Err(anyhow::anyhow!(
            "worker result relay not delivered: {err:?}"
        )),
    }
}
pub(crate) async fn worker_result_consumer_iteration(state: &AppState) -> anyhow::Result<()> {
    let filter = worker_result_filter();
    let checkpoint = load_checkpoint(state)?;
    let entries = state
        .store
        .replay_audit(&filter, checkpoint.last_acked_global_seq)?;
    for entry in entries {
        let event_id = entry.event.id;
        let global_seq = entry.global_seq;
        let task_grant_id = entry.event.task_grant_id;

        // Marker check BEFORE send: claim (or observe) the durable relay row
        // for this event id under BEGIN IMMEDIATE. A terminal marker from a
        // prior run means the checkpoint was already advanced — skip.
        let WorkerRelayClaim::Send { attempt } =
            state
                .store
                .claim_worker_result_relay(event_id, global_seq, task_grant_id)?
        else {
            continue;
        };

        // Resolve relay context early so we have chat_id/text for dead-letter
        // notification even if the relay itself fails.
        let relay_ctx = resolve_relay_context(state, &entry.event)?;

        let marker_state = PersistedConsumerState {
            schema_version: 1,
            checkpoint: ConsumerCheckpoint {
                schema_version: 1,
                last_acked_global_seq: global_seq,
            },
            filter: filter.clone(),
        };

        match relay_one(state, &entry.event).await {
            Ok(RelayOutcome::Delivered) => {
                state
                    .store
                    .complete_worker_result_relay(event_id, global_seq, &marker_state)?;
            }
            Ok(RelayOutcome::TerminalSkip) => {
                state
                    .store
                    .skip_worker_result_relay(event_id, global_seq, &marker_state)?;
            }
            Err(_) => {
                // The relay failed transiently. If we cannot resolve the owner
                // chat (relay_ctx None) the event is not dead-letterable with a
                // resolvable owner, so leave it retryable rather than crash or
                // silently skip.
                let Some((chat_id, text)) = relay_ctx else {
                    tracing::error!(
                        %event_id,
                        "worker result relay failed but owner context is unresolvable; leaving retryable"
                    );
                    return Ok(());
                };
                // A zero owner chat is unresolvable: dead-lettering would omit
                // the owner notification yet still commit the dead-letter and
                // advance the checkpoint, permanently hiding the result. Treat
                // as retryable instead.
                if chat_id == 0 {
                    tracing::error!(
                        %event_id,
                        "worker result relay failed with unresolvable owner chat (0); leaving retryable"
                    );
                    return Ok(());
                }
                // If artifact put fails, do NOT dead-letter — treat as
                // retryable so the next iteration retries the relay.
                let text_ref = match state.artifacts.put(text.as_bytes()) {
                    Ok(r) => r.digest.as_str().to_string(),
                    Err(e) => {
                        tracing::error!(error = %e, "storing dead-letter notification text; retrying");
                        return Ok(());
                    }
                };
                let dead_lettered = state.store.fail_worker_result_relay(
                    event_id,
                    global_seq,
                    attempt,
                    task_grant_id,
                    chat_id,
                    &text_ref,
                    &marker_state,
                )?;
                if dead_lettered {
                    continue;
                }
                // Non-final transient failure: leave the checkpoint unadvanced
                // and STOP the loop so no later event is checkpointed over this
                // still-pending one. The next run retries from here.
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Run until the first unrecoverable checkpoint/relay error, surfacing it to
/// startup rather than silently resetting and replaying from sequence zero.
pub(crate) async fn run_worker_result_consumer(state: &AppState) -> anyhow::Result<()> {
    loop {
        worker_result_consumer_iteration(state).await?;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
#[path = "worker_result_consumer_tests.rs"]
mod tests;

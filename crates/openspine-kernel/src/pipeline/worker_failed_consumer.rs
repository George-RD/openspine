//! Durable consumer for structured `worker.failed` events.
//!
//! The commission handler surfaces normal connector-cap exhaustion directly.
//! This consumer owns the legacy/unbound path: it routes the structured event
//! to the failure-surfacing lane, then advances a fail-closed checkpoint.

use crate::failure_surfacing::{notify_immediate_failure, FailureClass};
use crate::pipeline::AppState;
use crate::store::event_bus::PersistedConsumerState;
use openspine_schemas::audit::{AuditEvent, AuditKind};
use openspine_schemas::event_bus::{ConsumerCheckpoint, EventSubscriptionFilter};
use serde_json::Value;

const CONSUMER_ID: &str = "worker_failed_consumer";

fn worker_failed_filter() -> EventSubscriptionFilter {
    EventSubscriptionFilter::kinds([AuditKind::from_static("worker.failed")])
}

fn load_checkpoint(state: &AppState) -> anyhow::Result<ConsumerCheckpoint> {
    let filter = worker_failed_filter();
    match state.store.load_consumer_checkpoint(CONSUMER_ID) {
        Ok(Some(saved)) if saved.filter == filter => Ok(saved.checkpoint),
        Ok(Some(_)) => Err(anyhow::anyhow!(
            "worker failure consumer checkpoint filter mismatch"
        )),
        Ok(None) => Ok(ConsumerCheckpoint::default()),
        Err(error) => Err(anyhow::anyhow!(
            "worker failure consumer checkpoint load failed: {error}"
        )),
    }
}

async fn route_failure(state: &AppState, event: &AuditEvent) -> anyhow::Result<()> {
    let Some(payload) = event.payload_json.as_deref() else {
        return Ok(());
    };
    let value: Value = serde_json::from_str(payload)?;
    let connector_bound = value
        .get("connector")
        .is_some_and(|connector| !connector.is_null());
    let cap_exhausted = value
        .get("recomposition_permitted")
        .and_then(Value::as_bool)
        .is_some_and(|permitted| !permitted);
    // The synchronous handler already surfaces connector-bound cap exhaustion.
    // The consumer owns ordinary failures and all legacy unbound failures.
    if connector_bound && cap_exhausted {
        return Ok(());
    }
    let Some(grant_id) = event.task_grant_id else {
        return Ok(());
    };
    let Some((_, _, chat_id)) = state.store.find_task_grant_by_id(grant_id)? else {
        return Ok(());
    };
    let summary = if connector_bound {
        "worker failed; continuation requires normal re-composition"
    } else {
        "worker failure cannot be recomposed: connector identity is unbound"
    };
    notify_immediate_failure(state, chat_id, FailureClass::Escalation, summary).await?;
    Ok(())
}

pub(crate) async fn worker_failed_consumer_iteration(state: &AppState) -> anyhow::Result<()> {
    let filter = worker_failed_filter();
    let checkpoint = load_checkpoint(state)?;
    for entry in state
        .store
        .replay_audit(&filter, checkpoint.last_acked_global_seq)?
    {
        route_failure(state, &entry.event).await?;
        state.store.save_consumer_checkpoint(
            CONSUMER_ID,
            &PersistedConsumerState {
                schema_version: 1,
                checkpoint: ConsumerCheckpoint {
                    schema_version: 1,
                    last_acked_global_seq: entry.global_seq,
                },
                filter: filter.clone(),
            },
        )?;
    }
    Ok(())
}

pub(crate) async fn run_worker_failed_consumer(state: &AppState) -> anyhow::Result<()> {
    loop {
        worker_failed_consumer_iteration(state).await?;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

//! Task-board durable scheduled-dispatch consumer (D-082..D-084).
//!
//! Drains `workflow.timer_fired` events and ready dependency wakes through
//! the ordinary `scheduled_internal_lane` pipeline, correlating each fired
//! timer/wake to its task and composing exactly one grant per durable event.

use jiff::Timestamp;
use openspine_schemas::grant::TaskGrant;
use ulid::Ulid;

use super::driver::run_pipeline;
use super::lanes::{scheduled_internal_lane, EventInputs};
use super::AppState;
use crate::store::task_board::{DependencyWake, TimerDispatchRecord, TimerDispatchState};

/// Outcome of dispatching one durable timer event through the scheduled
/// pipeline. Drives the consumer's ack/retry decision (finding 2).
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum TimerDispatchOutcome {
    /// Grant persisted; the timer event is durably handled — ack it.
    Delivered { grant: Box<TaskGrant> },
    /// Permanently non-actionable (non-task timer, terminal task, duplicate
    /// event, unknown owner, unmet dependency): ack it without retry.
    AckSkip,
    /// Transiently unresolvable (authority denial, store error): withhold the
    /// checkpoint so the consumer retries and surfaces the stuck event.
    Retry,
}

/// Dispatch one durable `workflow.timer_fired` task event through the normal
/// scheduled lane. The timer driver only fires and appends the event; this
/// consumer correlates the event to a task and lets the ordinary pipeline
async fn reattempt_handed_off(
    state: &AppState,
    record: &TimerDispatchRecord,
) -> anyhow::Result<TimerDispatchOutcome> {
    if state.store.dispatch_receipt_exists(&record.event_id)? {
        return Ok(TimerDispatchOutcome::AckSkip);
    }
    let related_id = record
        .grant_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| record.event_id.clone());
    state.store.mark_dispatch_terminal(
        &record.event_id,
        &record.timer_id,
        record.task_id,
        "receiptless_handoff_refused",
        &related_id,
    )?;
    state.store.append_audit(
        "task.dispatch_refused",
        None,
        None,
        Some("worker handoff has no durable receipt; refusing redispatch"),
        record.grant_id,
        &[],
        &[],
    )?;
    Ok(TimerDispatchOutcome::AckSkip)
}

/// Dispatch one durable `workflow.timer_fired` task event through the normal
/// scheduled lane. The grant transaction records the encrypted worker token
/// and `handed_off` state before the single pipeline Run attempt.
pub(crate) async fn dispatch_task_timer_event(
    state: &AppState,
    event: &openspine_schemas::audit::AuditEvent,
) -> anyhow::Result<TimerDispatchOutcome> {
    let timer_id = event
        .payload_json
        .as_deref()
        .and_then(|payload| serde_json::from_str::<serde_json::Value>(payload).ok())
        .and_then(|payload| {
            payload
                .get("timer_id")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .ok_or_else(|| anyhow::anyhow!("workflow.timer_fired event has no timer_id"))?;
    let key = event.id.to_string();
    let Some(task) = state.store.task_by_timer_id(&timer_id)? else {
        state
            .store
            .mark_dispatch_terminal(&key, &timer_id, None, "non_task_timer", &key)?;
        return Ok(TimerDispatchOutcome::AckSkip);
    };
    if matches!(
        task.status,
        openspine_schemas::task::TaskStatus::Done | openspine_schemas::task::TaskStatus::Cancelled
    ) {
        state.store.mark_dispatch_terminal(
            &key,
            &timer_id,
            Some(task.id),
            "terminal_task",
            &key,
        )?;
        return Ok(TimerDispatchOutcome::AckSkip);
    }
    if let Some(record) = state.store.dispatch_state_for_key(&key)? {
        match record.state {
            TimerDispatchState::Terminal | TimerDispatchState::HandedOff => {
                // A grant was already durably queued (or finalized). Never
                // compose a second grant for a replayed fired event; the
                // consumer's recovery drain re-drives handed_off rows.
                return Ok(TimerDispatchOutcome::AckSkip);
            }
            TimerDispatchState::Pending => {}
        }
    }
    match state.store.owner_principal_by_id(task.owner_principal_id) {
        Ok(_) => {}
        Err(crate::store::StoreError::NotOwner(_)) => {
            state.store.mark_dispatch_terminal(
                &key,
                &timer_id,
                Some(task.id),
                "unknown_or_non_owner_principal",
                &key,
            )?;
            return Ok(TimerDispatchOutcome::AckSkip);
        }
        Err(err) => return Err(err.into()),
    }
    let mut unmet = Vec::new();
    for dependency_id in &task.dependencies {
        match state.store.get_task(*dependency_id)? {
            Some(dep_task) if dep_task.status == openspine_schemas::task::TaskStatus::Done => {}
            Some(_) | None => unmet.push(*dependency_id),
        }
    }
    if !unmet.is_empty() {
        for dependency_id in unmet {
            state.store.insert_dependency_waiter(
                task.id,
                task.owner_principal_id,
                dependency_id,
                &timer_id,
                &key,
            )?;
        }
        state.store.mark_task_blocked(task.id)?;
        state.store.mark_dispatch_terminal(
            &key,
            &timer_id,
            Some(task.id),
            "dependency_wait",
            &key,
        )?;
        return Ok(TimerDispatchOutcome::AckSkip);
    }
    let event_type = if task.reminder_timer_id.as_deref() == Some(timer_id.as_str()) {
        openspine_schemas::event::EventType::TimerReminderFired
    } else {
        openspine_schemas::event::EventType::TimerDeadlineFired
    };
    let inputs = EventInputs {
        chat_id: state.owner_user_id,
        text: format!("task:{}", task.id),
        thread_id: None,
        owner_verified: None,
        principal_override: Some(task.owner_principal_id),
        event_type_override: Some(event_type),
        timer_event_id: Some(key.clone()),
        correlated_task_id: Some(task.id),
        dispatch_key: Some(key),
        dispatch_timer_id: Some(timer_id),
    };
    match run_pipeline(
        state,
        scheduled_internal_lane(),
        &inputs,
        event.ts,
        &mut Vec::new(),
    )
    .await?
    {
        Some(grant) => Ok(TimerDispatchOutcome::Delivered {
            grant: Box::new(grant),
        }),
        None => Ok(TimerDispatchOutcome::Retry),
    }
}

/// Dispatch one durable dependency wake as a fresh queue attempt. Its stable
/// wake key is distinct from the original fired event's terminal idempotency
/// key, and all dependencies are revalidated before composition.
pub(crate) async fn dispatch_task_wake(
    state: &AppState,
    wake: &DependencyWake,
) -> anyhow::Result<TimerDispatchOutcome> {
    if let Some(record) = state.store.dispatch_state_for_key(&wake.wake_key)? {
        match record.state {
            TimerDispatchState::Terminal => {
                state
                    .store
                    .consume_dependency_waiter(wake.task_id, &wake.timer_id)?;
                return Ok(TimerDispatchOutcome::AckSkip);
            }
            TimerDispatchState::HandedOff => {
                let outcome = reattempt_handed_off(state, &record).await?;
                if matches!(outcome, TimerDispatchOutcome::Delivered { .. }) {
                    state
                        .store
                        .consume_dependency_waiter(wake.task_id, &wake.timer_id)?;
                }
                return Ok(outcome);
            }
            TimerDispatchState::Pending => {}
        }
    }
    let Some(task) = state.store.get_task(wake.task_id)? else {
        state
            .store
            .consume_dependency_waiter(wake.task_id, &wake.timer_id)?;
        state.store.mark_dispatch_terminal(
            &wake.wake_key,
            &wake.timer_id,
            Some(wake.task_id),
            "missing_task",
            &wake.wake_key,
        )?;
        return Ok(TimerDispatchOutcome::AckSkip);
    };
    if matches!(
        task.status,
        openspine_schemas::task::TaskStatus::Done | openspine_schemas::task::TaskStatus::Cancelled
    ) {
        state
            .store
            .consume_dependency_waiter(wake.task_id, &wake.timer_id)?;
        state.store.mark_dispatch_terminal(
            &wake.wake_key,
            &wake.timer_id,
            Some(wake.task_id),
            "terminal_task",
            &wake.wake_key,
        )?;
        return Ok(TimerDispatchOutcome::AckSkip);
    }
    match state.store.owner_principal_by_id(task.owner_principal_id) {
        Ok(_) => {}
        Err(crate::store::StoreError::NotOwner(_)) => {
            state
                .store
                .consume_dependency_waiter(wake.task_id, &wake.timer_id)?;
            state.store.mark_dispatch_terminal(
                &wake.wake_key,
                &wake.timer_id,
                Some(wake.task_id),
                "unknown_or_non_owner_principal",
                &wake.wake_key,
            )?;
            return Ok(TimerDispatchOutcome::AckSkip);
        }
        Err(err) => return Err(err.into()),
    }
    let mut all_done = true;
    for dependency_id in &task.dependencies {
        match state.store.get_task(*dependency_id)? {
            Some(dep_task) if dep_task.status == openspine_schemas::task::TaskStatus::Done => {}
            Some(_) | None => {
                all_done = false;
                break;
            }
        }
    }
    if !all_done {
        state
            .store
            .reset_dependency_waiter(wake.task_id, &wake.timer_id)?;
        return Ok(TimerDispatchOutcome::Retry);
    }
    let event_type = if task.reminder_timer_id.as_deref() == Some(wake.timer_id.as_str()) {
        openspine_schemas::event::EventType::TimerReminderFired
    } else {
        openspine_schemas::event::EventType::TimerDeadlineFired
    };
    let inputs = EventInputs {
        chat_id: state.owner_user_id,
        text: format!("task:{}", task.id),
        thread_id: None,
        owner_verified: None,
        principal_override: Some(task.owner_principal_id),
        event_type_override: Some(event_type),
        timer_event_id: None,
        correlated_task_id: Some(task.id),
        dispatch_key: Some(wake.wake_key.clone()),
        dispatch_timer_id: Some(wake.timer_id.clone()),
    };
    match run_pipeline(
        state,
        scheduled_internal_lane(),
        &inputs,
        Timestamp::now(),
        &mut Vec::new(),
    )
    .await?
    {
        Some(grant) => {
            state
                .store
                .consume_dependency_waiter(wake.task_id, &wake.timer_id)?;
            Ok(TimerDispatchOutcome::Delivered {
                grant: Box::new(grant),
            })
        }
        None => {
            state
                .store
                .reset_dependency_waiter(wake.task_id, &wake.timer_id)?;
            Ok(TimerDispatchOutcome::Retry)
        }
    }
}

/// Complete a task and immediately dispatch all dependency wakes that become
/// ready. This is the completion boundary for worker/task integrations.
#[allow(dead_code)]
pub(crate) async fn complete_task_and_wake(state: &AppState, task_id: Ulid) -> anyhow::Result<()> {
    let wakes = state.store.mark_task_done_and_poll(task_id)?;
    for wake in wakes {
        let _ = dispatch_task_wake(state, &wake).await?;
    }
    Ok(())
}

/// Drain non-terminal dispatch and ready dependency rows. This is called at
/// startup and each consumer iteration, independently of the event checkpoint.
pub(crate) async fn recover_timer_dispatches(state: &AppState) -> anyhow::Result<()> {
    for record in state.store.incomplete_timer_dispatches()? {
        if record.state == TimerDispatchState::HandedOff {
            let _ = reattempt_handed_off(state, &record).await?;
        }
    }
    let _ = state.store.poll_all_dependency_waits()?;
    let wakes = state.store.take_ready_wakes()?;
    for wake in wakes {
        let _ = dispatch_task_wake(state, &wake).await?;
    }
    Ok(())
}

/// firer; this loop only replays newly appended `workflow.timer_fired` rows
/// and advances its checkpoint after the scheduled pipeline succeeds.
pub(crate) async fn run_task_deadline_consumer(state: &AppState) -> ! {
    use openspine_schemas::audit::AuditKind;
    use openspine_schemas::event_bus::{ConsumerCheckpoint, EventSubscriptionFilter};
    let filter = EventSubscriptionFilter::kinds([
        AuditKind::new("workflow.timer_fired").expect("static audit kind is valid")
    ]);
    let consumer_id = "task_board_timer_consumer";
    let mut checkpoint = match state.store.load_consumer_checkpoint(consumer_id) {
        Ok(Some(saved)) => saved.checkpoint,
        Ok(None) => ConsumerCheckpoint::default(),
        Err(err) => {
            tracing::error!(error = %err, "task-board timer consumer checkpoint load failed");
            ConsumerCheckpoint::default()
        }
    };
    if let Err(err) = recover_timer_dispatches(state).await {
        tracing::error!(error = %err, "timer dispatch recovery failed at startup");
    }
    loop {
        if let Err(err) = recover_timer_dispatches(state).await {
            tracing::warn!(error = %err, "timer dispatch recovery poll failed");
        }
        match state
            .store
            .replay_audit(&filter, checkpoint.last_acked_global_seq)
        {
            Ok(entries) => {
                for entry in entries {
                    match dispatch_task_timer_event(state, &entry.event).await {
                        Ok(TimerDispatchOutcome::Delivered { .. })
                        | Ok(TimerDispatchOutcome::AckSkip) => {
                            checkpoint.last_acked_global_seq = entry.global_seq;
                            if let Err(err) = state.store.save_consumer_checkpoint(
                                consumer_id,
                                &crate::store::event_bus::PersistedConsumerState {
                                    schema_version: 1,
                                    checkpoint: checkpoint.clone(),
                                    filter: filter.clone(),
                                },
                            ) {
                                tracing::error!(error = %err, "task-board timer checkpoint save failed");
                            }
                        }
                        Ok(TimerDispatchOutcome::Retry) => {
                            // Withhold the checkpoint; retry the same event
                            // (surfaces a stuck consumer) rather than skip it.
                            tracing::warn!(
                                global_seq = entry.global_seq,
                                "task timer event withheld for retry (authority denial or stuck grant)"
                            );
                            break;
                        }
                        Err(err) => {
                            tracing::error!(
                                error = %err,
                                global_seq = entry.global_seq,
                                "task-board timer event handling failed; retrying"
                            );
                            break;
                        }
                    }
                }
            }
            Err(err) => tracing::error!(error = %err, "task-board timer replay failed"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        if let Err(err) = recover_timer_dispatches(state).await {
            tracing::error!(error = %err, "timer dispatch recovery drain failed");
        }
    }
}

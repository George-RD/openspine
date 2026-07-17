// openspine:allow-large-module reason: pipeline orchestration remains one audited stage boundary
//! The owner-message pipeline: Telegram update -> owner verification ->
//! identity resolution -> route resolution -> authority composition -> task
//! grant -> sandboxed shell spawn.
//!
//! The pipeline's execution is now delegated to the single typed
//! [`driver::run_pipeline`], which interprets one of two compiled-in lane
//! specifications ([`driver::owner_control_lane`] and
//! [`driver::email_preview_lane`]) over the nine-stage sequence declared once
//! in [`driver::PipelineStage`]. This module keeps the shared helpers the
//! lanes rely on ([`AppState`], [`empty_session_policy`],
//! [`notify_owner_best_effort`]) and the public entry points
//! ([`run_telegram_poll_loop`], [`handle_owner_update`]). Identity resolution
//! lives in [`crate::identity::IdentityResolver`], driven by an unforgeable
//! [`crate::telegram::VerifiedOwnerContext`] minted only by
//! [`crate::telegram::verify_update`].
//!
//! Lane selection (the `/draft <thread_id>` command) is recognized here, at
//! the Event-stage boundary, and handed to the driver as lane data — the
//! driver never re-branches on it. Every step that terminates the pipeline
//! early is audited, so "why didn't Lyra reply" is always answerable from
//! `audit_log` alone.
//!
//! v1 has one owner principal (bootstrapped at kernel start). The Telegram
//! owner user id remains only the channel *authentication* signal for
//! [`crate::telegram::verify_update`]; composition consumes the resolved
//! `principal_id` (AD-146).
mod approval;
mod artifact_activation;
mod artifact_nomination;
mod artifact_reconfirmation;
mod digest_pagination;
mod driver;
mod lanes;
mod offset;
mod plan_approval;
mod stages;
pub(crate) use offset::initialize_telegram_bot_id_until_ready;
#[cfg(test)]
pub(crate) use offset::{
    dispatch_polled_updates_for_test, initialize_telegram_bot_id, resolve_telegram_offset_for_test,
};
pub(crate) use offset::{is_already_processed, resolve_telegram_offset};
mod post_approval;
mod selection;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::approval_fixture_grant;

use jiff::Timestamp;
use openspine_gate::{gate, ActionOrigin};
use openspine_schemas::action::{ActionCatalog, ActionId, ActionRequest, GateDecision};
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::grant::{GrantLimits, TaskGrant};
use openspine_schemas::policy::{Constraints, SessionPolicy};
use ulid::Ulid;

use crate::api::handler_registry::ActionHandlerRegistry;
use crate::artifact_loader::ArtifactRegistry;
use crate::artifact_store::ArtifactStore;
use crate::connectors::ConnectorRegistry;
use crate::sandbox::Sandbox;
use crate::secret_store::SecretStore;
use crate::store::task_board::{DependencyWake, TimerDispatchRecord, TimerDispatchState};
use crate::store::{failure_surfacing_types::DetailReceipt, Store};
use crate::telegram::{self, VerifiedUpdate};
use std::collections::HashMap;
use std::path::PathBuf;

use approval::handle_draft_approval_callback;
use driver::{
    email_preview_lane, owner_control_lane, run_pipeline, scheduled_internal_lane, EventInputs,
};
use plan_approval::handle_plan_approval_callback;

/// Everything the pipeline needs to turn one Telegram update into an
/// audited, sandboxed task. Built once at kernel startup and shared
/// (read-only except for its own interior-mutable pieces) across the
/// Telegram poll loop and the axum HTTP layer.
pub struct AppState {
    pub store: Store,
    pub artifacts: ArtifactStore,
    pub secrets: std::sync::Arc<SecretStore>,
    pub registry: parking_lot::RwLock<ArtifactRegistry>,
    pub action_catalog: ActionCatalog,
    pub sandbox: Sandbox,
    pub action_handlers: ActionHandlerRegistry,
    pub connectors: ConnectorRegistry,
    pub owner_user_id: i64,
    pub owner_principal_id: Ulid,
    pub owner_identity_id: Ulid,
    /// e.g. `http://127.0.0.1:7777` — passed to the shell as `KERNEL_ENDPOINT`.
    pub kernel_endpoint: String,
    /// D-025 / PRD §16 escape hatch. See [`sandbox::refuses_external_communication_without_containment`].
    pub unsafe_allow_uncontained_private_data: bool,
    /// Provider clients are resolved once at startup from the operator's
    /// configured pool; runtime proposals can only switch the active role
    /// to one of these pre-vetted clients (AD-152, no silent swaps).
    pub provider_pool: HashMap<String, crate::model_gateway::ProviderClient>,
    /// Active provider id per governed model role. The map is kernel-owned
    /// and changes only in post-approval model-swap activation.
    pub active_model_providers:
        parking_lot::RwLock<HashMap<openspine_schemas::model_swap::ModelRole, String>>,
    pub provider_config_digests: HashMap<String, openspine_schemas::digest::Digest>,
    /// Backs `GET /v1/status`'s `uptime_seconds`.
    pub started_at: std::time::Instant,
    /// `data/artifacts.d` overlay dir (5a/5d): approved `artifact.propose`
    /// activations are written here as `<kind-plural>/<id>-v<version>.yaml`
    /// so they survive restart, and the startup loader re-merges them into
    /// the live registry alongside the fixtures.
    pub overlay_dir: PathBuf,
    /// AD-143: required global per-day spend cap across model and connector
    /// calls. The lane gate and usage reservations read this kernel setting.
    pub spend_cap: crate::config::SpendCapConfig,
    pub conversation_locks:
        parking_lot::Mutex<std::collections::HashMap<i64, std::sync::Arc<tokio::sync::Mutex<()>>>>,
    /// `(kind, id)` identities loaded from base fixtures before overlay merge.
    pub base_artifact_ids: std::collections::HashSet<(String, String)>,
    /// Digest of sorted active base artifacts reviewed by owner taps.
    pub base_compatibility_epoch: String,
}

impl AppState {
    pub async fn lock_conversation(&self, chat_id: i64) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.conversation_locks.lock();
            locks
                .entry(chat_id)
                .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        lock.lock_owned().await
    }
}
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

/// Phase 1 has no persisted per-user/session policy system yet (D-013's
/// "dynamic behavior should be easy" is served by the artifact registry, not
/// a session-policy store that doesn't exist). An empty session policy
/// narrows nothing — see `compose_authority`'s documented interpretation of
/// design.md's merge rule.
fn empty_session_policy() -> SessionPolicy {
    SessionPolicy {
        schema_version: 1,
        candidate_allowed_actions: vec![],
        approval_required: vec![],
        denied_actions: vec![],
        constraints: Constraints::default(),
    }
}

/// PRD §5.4: identity resolution is one *input* to authority, never a
/// grant of authority itself (D-006) — but by the time this runs, the
/// Telegram connector has already verified sender id + private chat, so
/// confidence is 1.0 and `source_verified` is `true` unconditionally.
/// Best-effort owner notification for a pipeline failure the owner can
/// actually act on (a `/draft` failure, a post-approval draft-creation
/// failure) — distinct from a security denial (route/authority reject
/// for a legitimate reason), which stays silent-and-audited like every
/// other denial in this pipeline. A failed reply here is logged, never
/// propagated: notifying the owner is a courtesy, not part of the
/// audited authority decision itself. Shared by the approval and email-preview
/// lanes — both are "tell the owner why their tap/command didn't work" call
/// sites, not just the selection flow.
///
/// D-055.2: kernel-originated effects route through `gate()` like any other
/// action, but `ActionOrigin::Kernel` auto-allows only actions in the catalog's
/// trusted kernel-origin set (`owner.notify`). `gate()` is the single authority
/// for that carve-out; if `owner.notify` is ever dropped from the set, the send
/// fails closed. Every send is still audited as `owner.notified` so the
/// trusted-path carve-out remains traceable. The audit append is itself
/// best-effort: a failure here must never suppress the owner-facing reply it is
/// only recording.
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
    let send_result = state.connectors.telegram().send_reply(chat_id, text).await;
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
                        format!("artifact persistence failed; notification send error: {err}");
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
                &err.to_string(),
                digest_item_ids,
                detail,
            ) {
                tracing::error!(error = %record_err, send_error = %err, "owner notification failure could not be durably recorded");
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
    };
    grant.seal_root(&key);
    Some(grant)
}

/// Short-lived owner-bound synthetic grant minted at the moment an owner taps
/// an `artifact.reconfirm` button (AD-070). The durable review object is the
/// pending learned-artifact row + ActionRequest; authority begins only here.
pub(super) fn mint_reconfirm_grant(task_grant_id: Ulid) -> Option<TaskGrant> {
    use openspine_schemas::action::ActionId;
    use openspine_schemas::grant::GrantMode;
    let key = crate::grant_hmac_key()?;
    let now = Timestamp::now();
    let reconfirm = ActionId::new("artifact.reconfirm");
    let mut grant = TaskGrant {
        id: task_grant_id,
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user: "kernel".to_string(),
        purpose: "overlay-reconfirm".to_string(),
        issued_by: "kernel".to_string(),
        issued_at: now,
        expires_at: now + std::time::Duration::from_secs(300),
        event_id: Ulid::new(),
        route_id: "overlay_reconfirm".to_string(),
        agent_id: "kernel".to_string(),
        workflow_id: "overlay_reconfirm".to_string(),
        capability_pack_id: "kernel".to_string(),
        authority_sources: vec![],
        selection_tokens: vec![],
        allowed_actions: vec![reconfirm.clone()],
        approval_required_actions: vec![reconfirm],
        denied_actions: vec![],
        allowed_egress_classes: vec![],
        output_channels: vec![],
        limits: GrantLimits {
            max_model_calls: 0,
            max_artifacts: 0,
            max_runtime_seconds: 0,
        },
        task_token: format!("reconfirm-{}", Ulid::new()),
        root_grant_id: Ulid::nil(),
        parent_grant_id: None,
        mode: GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
    };
    grant.seal_root(&key);
    Some(grant)
}

/// Long-poll Telegram forever, dispatching every verified owner update
/// through [`handle_owner_update`]. Replay protection (design.md):
/// **at-most-once**, not at-least-once — `update_id` is persisted to
/// `kv_state` *before* the update is handed to the pipeline. For an
/// action-taking assistant a duplicate task grant (double shell spawn,
/// double reply, and in a future phase a double-sent email) is worse than
/// occasionally dropping a message the owner can just retype; a crash
/// between "offset persisted" and "handling finished" loses at most one
/// update rather than replaying an already-actioned one.
pub async fn run_telegram_poll_loop(state: &AppState) -> anyhow::Result<()> {
    const POLL_ERROR_BACKOFF: std::time::Duration = std::time::Duration::from_secs(5);
    initialize_telegram_bot_id_until_ready(state, POLL_ERROR_BACKOFF).await;
    loop {
        let (offset_key, last_update_id) = resolve_telegram_offset(state)?;

        crate::spend::guard_connector(state, true).await?;
        let updates = match state.connectors.telegram().poll_once(last_update_id).await {
            Ok(updates) => {
                crate::failure_surfacing::record_connector_outcome(&state.store, "telegram", true)?;
                updates
            }
            Err(err) => {
                crate::failure_surfacing::record_connector_outcome(
                    &state.store,
                    "telegram",
                    false,
                )?;
                crate::failure_surfacing::batch_failure(
                    state,
                    crate::failure_surfacing::FailureClass::Connector,
                    "telegram poll failed",
                    &format!("telegram poll: {err}"),
                )?;
                tracing::warn!(error = %err, "telegram poll_once failed, backing off");
                tokio::time::sleep(POLL_ERROR_BACKOFF).await;
                continue;
            }
        };
        dispatch_polled_updates(state, updates, offset_key, last_update_id).await?;
    }
}

async fn dispatch_polled_updates(
    state: &AppState,
    updates: Vec<telegram::TelegramUpdate>,
    offset_key: String,
    last_update_id: Option<i64>,
) -> anyhow::Result<()> {
    for update in updates {
        // At-most-once replay guard: a previously consumed update is
        // dropped before it can reach the pipeline, model, or shell.
        if is_already_processed(update.update_id, last_update_id) {
            continue;
        }
        // Persist the offset *before* handling: see this function's
        // at-most-once contract above.
        state
            .store
            .set_kv(&offset_key, &update.update_id.to_string())?;
        if let Err(err) = handle_owner_update(state, &update).await {
            tracing::warn!(
                error = %err,
                update_id = update.update_id,
                "owner update handling failed"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) async fn poll_telegram_once_for_test(state: &AppState) -> anyhow::Result<()> {
    let (offset_key, last_update_id) = resolve_telegram_offset(state)?;
    crate::spend::guard_connector(state, true).await?;
    let updates = state
        .connectors
        .telegram()
        .poll_once(last_update_id)
        .await?;
    dispatch_polled_updates(state, updates, offset_key, last_update_id).await
}

/// Run one verified-or-not Telegram update through the full pipeline.
/// Returns `Ok(None)` for every outcome the pipeline itself decides on
/// (ignored, denied, refused, ambiguous) — those are not errors, they are
/// the pipeline correctly declining to act. Returns `Ok(Some(grant))` once
/// authority has been composed and persisted, *regardless* of whether the
/// subsequent shell spawn succeeds. Only a genuine infrastructure failure —
/// store I/O or an inconsistent registry — surfaces as `Err`.
///
/// Lane selection happens here, at the Event-stage boundary: a `/draft
/// <thread_id>` message selects the email-preview lane, any other owner
/// message selects the owner-control lane. The driver interprets the chosen
/// lane as data; it does not branch on command syntax itself.
pub async fn handle_owner_update(
    state: &AppState,
    update: &telegram::TelegramUpdate,
) -> anyhow::Result<Option<TaskGrant>> {
    let verified = telegram::verify_update(update, state.owner_user_id);
    if let VerifiedUpdate::Ignored { reason } = &verified {
        state.store.append_audit(
            "telegram.update.ignored",
            None,
            None,
            Some(reason),
            None,
            &[],
            &[],
        )?;
        return Ok(None);
    }

    let chat_id = match &verified {
        VerifiedUpdate::OwnerMessage { chat_id, .. } => *chat_id,
        VerifiedUpdate::OwnerCallback { chat_id, .. } => *chat_id,
        VerifiedUpdate::Ignored { .. } => unreachable!(),
    };

    let _guard = state.lock_conversation(chat_id).await;

    let (chat_id, text, owner_verified) = match verified {
        VerifiedUpdate::OwnerMessage {
            chat_id,
            text,
            context,
        } => (chat_id, text, Some(context)),
        VerifiedUpdate::OwnerCallback {
            chat_id,
            callback_query_id,
            data,
            context: _,
        } => {
            if let Some(action_request_id) = telegram::parse_approve_callback(&data) {
                handle_draft_approval_callback(
                    state,
                    chat_id,
                    &callback_query_id,
                    action_request_id,
                )
                .await?;
            } else if let Some(action_request_id) = telegram::parse_approve_plan_callback(&data) {
                handle_plan_approval_callback(
                    state,
                    chat_id,
                    &callback_query_id,
                    action_request_id,
                )
                .await?;
            } else {
                crate::spend::guard_connector(state, true).await?;
                let answer_result = state
                    .connectors
                    .telegram()
                    .answer_callback_query(&callback_query_id)
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

                state.store.append_audit(
                    "telegram.callback_unrecognized",
                    None,
                    None,
                    Some(&data),
                    None,
                    &[],
                    &[],
                )?;
            }
            return Ok(None);
        }
        VerifiedUpdate::Ignored { .. } => unreachable!(),
    };
    match crate::secret_intake::capture(state, chat_id, &text).await {
        Ok(Some(outcome)) => {
            let response = match outcome {
                crate::secret_intake::CaptureOutcome::Stored(crate::secret_intake::SecretMode::Intake) => {
                    "Secret intake completed; value was stored."
                }
                crate::secret_intake::CaptureOutcome::Staged(crate::secret_intake::SecretMode::Intake) => {
                    "Secret received and staged; provide the paired Gmail credential to validate and activate it."
                }
                crate::secret_intake::CaptureOutcome::Staged(crate::secret_intake::SecretMode::Rotate) => {
                    "Secret received and staged; provide the paired Gmail credential to validate and activate rotation."
                }
                crate::secret_intake::CaptureOutcome::Stored(crate::secret_intake::SecretMode::Rotate) => {
                    "Secret rotation completed; value was stored."
                }
                crate::secret_intake::CaptureOutcome::Rejected => {
                    "Secret message discarded; intake expired, failed validation, or was not bound to this chat. Retry."
                }
            };
            notify_owner_best_effort(state, chat_id, response).await;
            return Ok(None);
        }
        Ok(None) => {}
        Err(err) => {
            let _ = state.store.delete_kv("secret.intake.pending");
            tracing::warn!(error = %err, "secret capture failed; pending state cleared");
            notify_owner_best_effort(
                state,
                chat_id,
                "Secret capture failed; intake was cleared. Retry.",
            )
            .await;
            return Ok(None);
        }
    }
    if text.trim().starts_with("/secret") {
        if let Some((mode, slot)) = crate::secret_intake::parse_command(&text) {
            let proof = owner_verified
                .as_ref()
                .expect("verified owner message carries proof");
            let armed = crate::secret_intake::arm(
                state,
                chat_id,
                state.owner_principal_id,
                proof,
                mode,
                slot,
            )?;
            let response = if armed {
                "Secret mode armed; send the value in your next private message."
            } else {
                "Secret mode was denied; retry after verifying owner authority."
            };
            notify_owner_best_effort(state, chat_id, response).await;
        } else {
            notify_owner_best_effort(
                state,
                chat_id,
                "Invalid /secret command. Use /secret intake <slot> or /secret rotate <slot>.",
            )
            .await;
        }
        return Ok(None);
    }

    if let Some(args) = telegram::parse_digest_namespace(&text) {
        if !args.is_empty() && telegram::parse_digest_detail_command(&text).is_none() {
            notify_owner_best_effort(state, chat_id, "Usage: /digest or /digest <ULID> [page]")
                .await;
            return Ok(None);
        }
    }
    if let Some((channel_user_id, relationship_str)) = telegram::parse_bind_command(&text) {
        let result = crate::identity::handle_owner_bind(
            &state.store,
            state.owner_principal_id,
            state.owner_identity_id,
            owner_verified
                .as_ref()
                .expect("bind command requires verified owner"),
            channel_user_id,
            relationship_str,
        );
        let message = result.unwrap_or_else(|err| err);
        notify_owner_best_effort(state, chat_id, &message).await;
        return Ok(None);
    }

    if let Some((id, page)) = telegram::parse_digest_detail_command(&text) {
        digest_pagination::handle_detail_command(state, chat_id, id, page).await?;
        return Ok(None);
    }
    if telegram::parse_digest_command(&text) {
        digest_pagination::handle_command(state, chat_id).await?;
        return Ok(None);
    }

    // D-036 / design.md: the `/draft <thread_id>` command is the entire
    // trust boundary for "did the owner select this thread" — it is
    // recognized here as lane selection, and the driver interprets lane data;
    // it never re-branches on command syntax. The chosen lane is then run
    // through the same synchronous stage prefix as every other owner event.
    let thread_id = telegram::parse_draft_command(&text).map(str::to_string);
    let spec = if thread_id.is_some() {
        email_preview_lane()
    } else {
        owner_control_lane()
    };
    let inputs = EventInputs {
        chat_id,
        text,
        thread_id,
        owner_verified,
        principal_override: None,
        event_type_override: None,
        timer_event_id: None,
        correlated_task_id: None,
        dispatch_key: None,
        dispatch_timer_id: None,
    };
    run_pipeline(state, spec, &inputs, Timestamp::now(), &mut Vec::new()).await
}

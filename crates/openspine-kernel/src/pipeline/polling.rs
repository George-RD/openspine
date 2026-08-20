//! Telegram long-poll loop and replay-safe update dispatch.

use super::{
    handle_owner_update, initialize_telegram_bot_id_until_ready, is_already_processed,
    resolve_telegram_offset, AppState,
};
use crate::telegram;

/// Backoff between failed poll iterations (and failed bot-identity
/// initialization attempts) in production. Tests drive the per-iteration
/// driver with `Duration::ZERO`.
const POLL_ERROR_BACKOFF: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug)]
pub(crate) enum PollTelegramOutcome {
    Complete,
    Retry(anyhow::Error),
}

/// Long-poll Telegram forever. Initializes the bot identity once (retrying
/// under backoff so a transient getMe failure never terminates the kernel),
/// then drives [`poll_telegram_iteration`] — the single loop body that is both
/// the production path and the test surface — until the process exits.
pub async fn run_telegram_poll_loop(state: &AppState) -> anyhow::Result<()> {
    initialize_telegram_bot_id_until_ready(state, POLL_ERROR_BACKOFF).await;
    loop {
        poll_telegram_iteration(state, POLL_ERROR_BACKOFF).await?;
    }
}

/// One iteration of the poll loop: run [`poll_telegram_once`], and when the
/// Telegram preflight/poll itself fails, surface that connector error to the
/// owner digest, log it, and sleep `backoff` before the caller loops again.
/// Offset resolution, spend admission, dispatch persistence, and failure-
/// surfacing infrastructure errors still propagate exactly as they did in the
/// original production loop. `backoff` is `POLL_ERROR_BACKOFF` in production
/// and `Duration::ZERO` in tests (mirroring
/// [`initialize_telegram_bot_id_until_ready`]).
pub(crate) async fn poll_telegram_iteration(
    state: &AppState,
    backoff: std::time::Duration,
) -> anyhow::Result<()> {
    match poll_telegram_once(state).await? {
        PollTelegramOutcome::Complete => {}
        PollTelegramOutcome::Retry(err) => {
            crate::failure_surfacing::batch_failure(
                state,
                crate::failure_surfacing::FailureClass::Connector,
                "telegram poll failed",
                &format!("telegram poll: {err}"),
            )?;
            tracing::warn!(error = %err, "telegram poll_once failed, backing off");
            tokio::time::sleep(backoff).await;
        }
    }
    Ok(())
}

/// Perform exactly one poll cycle: resolve the consumed offset, admit the
/// connector spend, poll once through the breaker preflight, and dispatch the
/// returned updates. Classifies only a preflight/poll error for retry.
pub(crate) async fn poll_telegram_once(state: &AppState) -> anyhow::Result<PollTelegramOutcome> {
    let (offset_key, last_update_id) = resolve_telegram_offset(state)?;
    crate::spend::guard_connector(state, true).await?;
    let updates = match crate::api::connector_breaker::call_with_connector_preflight(
        state,
        "telegram",
        None,
        state.connectors.telegram().poll_once(last_update_id),
    )
    .await
    {
        Ok(updates) => updates,
        Err(err) => return Ok(PollTelegramOutcome::Retry(err.into())),
    };
    dispatch_polled_updates(state, updates, offset_key, last_update_id).await?;
    Ok(PollTelegramOutcome::Complete)
}

/// Dispatch polled updates through [`handle_owner_update`] with replay
/// protection (design.md): **at-most-once**, not at-least-once — each
/// `update_id` is persisted to `kv_state` *before* the update is handed to the
/// pipeline. For an action-taking assistant a duplicate task grant (double
/// shell spawn, double reply, and in a future phase a double-sent email) is
/// worse than occasionally dropping a message the owner can just retype; a
/// crash between "offset persisted" and "handling finished" loses at most one
/// update rather than replaying an already-actioned one. A previously consumed
/// `update_id` is dropped before it can reach the pipeline, model, or shell.
/// `handle_owner_update` errors are logged, not propagated — one update
/// failing must not stall the loop. Returns the number of updates that passed
/// the replay guard and were dispatched.
pub(crate) async fn dispatch_polled_updates(
    state: &AppState,
    updates: Vec<telegram::TelegramUpdate>,
    offset_key: String,
    last_update_id: Option<i64>,
) -> anyhow::Result<usize> {
    let mut dispatched = 0usize;
    for update in updates {
        if is_already_processed(update.update_id, last_update_id) {
            continue;
        }
        // Persist the offset *before* handling: see this function's
        // at-most-once contract above.
        state
            .store
            .set_kv(&offset_key, &update.update_id.to_string())?;
        dispatched += 1;
        if let Err(err) = handle_owner_update(state, &update).await {
            tracing::warn!(
                error = %err,
                update_id = update.update_id,
                "owner update handling failed"
            );
        }
    }
    Ok(dispatched)
}

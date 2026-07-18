//! Telegram long-poll loop and replay-safe update dispatch.

use super::{
    handle_owner_update, initialize_telegram_bot_id_until_ready, is_already_processed,
    resolve_telegram_offset, AppState,
};
use crate::telegram;

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
        let updates = match crate::api::connector_breaker::call_with_connector_preflight(
            state,
            "telegram",
            None,
            state.connectors.telegram().poll_once(last_update_id),
        )
        .await
        {
            Ok(updates) => updates,
            Err(err) => {
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
    crate::spend::guard_connector(state, true).await?;
    let (offset_key, last_update_id) = resolve_telegram_offset(state)?;
    let updates = crate::api::connector_breaker::call_with_connector_preflight(
        state,
        "telegram",
        None,
        state.connectors.telegram().poll_once(last_update_id),
    )
    .await?;
    dispatch_polled_updates(state, updates, offset_key, last_update_id).await
}

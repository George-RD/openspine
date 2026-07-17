use super::AppState;

pub(crate) async fn initialize_telegram_bot_id(state: &AppState) -> anyhow::Result<()> {
    let token = state
        .secrets
        .get_string(crate::telegram::BOT_TOKEN_SLOT)?
        .ok_or_else(|| anyhow::anyhow!("telegram bot identity unavailable"))?;
    // ALWAYS validate the vault token's actual bot id at startup — never trust
    // the persisted `telegram.bot_id`. A vault/SQLite split (process death
    // between a token `put` and the `telegram.bot_id` update during a
    // mid-rotation capture) would otherwise leave the kernel polling bot B
    // under bot A's offset namespace, stranding B's updates.
    crate::spend::guard_connector(state, true).await?;
    let actual_bot_id = state
        .connectors
        .telegram()
        .validate_candidate_token_id(&token)
        .await
        .ok_or_else(|| anyhow::anyhow!("telegram bot identity validation failed"))?;
    match state.store.get_kv("telegram.bot_id")? {
        // True first boot: persist the identity and migrate the legacy offset.
        None => state
            .store
            .initialize_telegram_bot_id_and_migrate_offset(actual_bot_id)?,
        Some(raw) => {
            // Preserve key presence separately: a malformed persisted id is an
            // error, NOT a first boot (it must not permit legacy migration).
            let persisted: i64 = raw
                .parse()
                .map_err(|_| anyhow::anyhow!("persisted telegram.bot_id is malformed: {raw}"))?;
            if persisted != actual_bot_id {
                // Changed token: switch to the actual id's fresh namespace
                // without inheriting the previous bot's consumed offset.
                state
                    .store
                    .reconcile_telegram_bot_id_to_actual(actual_bot_id)?;
            }
            // Same id: offset namespace preserved, nothing to do.
        }
    }
    Ok(())
}

/// Retry bot-identity initialization under `backoff` until it succeeds. A
/// transient getMe/network failure on first upgraded startup (bot_id absent)
/// must not terminate the poll future — `main`'s `tokio::select!` treats a
/// returned `Err` from the poll loop as a fatal "telegram poll loop failed"
/// and exits the kernel, so identity init is retried here (under the same
/// backoff as poll errors) rather than propagated. `backoff` is
/// `POLL_ERROR_BACKOFF` in production and `Duration::ZERO` in tests.
pub(crate) async fn initialize_telegram_bot_id_until_ready(
    state: &AppState,
    backoff: std::time::Duration,
) {
    loop {
        match initialize_telegram_bot_id(state).await {
            Ok(()) => return,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "telegram bot identity initialization failed, backing off"
                );
                tokio::time::sleep(backoff).await;
            }
        }
    }
}

pub(crate) fn resolve_telegram_offset(state: &AppState) -> anyhow::Result<(String, Option<i64>)> {
    let bot_id = state.store.get_kv("telegram.bot_id")?;
    let offset_key = bot_id
        .as_deref()
        .map(|id| format!("last_telegram_update_id.{id}"))
        .unwrap_or_else(|| "last_telegram_update_id".to_string());
    let last_update_id = state
        .store
        .get_kv(&offset_key)?
        .and_then(|s| s.parse().ok());
    Ok((offset_key, last_update_id))
}

pub(crate) fn is_already_processed(update_id: i64, last_update_id: Option<i64>) -> bool {
    last_update_id.is_some_and(|last| update_id <= last)
}

#[cfg(test)]
pub(crate) fn resolve_telegram_offset_for_test(
    state: &AppState,
) -> anyhow::Result<(String, Option<i64>)> {
    resolve_telegram_offset(state)
}

#[cfg(test)]
pub(crate) async fn dispatch_polled_updates_for_test(
    state: &AppState,
    updates: Vec<crate::telegram::TelegramUpdate>,
    last_update_id: Option<i64>,
) -> anyhow::Result<usize> {
    let mut dispatched = 0usize;
    for update in updates {
        if is_already_processed(update.update_id, last_update_id) {
            continue;
        }
        if super::handle_owner_update(state, &update).await.is_ok() {
            dispatched += 1;
        }
    }
    Ok(dispatched)
}

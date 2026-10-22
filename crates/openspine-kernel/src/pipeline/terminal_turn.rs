//! One direct-terminal chat turn: send the owner's line through the pipeline
//! and wait for the single reply the worker delivers, or report why none will
//! come.

use std::time::Duration;

use anyhow::Context as _;

use super::{handle_terminal_message, AppState};

/// How long to wait for a reply after the worker has already exited. Replies
/// are delivered over the kernel's HTTP surface while the worker runs, so one
/// that is not queued by now is never coming; the wait only absorbs scheduling
/// jitter and is not a model-latency budget.
const REPLY_GRACE: Duration = Duration::from_secs(5);

pub(crate) async fn run_terminal_turn(
    state: &AppState,
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
    message: String,
) -> anyhow::Result<String> {
    if message.trim().is_empty() {
        anyhow::bail!("terminal message must not be empty");
    }
    // A failure recorded by an earlier turn must not be blamed on this one.
    state.terminal_turn_failure.lock().take();
    let grant = handle_terminal_message(state, message).await?;
    if grant.is_none() {
        anyhow::bail!("terminal message was denied before a task grant was created");
    }
    if let Some(failure) = state.terminal_turn_failure.lock().take() {
        // A worker that replied and then died (for instance while reporting
        // its result) still answered the owner: deliver that reply. The
        // failure is already audited and logged. Only a turn with nothing
        // queued is an error.
        return match receiver.try_recv() {
            Ok(reply) => Ok(reply),
            Err(_) => Err(anyhow::anyhow!(
                "the worker failed before it could reply: {failure}"
            )),
        };
    }
    tokio::time::timeout(REPLY_GRACE, receiver.recv())
        .await
        .context("the worker finished without sending a reply")?
        .ok_or_else(|| anyhow::anyhow!("terminal reply channel closed before a reply arrived"))
}

//! One direct-terminal chat turn: send the owner's line through the pipeline
//! and wait for the single reply the worker delivers, or report why none will
//! come.

use std::time::Duration;

use super::{handle_terminal_message, AppState};

/// How long to wait for a reply after the worker has already exited. Replies
/// are delivered over the kernel's HTTP surface while the worker runs, so one
/// that is not queued by now is never coming; the wait only absorbs scheduling
/// jitter and is not a model-latency budget.
const REPLY_GRACE: Duration = Duration::from_secs(5);

/// A chat turn that ran but produced no reply (worker failure, silence, a
/// refused message). Setup readiness is not the cause, so the CLI failure
/// report omits its readiness remedy for this class.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct TurnFailed(pub(crate) String);

pub(crate) async fn run_terminal_turn(
    state: &AppState,
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
    message: String,
) -> anyhow::Result<String> {
    if message.trim().is_empty() {
        return Err(TurnFailed("terminal message must not be empty".into()).into());
    }
    // Each turn starts from an empty queue and no recorded failure, so a
    // leftover from an earlier turn is never shown as this turn's answer.
    while receiver.try_recv().is_ok() {}
    state.terminal_turn_failure.lock().take();
    let grant = handle_terminal_message(state, message).await?;
    if grant.is_none() {
        // Owner commands (`/review …`) are answered without a task grant and
        // queue their reply before returning; a genuine denial queues nothing.
        return receiver.try_recv().map_err(|_| {
            TurnFailed("terminal message was denied before a task grant was created".into()).into()
        });
    }
    if let Some(failure) = state.terminal_turn_failure.lock().take() {
        // A worker that replied and then died (for instance while reporting
        // its result) still answered the owner: deliver that reply. The
        // failure is already audited and logged. Only a turn with nothing
        // queued is an error.
        return match receiver.try_recv() {
            Ok(reply) => Ok(reply),
            Err(_) => Err(TurnFailed(format!(
                "the worker failed before it could reply: {failure}"
            ))
            .into()),
        };
    }
    tokio::time::timeout(REPLY_GRACE, receiver.recv())
        .await
        .map_err(|_| TurnFailed("the worker finished without sending a reply".into()))?
        .ok_or_else(|| {
            anyhow::Error::from(TurnFailed(
                "terminal reply channel closed before a reply arrived".into(),
            ))
        })
}

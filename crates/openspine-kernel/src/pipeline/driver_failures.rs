use super::lanes::PreflightFailure;
use super::{notify_owner_best_effort, AppState};

pub(super) async fn emit_preflight_failure(
    state: &AppState,
    chat_id: i64,
    failure: PreflightFailure,
) -> anyhow::Result<()> {
    match failure {
        PreflightFailure::GmailNotConfigured => {
            state.store.append_audit(
                "selection.gmail_not_configured",
                None,
                None,
                Some("no gmail connector configured; /draft is unavailable"),
                None,
                &[],
                &[],
            )?;
            notify_owner_best_effort(
                state,
                chat_id,
                "Gmail isn't configured on this kernel yet, so /draft is unavailable.",
            )
            .await;
        }
        PreflightFailure::RefusedUncontained => {
            state.store.append_audit(
                "route.refused_uncontained",
                None,
                None,
                Some("external_communication lane requires a containing sandbox driver"),
                None,
                &[],
                &[],
            )?;
        }
        PreflightFailure::ThreadNotFound { thread_id } => {
            state.store.append_audit(
                "selection.thread_not_found",
                None,
                None,
                Some(&thread_id),
                None,
                &[],
                &[],
            )?;
            notify_owner_best_effort(
                state,
                chat_id,
                &format!("Couldn't find a Gmail thread with id \"{thread_id}\"."),
            )
            .await;
        }
        PreflightFailure::CounterpartyUnavailable { thread_id } => {
            state.store.append_audit(
                "selection.counterparty_unavailable",
                None,
                None,
                Some(&thread_id),
                None,
                &[],
                &[],
            )?;
            notify_owner_best_effort(
                state,
                chat_id,
                "This Gmail thread has no identifiable counterparty.",
            )
            .await;
        }
        PreflightFailure::GmailError { status, class } => {
            let reason = format!("gmail_error: class={class:?}, status={status:?}");
            state.store.append_audit(
                "selection.gmail_error",
                None,
                None,
                Some(&reason),
                None,
                &[],
                &[],
            )?;
            notify_owner_best_effort(
                state,
                chat_id,
                "Couldn't reach Gmail just now — try again shortly.",
            )
            .await;
        }
    }
    Ok(())
}

use std::time::Duration;

use anyhow::Result;
use jiff::Timestamp;
use openspine_schemas::action::ActionId;

use crate::pipeline::AppState;
use crate::store::failure_surfacing_types::{
    parse_availability_outcome, DetailReceipt, DETAIL_SEMANTIC_KIND,
};

/// Exponential backoff (capped) for a failed retry. `attempts` is the count
/// recorded at claim time, so the first retry backs off `5s`, then `10s`,
/// `20s`, … up to a ~5h ceiling.
fn retry_backoff(attempts: u32) -> Duration {
    const BASE_SECS: u64 = 5;
    const MAX_SHIFTS: u32 = 10;
    let shifts = attempts.saturating_sub(1).min(MAX_SHIFTS);
    Duration::from_secs(BASE_SECS.saturating_mul(1u64 << shifts))
}

/// Resolve a dead-letter's encrypted `text_ref` back to plaintext for resend.
fn resolve_text(state: &AppState, text_ref: &str) -> Result<String> {
    if text_ref.is_empty() {
        return Ok(String::new());
    }
    let digest = openspine_schemas::digest::Digest::parse(text_ref)
        .map_err(|_| anyhow::anyhow!("dead-letter text_ref is not a valid digest"))?;
    let bytes = state
        .artifacts
        .get(&openspine_schemas::artifact::ArtifactRef {
            digest,
            schema_version: 1,
        })
        .map_err(|err| anyhow::anyhow!("resolving dead-letter text artifact: {err}"))?;
    String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("dead-letter text artifact is not utf-8"))
}

/// Run one retry pass. Claiming is compare-and-set guarded and the success
/// and failure terminal transitions are each a single Store transaction
/// conditioned on the claim token, so a stale worker cannot resolve or
/// reopen a reclaimed row, and a crash between send and completion cannot
/// produce a duplicate `owner.notified` (FsReviewFit P1 / D-050).
pub(crate) async fn retry_due_notifications(state: &AppState) -> Result<()> {
    let Some(dead_letter) = state.store.claim_due_dead_letter(Timestamp::now())? else {
        return Ok(());
    };
    let Some(token) = dead_letter.claim_token.as_deref() else {
        anyhow::bail!("claimed dead-letter had no claim token");
    };
    let text = match resolve_text(state, &dead_letter.text_ref) {
        Ok(text) => text,
        Err(err) => {
            let reason = format!("resource failure resolving notification artifact: {err}");
            crate::failure_surfacing::batch_failure(
                state,
                crate::failure_surfacing::FailureClass::Resource,
                "resource failure resolving notification artifact",
                &reason,
            )?;
            let next = Timestamp::now() + retry_backoff(dead_letter.attempts);
            state.store.reschedule_dead_letter_failure(
                dead_letter.id,
                token,
                next,
                &reason,
                dead_letter.task_grant_id,
            )?;
            return Ok(());
        }
    };
    crate::spend::guard_connector(state, true).await?;
    match crate::api::connector_breaker::call_with_connector_preflight(
        state,
        "telegram",
        Some(&ActionId::new("owner.notify")),
        state
            .connectors
            .telegram()
            .send_reply(dead_letter.chat_id, &text),
    )
    .await
    {
        Ok(()) => {
            // Reconstruct the contract-specific receipt metadata if this
            // dead-letter was a `/digest <ULID>` detail delivery. A generic
            // notification (NULL semantic_kind) records only `owner.notified`.
            // Malformed/missing outcome fails closed to `unavailable` so a
            // retry can never silently emit a "viewed" receipt; negative or
            // zero page integers are rejected (fail-closed) rather than
            // wrapped to a huge usize by `as`.
            let detail = match dead_letter.semantic_kind.as_deref() {
                Some(DETAIL_SEMANTIC_KIND) => {
                    let (viewable, reason) =
                        parse_availability_outcome(dead_letter.availability_outcome.as_deref())
                            .unwrap_or((false, Some("missing availability outcome".to_string())));
                    let page_index = dead_letter
                        .page_index
                        .and_then(|p| usize::try_from(p).ok())
                        .filter(|n| *n > 0)
                        .unwrap_or(1);
                    let page_count = dead_letter
                        .page_count
                        .and_then(|p| usize::try_from(p).ok())
                        .filter(|n| *n > 0)
                        .unwrap_or(1);
                    Some(DetailReceipt {
                        detail_ref: dead_letter.detail_ref.clone(),
                        page_index,
                        page_count,
                        unavailable_reason: if viewable {
                            None
                        } else {
                            reason.or(Some("unavailable".to_string()))
                        },
                    })
                }
                _ => None,
            };
            // Send already happened; the only durable question is whether
            // *this* claim still owns the row. If it does not, a concurrent
            // pass already delivered it and we must not write a second
            // `owner.notified` (or a second detail receipt) — drop silently.
            let applied = state.store.complete_dead_letter_success(
                dead_letter.id,
                token,
                dead_letter.task_grant_id,
                &dead_letter.digest_item_ids,
                detail.as_ref(),
            )?;
            if !applied {
                tracing::warn!(
                    dead_letter = %dead_letter.id,
                    "dead-letter claim was reclaimed before success commit; not double-recording"
                );
            }
        }
        Err(err) => {
            let backoff = retry_backoff(dead_letter.attempts);
            let next = Timestamp::now() + backoff;
            let applied = state.store.reschedule_dead_letter_failure(
                dead_letter.id,
                token,
                next,
                &err.to_string(),
                dead_letter.task_grant_id,
            )?;
            if !applied {
                tracing::warn!(
                    dead_letter = %dead_letter.id,
                    "dead-letter claim was reclaimed before failure commit; not re-recording"
                );
            }
        }
    }
    Ok(())
}

/// Kernel-owned supervised retry loop.
pub(crate) async fn run_retry_loop(state: &AppState) -> Result<()> {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        if let Err(err) = retry_due_notifications(state).await {
            tracing::error!(error = %err, "dead-letter retry pass failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_exponential_and_capped() {
        assert_eq!(retry_backoff(1), Duration::from_secs(5));
        assert_eq!(retry_backoff(2), Duration::from_secs(10));
        assert_eq!(retry_backoff(3), Duration::from_secs(20));
        assert_eq!(retry_backoff(4), Duration::from_secs(40));
        assert_eq!(retry_backoff(20), Duration::from_secs(5 * 1024));
    }
}

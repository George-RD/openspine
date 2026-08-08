//! Telegram presentation/input adapter for canonical owner reviews.

use openspine_schemas::owner_review::DecisionIntent;
use openspine_schemas::owner_surface::OwnerSurfaceRef;
use ulid::Ulid;

use super::owner_review_decision::{
    submit_owner_review_callback_async, OwnerReviewDecisionOutcome,
};
use super::{notify_owner_best_effort, AppState};
pub(crate) async fn handle_owner_review_callback(
    state: &AppState,
    owner_surface: &OwnerSurfaceRef,
    callback_query_id: &str,
    review_id: Ulid,
    digest_token: &str,
    intent: DecisionIntent,
) -> anyhow::Result<()> {
    let outcome = submit_owner_review_callback_async(
        state,
        owner_surface,
        review_id,
        digest_token,
        intent,
        jiff::Timestamp::now(),
    )
    .await;
    crate::spend::guard_connector(state, true).await?;
    state
        .connectors
        .telegram()
        .answer_callback_query(callback_query_id)
        .await?;

    match outcome {
        Ok(OwnerReviewDecisionOutcome::Inspected(rendered)) => {
            state
                .connectors
                .telegram()
                .send_owner_review(
                    owner_surface,
                    &rendered.text,
                    rendered.review_id,
                    &rendered.binding_digest,
                    &rendered.intents,
                )
                .await?;
        }
        Ok(OwnerReviewDecisionOutcome::Committed {
            receipt,
            replacement,
            ..
        }) => {
            notify_owner_best_effort(state, owner_surface, &receipt).await;
            if let Some(rendered) = replacement {
                state
                    .connectors
                    .telegram()
                    .send_owner_review(
                        owner_surface,
                        &rendered.text,
                        rendered.review_id,
                        &rendered.binding_digest,
                        &rendered.intents,
                    )
                    .await?;
            }
        }
        Err(error) => {
            notify_owner_best_effort(
                state,
                owner_surface,
                &format!("Review decision refused: {error}"),
            )
            .await;
        }
    }
    Ok(())
}

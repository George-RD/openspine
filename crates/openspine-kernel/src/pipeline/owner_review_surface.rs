//! Channel-neutral rendering and persistence for owner reviews.

use jiff::Timestamp;
use openspine_schemas::digest::Digest;
use openspine_schemas::owner_review::{DecisionIntent, OwnerReviewRequest};
use ulid::Ulid;

use super::AppState;

/// D-045 (WYSIWYS) generalised from previews to reviews. A channel's fit test
/// is the channel's own truncator: if the D-045 truncator would alter the full
/// rendering, the owner could not have read it in full, so the review must not
/// be persisted as approvable. There is deliberately no second size constant
/// here — a private byte budget would disagree with what Telegram actually
/// enforces (UTF-16 units) the moment the copy contains non-ASCII text.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedOwnerReview {
    pub review_id: Ulid,
    pub binding_digest: Digest,
    pub text: String,
    pub intents: Vec<DecisionIntent>,
}

#[derive(Debug, thiserror::Error)]
pub enum OwnerReviewSurfaceError {
    #[error("owner review is not binding-valid")]
    InvalidBinding,
    #[error(
        "owner review rendering does not fit one {channel} message in full, so it cannot be \
         persisted as approvable (D-045 WYSIWYS)"
    )]
    RenderingTooLarge { channel: &'static str },
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    #[error(transparent)]
    Store(#[from] crate::store::StoreError),
    #[error(transparent)]
    Artifact(#[from] crate::artifact_store::ArtifactStoreError),
}

pub(crate) trait OwnerReviewRenderer {
    /// The channel's own name, for the refusal message.
    const CHANNEL: &'static str;

    /// Whether this channel can show `text` in full. Implementations MUST
    /// delegate to the channel's existing truncation mechanism rather than
    /// introduce a size budget of their own.
    fn fits(text: &str) -> bool;

    fn render(
        state: &AppState,
        review: &OwnerReviewRequest,
    ) -> Result<RenderedOwnerReview, OwnerReviewSurfaceError> {
        render_owner_review(state, review, Self::CHANNEL, Self::fits)
    }
}

pub(crate) struct TelegramOwnerReviewRenderer;
impl OwnerReviewRenderer for TelegramOwnerReviewRenderer {
    const CHANNEL: &'static str = "Telegram";

    /// Reuses D-045's `truncate_for_telegram` verbatim: the rendering fits
    /// exactly when the truncator leaves it unchanged.
    fn fits(text: &str) -> bool {
        crate::api::telegram_truncate::truncate_for_telegram(text) == text
    }
}

pub(crate) struct TerminalOwnerReviewRenderer;
impl OwnerReviewRenderer for TerminalOwnerReviewRenderer {
    const CHANNEL: &'static str = "terminal";

    /// The local terminal streams a reply of any length, so it truncates
    /// nothing and every rendering fits.
    fn fits(_text: &str) -> bool {
        true
    }
}

pub(crate) fn render_owner_review(
    state: &AppState,
    review: &OwnerReviewRequest,
    channel: &'static str,
    fits: fn(&str) -> bool,
) -> Result<RenderedOwnerReview, OwnerReviewSurfaceError> {
    if !review.binding_is_valid() || !review.reviewed_scope.binding_is_valid() {
        return Err(OwnerReviewSurfaceError::InvalidBinding);
    }
    let readiness = if state.is_execution_backed(review.reviewed_scope.action_id()) {
        "ready"
    } else {
        "unavailable (approval cannot create an executable effect path)"
    };
    let decisions = review
        .available_decisions
        .iter()
        .map(|decision| format!("{decision:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let controls = review
        .lifecycle_controls
        .iter()
        .map(|control| format!("{control:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let scope = serde_json::to_string_pretty(&review.reviewed_scope)?;
    let text = format!(
        "Responsibility review {}\n\n{}\n\nAction: {}\nScope digest: {}\nEvidence: {:?} ({} qualifying event(s))\nEvidence basis: {}\nExecutor: {}\nRemaining boundaries: {}\nLimits: {}\nFallback: {:?}\nDecisions: {}\nLifecycle: {}\n\nReviewed scope:\n{}\n\nBinding digest: {}",
        review.id,
        review.description,
        review.reviewed_scope.action_id(),
        review.reviewed_scope.context_class_digest().as_str(),
        review.provenance.kind,
        review.provenance.evidence_count,
        review.provenance.summary,
        readiness,
        review.remaining_boundaries.join(", "),
        serde_json::to_string(&review.limits)?,
        review.fallback_behavior,
        decisions,
        controls,
        scope,
        review.binding_digest().as_str(),
    );
    if !fits(&text) {
        return Err(OwnerReviewSurfaceError::RenderingTooLarge { channel });
    }
    let intents = decision_intents(state, review);
    Ok(RenderedOwnerReview {
        review_id: review.id,
        binding_digest: review.binding_digest().clone(),
        text,
        intents,
    })
}

fn decision_intents(state: &AppState, review: &OwnerReviewRequest) -> Vec<DecisionIntent> {
    use openspine_schemas::owner_review::{
        OwnerReviewDecision as Decision, ResponsibilityLifecycleControl as Control,
    };
    let mut intents = review
        .available_decisions
        .iter()
        .map(|decision| match decision {
            Decision::Approve => DecisionIntent::Approve,
            Decision::Reject => DecisionIntent::Reject,
            Decision::Narrow => DecisionIntent::Narrow,
            Decision::Edit => DecisionIntent::Edit,
        })
        .collect::<Vec<_>>();
    if state
        .store
        .owner_review_standing_rule(review.id)
        .ok()
        .flatten()
        .is_some()
    {
        intents.extend(
            review
                .lifecycle_controls
                .iter()
                .map(|control| match control {
                    Control::Pause => DecisionIntent::Pause,
                    Control::Resume => DecisionIntent::Resume,
                    Control::Expire => DecisionIntent::Expire,
                    Control::Revoke => DecisionIntent::Revoke,
                }),
        );
    }
    intents.push(DecisionIntent::Inspect);
    intents
}

/// Persist only after proving the complete canonical review is renderable on
/// the smallest supported owner surface. No truncation is permitted.
pub(crate) fn persist_owner_review(
    state: &AppState,
    review: &OwnerReviewRequest,
    owner_principal_id: Ulid,
    expires_at: Timestamp,
    now: Timestamp,
    supersedes: Option<(Ulid, &Digest)>,
) -> Result<RenderedOwnerReview, OwnerReviewSurfaceError> {
    let rendered = TelegramOwnerReviewRenderer::render(state, review)?;
    let bytes = serde_json::to_vec(review)?;
    let artifact_ref = state.artifacts.put(&bytes)?;
    if let Some((original_id, original_digest)) = supersedes {
        state.store.insert_narrowed_owner_review(
            (original_id, original_digest),
            review.id,
            &artifact_ref,
            owner_principal_id,
            expires_at,
            now,
        )?;
    } else {
        state.store.insert_owner_review(
            review.id,
            &artifact_ref,
            owner_principal_id,
            expires_at,
            now,
        )?;
    }
    Ok(rendered)
}

pub(crate) fn render_decision_receipt(
    intent: DecisionIntent,
    review_id: Ulid,
    digest: &Digest,
    replayed: bool,
    detail: &str,
) -> String {
    let replay = if replayed { " (replay: unchanged)" } else { "" };
    format!(
        "Review {review_id}: {intent:?}{replay}\nBinding digest: {}\nOutcome: {detail}",
        digest.as_str()
    )
}

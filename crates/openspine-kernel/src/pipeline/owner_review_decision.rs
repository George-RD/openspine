//! Principal- and digest-bound owner-review decision routing.

use jiff::Timestamp;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::digest::Digest;
use openspine_schemas::owner_review::{DecisionIntent, OwnerReviewRequest, OwnerReviewState};
use openspine_schemas::owner_surface::{OwnerSurfaceKind, OwnerSurfaceRef};
use openspine_schemas::reviewed_scope::ReviewedActionScope;
use openspine_schemas::standing_rule::{ReviewedScopeBinding, StandingRuleManifest};
use ulid::Ulid;

use super::owner_review::resume_standing_rule_revalidated;
use super::owner_review_surface::{
    persist_owner_review, render_decision_receipt, OwnerReviewRenderer, OwnerReviewSurfaceError,
    RenderedOwnerReview, TelegramOwnerReviewRenderer, TerminalOwnerReviewRenderer,
};
use super::AppState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OwnerReviewDecisionOutcome {
    Inspected(RenderedOwnerReview),
    Committed {
        receipt: String,
        replacement: Option<RenderedOwnerReview>,
        replayed: bool,
    },
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum OwnerReviewDecisionError {
    #[error("owner review does not exist")]
    UnknownReview,
    #[error("owner surface principal does not match the review owner")]
    PrincipalMismatch,
    #[error("owner review has expired")]
    Expired,
    #[error("rendered decision digest does not match the stored review")]
    DigestMismatch,
    #[error("decision intent is not available on this review")]
    IntentNotPermitted,
    #[error("Narrow requires a complete, strictly narrower reviewed scope")]
    MissingNarrowedScope,
    #[error("lifecycle intent requires a review-bound standing rule")]
    MissingLifecycleSubject,
    #[error("Edit requires submission of a replacement proposal")]
    EditRequiresReplacement,
    #[error("lifecycle transition refused: {0}")]
    LifecycleRefused(String),
    #[error("reviewed action has no ready executor")]
    ExecutorUnavailable,
    #[error(transparent)]
    Store(#[from] crate::store::StoreError),
    #[error(transparent)]
    Artifact(#[from] crate::artifact_store::ArtifactStoreError),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    #[error(transparent)]
    Surface(#[from] OwnerReviewSurfaceError),
    #[error(transparent)]
    Narrow(#[from] openspine_schemas::owner_review::OwnerReviewNarrowError),
}

pub(crate) fn load_owner_review_for_surface(
    state: &AppState,
    surface: &OwnerSurfaceRef,
    review_id: Ulid,
    digest_token: &str,
) -> Result<OwnerReviewRequest, OwnerReviewDecisionError> {
    let row = state
        .store
        .owner_review_row(review_id)?
        .ok_or(OwnerReviewDecisionError::UnknownReview)?;
    if row.owner_principal_id != surface.principal_id() {
        return Err(OwnerReviewDecisionError::PrincipalMismatch);
    }
    let bytes = state.artifacts.get(&row.artifact_ref)?;
    let review: OwnerReviewRequest = serde_json::from_slice(&bytes)?;
    if review.id != review_id || !review.binding_is_valid() {
        return Err(OwnerReviewDecisionError::DigestMismatch);
    }
    let full = review.binding_digest().as_str();
    let short = &full["sha256:".len()..][..12];
    if digest_token != full && digest_token.to_ascii_lowercase() != short {
        return Err(OwnerReviewDecisionError::DigestMismatch);
    }
    Ok(review)
}

pub(crate) fn submit_owner_review_callback(
    state: &AppState,
    surface: &OwnerSurfaceRef,
    review_id: Ulid,
    digest_token: &str,
    intent: DecisionIntent,
    now: Timestamp,
) -> Result<OwnerReviewDecisionOutcome, OwnerReviewDecisionError> {
    let review = match load_owner_review_for_surface(state, surface, review_id, digest_token) {
        Ok(review) => review,
        Err(error) => {
            state.store.record_owner_review_refusal(
                review_id,
                surface.principal_id(),
                intent,
                &error.to_string(),
            )?;
            return Err(error);
        }
    };
    submit_owner_review_decision(
        state,
        surface,
        review_id,
        review.binding_digest(),
        intent,
        None,
        now,
    )
}

pub(crate) fn submit_owner_review_decision(
    state: &AppState,
    surface: &OwnerSurfaceRef,
    review_id: Ulid,
    rendered_binding_digest: &Digest,
    intent: DecisionIntent,
    narrowed_scope: Option<ReviewedActionScope>,
    now: Timestamp,
) -> Result<OwnerReviewDecisionOutcome, OwnerReviewDecisionError> {
    let outcome = submit_owner_review_decision_inner(
        state,
        surface,
        review_id,
        rendered_binding_digest,
        intent,
        narrowed_scope,
        now,
    );
    if let Err(error) = &outcome {
        state.store.record_owner_review_refusal(
            review_id,
            surface.principal_id(),
            intent,
            &error.to_string(),
        )?;
    }
    outcome
}

fn submit_owner_review_decision_inner(
    state: &AppState,
    surface: &OwnerSurfaceRef,
    review_id: Ulid,
    rendered_binding_digest: &Digest,
    intent: DecisionIntent,
    narrowed_scope: Option<ReviewedActionScope>,
    now: Timestamp,
) -> Result<OwnerReviewDecisionOutcome, OwnerReviewDecisionError> {
    let row = state
        .store
        .owner_review_row(review_id)?
        .ok_or(OwnerReviewDecisionError::UnknownReview)?;
    if row.owner_principal_id != surface.principal_id() {
        return Err(OwnerReviewDecisionError::PrincipalMismatch);
    }
    if now >= row.expires_at {
        if row.state == OwnerReviewState::Pending {
            state.store.transition_owner_review(
                review_id,
                OwnerReviewState::Pending,
                OwnerReviewState::Expired,
                "decision_after_expiry",
            )?;
        }
        return Err(OwnerReviewDecisionError::Expired);
    }
    let bytes = state.artifacts.get(&row.artifact_ref)?;
    let review: OwnerReviewRequest = serde_json::from_slice(&bytes)?;
    if review.id != row.id
        || !review.binding_is_valid()
        || review.binding_digest() != rendered_binding_digest
    {
        return Err(OwnerReviewDecisionError::DigestMismatch);
    }
    if !intent.is_permitted(&review.available_decisions, &review.lifecycle_controls) {
        return Err(OwnerReviewDecisionError::IntentNotPermitted);
    }
    if intent == DecisionIntent::Inspect {
        state.store.record_owner_review_inspection(
            review.id,
            surface.principal_id(),
            rendered_binding_digest,
        )?;
        let rendered = match surface.kind() {
            OwnerSurfaceKind::TelegramPrivate => {
                TelegramOwnerReviewRenderer::render(state, &review)?
            }
            OwnerSurfaceKind::LocalTerminal | OwnerSurfaceKind::WebOrMobile => {
                TerminalOwnerReviewRenderer::render(state, &review)?
            }
        };
        return Ok(OwnerReviewDecisionOutcome::Inspected(rendered));
    }
    let intent_name = format!("{intent:?}");
    if state
        .store
        .owner_review_last_decision(review_id)?
        .is_some_and(|(previous_intent, previous_digest)| {
            previous_intent == intent_name && previous_digest == *rendered_binding_digest
        })
    {
        let receipt = render_decision_receipt(
            intent,
            review_id,
            rendered_binding_digest,
            true,
            "the previously committed outcome remains in force",
        );
        return Ok(OwnerReviewDecisionOutcome::Committed {
            receipt,
            replacement: None,
            replayed: true,
        });
    }

    match intent {
        DecisionIntent::Approve => {
            if !state.is_execution_backed(review.reviewed_scope.action_id()) {
                return Err(OwnerReviewDecisionError::ExecutorUnavailable);
            }
            let manifest = standing_rule_from_review(&review);
            state.store.approve_owner_review_and_activate_rule(
                review.id,
                surface.principal_id(),
                rendered_binding_digest,
                &manifest,
                now,
            )?;
            Ok(OwnerReviewDecisionOutcome::Committed {
                receipt: render_decision_receipt(
                    intent,
                    review.id,
                    rendered_binding_digest,
                    false,
                    &format!(
                        "activated standing rule {}; each matching effect still crosses fresh grant admission and gate",
                        manifest.id
                    ),
                ),
                replacement: None,
                replayed: false,
            })
        }
        DecisionIntent::Reject => commit_disposition(
            state,
            &review,
            surface.principal_id(),
            rendered_binding_digest,
            intent,
            OwnerReviewState::Rejected,
            "rejected exact stored review",
        ),
        DecisionIntent::Narrow => {
            let narrowed_scope =
                narrowed_scope.ok_or(OwnerReviewDecisionError::MissingNarrowedScope)?;
            let replacement = review.narrowed_review(Ulid::new(), narrowed_scope)?;
            let rendered = persist_owner_review(
                state,
                &replacement,
                row.owner_principal_id,
                row.expires_at,
                now,
                Some((review.id, rendered_binding_digest)),
            )?;
            let receipt = render_decision_receipt(
                intent,
                review.id,
                rendered_binding_digest,
                false,
                &format!(
                    "created narrowed review {} with binding digest {}",
                    replacement.id,
                    replacement.binding_digest().as_str()
                ),
            );
            Ok(OwnerReviewDecisionOutcome::Committed {
                receipt,
                replacement: Some(rendered),
                replayed: false,
            })
        }
        DecisionIntent::Pause
        | DecisionIntent::Resume
        | DecisionIntent::Expire
        | DecisionIntent::Revoke => commit_lifecycle(
            state,
            &review,
            row.state,
            surface.principal_id(),
            rendered_binding_digest,
            intent,
            now,
        ),
        DecisionIntent::Edit => Err(OwnerReviewDecisionError::EditRequiresReplacement),
        DecisionIntent::Inspect => unreachable!("Inspect returned before mutation routing"),
    }
}

fn standing_rule_from_review(review: &OwnerReviewRequest) -> StandingRuleManifest {
    StandingRuleManifest {
        id: format!("owner-review-{}", review.id),
        schema_version: 1,
        version: review.review_version,
        lifecycle_state: Lifecycle::Active,
        action_id: review.reviewed_scope.action_id().clone(),
        description: review.description.clone(),
        quota: review.limits.quota,
        rate: review.limits.rate,
        expires_after_secs: review.limits.expires_after_secs,
        dark_window: None,
        reviewed_scope: Some(ReviewedScopeBinding::derive_from(
            review.reviewed_scope.clone(),
            review.compatibility_digest.clone(),
        )),
    }
}

fn commit_disposition(
    state: &AppState,
    review: &OwnerReviewRequest,
    owner_principal_id: Ulid,
    digest: &Digest,
    intent: DecisionIntent,
    to: OwnerReviewState,
    detail: &str,
) -> Result<OwnerReviewDecisionOutcome, OwnerReviewDecisionError> {
    state.store.commit_owner_review_decision(
        review.id,
        intent,
        owner_principal_id,
        digest,
        Some((OwnerReviewState::Pending, to)),
    )?;
    Ok(OwnerReviewDecisionOutcome::Committed {
        receipt: render_decision_receipt(intent, review.id, digest, false, detail),
        replacement: None,
        replayed: false,
    })
}

fn commit_lifecycle(
    state: &AppState,
    review: &OwnerReviewRequest,
    review_state: OwnerReviewState,
    owner_principal_id: Ulid,
    digest: &Digest,
    intent: DecisionIntent,
    now: Timestamp,
) -> Result<OwnerReviewDecisionOutcome, OwnerReviewDecisionError> {
    let (rule_id, version) = state
        .store
        .owner_review_standing_rule(review.id)?
        .ok_or(OwnerReviewDecisionError::MissingLifecycleSubject)?;
    let lifecycle_changed = match intent {
        DecisionIntent::Pause => state.store.pause_standing_rule(&rule_id, now)?,
        DecisionIntent::Resume => {
            resume_standing_rule_revalidated(state, &rule_id, version, now)
                .map_err(|error| OwnerReviewDecisionError::LifecycleRefused(error.to_string()))?
        }
        DecisionIntent::Expire | DecisionIntent::Revoke => {
            state.store.revoke_standing_rule(&rule_id, now)?
        }
        _ => unreachable!("caller restricted lifecycle intents"),
    };
    let review_transition = match intent {
        DecisionIntent::Expire => Some((review_state, OwnerReviewState::Expired)),
        DecisionIntent::Revoke => Some((review_state, OwnerReviewState::Revoked)),
        DecisionIntent::Pause | DecisionIntent::Resume => None,
        _ => unreachable!("caller restricted lifecycle intents"),
    };
    let review_changed = state.store.commit_owner_review_decision(
        review.id,
        intent,
        owner_principal_id,
        digest,
        review_transition,
    )?;
    let changed = match intent {
        DecisionIntent::Pause | DecisionIntent::Resume => lifecycle_changed,
        DecisionIntent::Expire | DecisionIntent::Revoke => lifecycle_changed || review_changed,
        _ => unreachable!("caller restricted lifecycle intents"),
    };
    let detail = if lifecycle_changed {
        format!("standing rule {rule_id} transitioned")
    } else if review_changed {
        format!("review lifecycle transitioned; standing rule {rule_id} was already revoked")
    } else {
        format!("standing rule {rule_id} already had the requested state")
    };
    Ok(OwnerReviewDecisionOutcome::Committed {
        receipt: render_decision_receipt(intent, review.id, digest, !changed, &detail),
        replacement: None,
        replayed: !changed,
    })
}

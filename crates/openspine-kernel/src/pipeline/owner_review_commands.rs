//! Shared textual adapter for terminal and Telegram owner-review commands.

use std::collections::BTreeSet;

use jiff::Timestamp;
use openspine_schemas::action::ReviewedScopeDimension;
use openspine_schemas::owner_review::{DecisionIntent, OwnerReviewRequest};
use openspine_schemas::owner_surface::OwnerSurfaceRef;
use openspine_schemas::reviewed_scope::{ReviewedActionScope, ReviewedScopeValue};

use super::owner_review_decision::{load_owner_review_for_surface, OwnerReviewDecisionOutcome};
use super::AppState;

pub(crate) async fn handle_owner_review_command(
    state: &AppState,
    surface: &OwnerSurfaceRef,
    text: &str,
    now: Timestamp,
) -> Result<Option<String>, anyhow::Error> {
    let Some(mut rest) = text.strip_prefix("/review") else {
        return Ok(None);
    };
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return Ok(None);
    }
    let review_id = take_word(&mut rest)
        .ok_or_else(|| anyhow::anyhow!(usage()))?
        .parse()?;
    let digest_token = take_word(&mut rest).ok_or_else(|| anyhow::anyhow!(usage()))?;
    let intent = parse_intent(take_word(&mut rest).ok_or_else(|| anyhow::anyhow!(usage()))?)
        .ok_or_else(|| anyhow::anyhow!(usage()))?;
    let review = load_owner_review_for_surface(state, surface, review_id, digest_token)?;
    let narrowed_scope = if intent == DecisionIntent::Narrow {
        Some(derive_narrowed_scope(&review, rest.trim())?)
    } else {
        if !rest.trim().is_empty() {
            return Err(anyhow::anyhow!(usage()));
        }
        None
    };
    let outcome = match super::owner_review_decision::submit_owner_review_decision_async(
        state,
        surface,
        review_id,
        review.binding_digest(),
        intent,
        narrowed_scope,
        now,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => return Ok(Some(format!("Review decision refused: {error}"))),
    };
    let output = match outcome {
        OwnerReviewDecisionOutcome::Inspected(rendering) => rendering.text,
        OwnerReviewDecisionOutcome::Committed {
            receipt,
            replacement: Some(replacement),
            ..
        } => format!("{receipt}\n\n{}", replacement.text),
        OwnerReviewDecisionOutcome::Committed { receipt, .. } => receipt,
    };
    Ok(Some(output))
}

fn derive_narrowed_scope(
    review: &OwnerReviewRequest,
    delta: &str,
) -> Result<ReviewedActionScope, anyhow::Error> {
    let (dimension, requested) = delta
        .split_once('=')
        .ok_or_else(|| anyhow::anyhow!(narrow_usage()))?;
    let requested = requested
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    if requested.is_empty() {
        return Err(anyhow::anyhow!(narrow_usage()));
    }
    let mut dimensions = review.reviewed_scope.dimensions().clone();
    match dimension.trim() {
        "target" => {
            let Some(ReviewedScopeValue::Target(target)) =
                dimensions.get_mut(&ReviewedScopeDimension::Target)
            else {
                return Err(anyhow::anyhow!("target is not a reviewed dimension"));
            };
            target.refs.retain(|target_ref| {
                target_ref
                    .id
                    .as_deref()
                    .is_some_and(|id| requested.contains(id))
            });
            let retained = target
                .refs
                .iter()
                .filter_map(|target_ref| target_ref.id.as_deref())
                .collect::<BTreeSet<_>>();
            if retained != requested {
                return Err(anyhow::anyhow!("target delta names an unreviewed target"));
            }
        }
        "output_channels" => {
            let Some(ReviewedScopeValue::OutputChannels(channels)) =
                dimensions.get_mut(&ReviewedScopeDimension::OutputChannel)
            else {
                return Err(anyhow::anyhow!(
                    "output_channels is not a reviewed dimension"
                ));
            };
            channels.retain(|channel| requested.contains(channel.as_str()));
            if channels.iter().map(String::as_str).collect::<BTreeSet<_>>() != requested {
                return Err(anyhow::anyhow!("delta names an unreviewed output channel"));
            }
        }
        _ => return Err(anyhow::anyhow!(narrow_usage())),
    }
    review
        .reviewed_scope
        .narrowed(dimensions)
        .map_err(anyhow::Error::from)
}

fn take_word<'a>(input: &mut &'a str) -> Option<&'a str> {
    *input = input.trim_start();
    if input.is_empty() {
        return None;
    }
    let end = input.find(char::is_whitespace).unwrap_or(input.len());
    let word = &input[..end];
    *input = &input[end..];
    Some(word)
}

fn parse_intent(value: &str) -> Option<DecisionIntent> {
    match value.to_ascii_lowercase().as_str() {
        "approve" => Some(DecisionIntent::Approve),
        "reject" => Some(DecisionIntent::Reject),
        "narrow" => Some(DecisionIntent::Narrow),
        "edit" => Some(DecisionIntent::Edit),
        "pause" => Some(DecisionIntent::Pause),
        "resume" => Some(DecisionIntent::Resume),
        "expire" => Some(DecisionIntent::Expire),
        "revoke" => Some(DecisionIntent::Revoke),
        "inspect" => Some(DecisionIntent::Inspect),
        _ => None,
    }
}

fn usage() -> &'static str {
    "usage: /review <review-ulid> <sha256:digest|12-hex-token> <approve|reject|narrow|edit|pause|resume|expire|revoke|inspect> [narrow-delta]"
}

fn narrow_usage() -> &'static str {
    "narrow delta must be exactly one of target=<reviewed-id,...> or output_channels=<name,...>"
}

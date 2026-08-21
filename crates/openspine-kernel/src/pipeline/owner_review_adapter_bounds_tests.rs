//! The four things an owner surface may never do, one named production-path
//! test each (proposal.md's Invariant; design.md's "Authority, containment").
//!
//! Split from `owner_review_surface_tests.rs` for the 500-line gate. Every
//! test here drives a real `handle_terminal_message` / `handle_owner_update`
//! entry point rather than calling the store, so it is evidence that the
//! *wired* surface is bounded, not that a helper could be.

use jiff::Timestamp;
use openspine_schemas::owner_review::OwnerReviewState;
use ulid::Ulid;

use super::owner_review_surface::{
    persist_owner_review, OwnerReviewRenderer, TerminalOwnerReviewRenderer,
};
use super::owner_review_tests::{owner_review, owner_review_with_target_count};
use super::{handle_terminal_message, AppState};
use crate::test_support::fixtures::test_state;

fn terminal_state() -> (AppState, tokio::sync::mpsc::UnboundedReceiver<String>) {
    let mut state = test_state();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    state.terminal_reply_tx = Some(tx);
    (state, rx)
}

fn persist(state: &AppState, review: &openspine_schemas::owner_review::OwnerReviewRequest) {
    let now = Timestamp::now();
    persist_owner_review(
        state,
        review,
        state.owner.principal_id.as_ulid(),
        now + std::time::Duration::from_secs(3600),
        now,
        None,
    )
    .unwrap();
}

fn token(review: &openspine_schemas::owner_review::OwnerReviewRequest) -> String {
    review.binding_digest().as_str()["sha256:".len()..][..12].to_string()
}

/// The terminal's own delta parser rejects a target the reviewed scope never
/// contained, before the kernel guard is ever reached. This covers the
/// *adapter*: it is deliberately NOT evidence for the kernel invariant, which
/// is covered adapter-independently by
/// [`kernel_refuses_a_widened_narrow_regardless_of_the_delta_parser`] and, at
/// the schemas level, by `reviewed_scope::narrow::tests::
/// a_widened_candidate_is_not_a_strict_narrowing`.
#[tokio::test]
async fn terminal_delta_parser_rejects_an_unreviewed_target() {
    let (state, mut rx) = terminal_state();
    let review = owner_review_with_target_count(
        Ulid::new(),
        state.owner.principal_id.as_ulid(),
        "Two reviewed targets".into(),
        2,
    );
    persist(&state, &review);
    let before = state.store.count_owner_reviews().unwrap();

    let command = format!(
        "/review {} {} narrow target=thread-never-reviewed",
        review.id,
        token(&review)
    );
    handle_terminal_message(&state, command).await.unwrap();

    let refusal = rx.recv().await.expect("widening refusal");
    assert!(
        refusal.contains("Review decision refused"),
        "a widening narrow must be refused, got: {refusal}"
    );
    assert_eq!(
        state
            .store
            .owner_review_row(review.id)
            .unwrap()
            .unwrap()
            .state,
        OwnerReviewState::Pending,
        "a refused widening leaves the original review untouched"
    );
    assert_eq!(
        state.store.count_owner_reviews().unwrap(),
        before,
        "a refused widening must not persist a replacement review"
    );
}

/// **A surface cannot mutate lifecycle state.** Pause targets the standing
/// rule an approval binds; on a still-Pending review there is no bound rule,
/// `lifecycle_controls` does not offer Pause, and submitting it anyway is
/// refused without touching the review.
#[tokio::test]
async fn terminal_cannot_mutate_lifecycle_state_before_approval_binds_a_rule() {
    let (state, mut rx) = terminal_state();
    let review = owner_review(
        Ulid::new(),
        state.owner.principal_id.as_ulid(),
        "Nothing is bound yet".into(),
    );
    persist(&state, &review);

    let rendered = TerminalOwnerReviewRenderer::render(&state, &review).unwrap();
    assert!(
        !rendered
            .intents
            .contains(&openspine_schemas::owner_review::DecisionIntent::Pause),
        "a pending review must not offer a lifecycle control"
    );

    handle_terminal_message(
        &state,
        format!("/review {} {} pause", review.id, token(&review)),
    )
    .await
    .unwrap();

    let refusal = rx.recv().await.expect("lifecycle refusal");
    assert!(
        refusal.contains("Review decision refused"),
        "pausing an unbound review must be refused, got: {refusal}"
    );
    assert_eq!(
        state
            .store
            .owner_review_row(review.id)
            .unwrap()
            .unwrap()
            .state,
        OwnerReviewState::Pending
    );
    assert_eq!(
        state.store.owner_review_standing_rule(review.id).unwrap(),
        None,
        "a refused lifecycle intent binds no rule"
    );
}

/// **A surface cannot re-derive authority.** `Inspect` is the one intent
/// exempt from the membership check precisely because it is read-only: it
/// re-renders the stored object and mutates neither the review state nor the
/// standing-rule status, and writes no decision disposition.
#[tokio::test]
async fn inspect_is_read_only_and_writes_no_disposition() {
    let (state, mut rx) = terminal_state();
    let review = owner_review(
        Ulid::new(),
        state.owner.principal_id.as_ulid(),
        "Inspect must not decide anything".into(),
    );
    persist(&state, &review);

    handle_terminal_message(
        &state,
        format!("/review {} {} inspect", review.id, token(&review)),
    )
    .await
    .unwrap();

    let rendered = rx.recv().await.expect("inspect rendering");
    assert!(
        rendered.contains(review.binding_digest().as_str()),
        "inspect re-renders the stored object against its binding digest"
    );
    assert_eq!(
        state
            .store
            .owner_review_row(review.id)
            .unwrap()
            .unwrap()
            .state,
        OwnerReviewState::Pending,
        "inspect causes no state transition"
    );
    assert_eq!(
        state.store.owner_review_decision_digest(review.id).unwrap(),
        None,
        "inspect records no decision"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner_review.decision")
            .unwrap(),
        0,
        "inspect writes no decision audit"
    );
}

/// Copy cannot outrun executor readiness (#127). The rendered review reports
/// readiness from `AppState::is_execution_backed` and never claims the
/// reusable effect path is ready for an action with no registered executor.
#[test]
fn rendered_copy_never_claims_readiness_without_a_registered_executor() {
    let state = test_state();
    let review = owner_review(
        Ulid::new(),
        state.owner.principal_id.as_ulid(),
        "Readiness is derived, not asserted".into(),
    );
    let backed = state.is_execution_backed(review.reviewed_scope.action_id());
    let rendered = TerminalOwnerReviewRenderer::render(&state, &review).unwrap();

    if backed {
        assert!(rendered.text.contains("Executor: ready"));
    } else {
        assert!(
            rendered.text.contains("Executor: unavailable"),
            "an unbacked action must render as unavailable, got: {}",
            rendered.text
        );
        assert!(
            !rendered.text.contains("Executor: ready"),
            "copy must not claim the reusable effect path is ready"
        );
    }
    // Truthfulness is not a matter of phrasing: the rendered claim must equal
    // the catalog query it is derived from.
    assert_eq!(
        rendered.text.contains("Executor: ready"),
        backed,
        "rendered readiness must equal is_execution_backed"
    );
}

/// Narrow's immutability guarantee, end to end on the production path: the
/// replacement is its OWN content-addressed record, the original artifact
/// bytes are byte-identical afterwards, and the original id can no longer be
/// approved against the digest the owner was originally shown.
#[tokio::test]
async fn narrow_persists_a_distinct_record_and_leaves_the_original_bytes_untouched() {
    let (state, mut rx) = terminal_state();
    let review = owner_review_with_target_count(
        Ulid::new(),
        state.owner.principal_id.as_ulid(),
        "Two reviewed targets".into(),
        2,
    );
    persist(&state, &review);

    let original_row = state.store.owner_review_row(review.id).unwrap().unwrap();
    let original_bytes = state.artifacts.get(&original_row.artifact_ref).unwrap();

    handle_terminal_message(
        &state,
        format!(
            "/review {} {} narrow target=thread-1",
            review.id,
            token(&review)
        ),
    )
    .await
    .unwrap();
    let receipt = rx.recv().await.expect("narrow receipt");
    assert!(receipt.contains("created narrowed review"));

    // The replacement is a second, independently content-addressed record.
    assert_eq!(
        state.store.count_owner_reviews().unwrap(),
        2,
        "narrow persists the replacement as its own review row"
    );

    // The original artifact is immutable: same ref, same bytes, and the
    // artifact store re-verifies the digest on this read.
    let after = state.store.owner_review_row(review.id).unwrap().unwrap();
    assert_eq!(
        after.artifact_ref, original_row.artifact_ref,
        "narrow must not repoint the original review at new bytes"
    );
    assert_eq!(
        state.artifacts.get(&after.artifact_ref).unwrap(),
        original_bytes,
        "the original reviewed bytes must be byte-identical after narrowing"
    );
    assert_eq!(after.state, OwnerReviewState::Narrowed);

    // And the narrowed decision cannot be replayed as approval of the broader
    // original, even with the exact digest the owner was first shown.
    handle_terminal_message(
        &state,
        format!("/review {} {} approve", review.id, token(&review)),
    )
    .await
    .unwrap();
    let refusal = rx.recv().await.expect("broader approval refusal");
    assert!(
        refusal.contains("Review decision refused"),
        "the broader original must not be approvable after narrowing, got: {refusal}"
    );
    assert_eq!(
        state
            .store
            .owner_review_row(review.id)
            .unwrap()
            .unwrap()
            .state,
        OwnerReviewState::Narrowed
    );
    // The original's sole recorded disposition is the Narrow, bound to the
    // digest the owner was shown — the refused Approve leaves no trace on it.
    let (last_intent, decided_digest) = state
        .store
        .owner_review_last_decision(review.id)
        .unwrap()
        .expect("the narrow is the original's disposition");
    assert_eq!(last_intent, "Narrow");
    assert_eq!(&decided_digest, review.binding_digest());
}

/// Terminal Reject is a real decision through the production path, not just a
/// membership-table entry: it transitions the review and binds the digest the
/// owner was shown, and it activates nothing.
#[tokio::test]
async fn terminal_reject_transitions_the_review_and_binds_no_rule() {
    let (state, mut rx) = terminal_state();
    let review = owner_review(
        Ulid::new(),
        state.owner.principal_id.as_ulid(),
        "Rejectable review".into(),
    );
    persist(&state, &review);

    handle_terminal_message(
        &state,
        format!("/review {} {} reject", review.id, token(&review)),
    )
    .await
    .unwrap();

    let receipt = rx.recv().await.expect("reject receipt");
    assert!(receipt.contains(review.binding_digest().as_str()));
    assert_eq!(
        state
            .store
            .owner_review_row(review.id)
            .unwrap()
            .unwrap()
            .state,
        OwnerReviewState::Rejected
    );
    assert_eq!(
        state.store.owner_review_standing_rule(review.id).unwrap(),
        None,
        "a rejection binds no standing rule"
    );
    let (intent, digest) = state
        .store
        .owner_review_last_decision(review.id)
        .unwrap()
        .expect("the rejection is the disposition");
    assert_eq!(intent, "Reject");
    assert_eq!(&digest, review.binding_digest());
}

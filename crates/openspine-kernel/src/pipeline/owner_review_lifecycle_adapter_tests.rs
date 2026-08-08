//! Production terminal-path lifecycle refusals and replay semantics.
//!
//! Kept separate from `owner_review_adapter_bounds_tests.rs` so each test
//! module stays below the repository's 500-line Rust source cap. These tests
//! drive `handle_terminal_message`, not the lifecycle helpers directly.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use ulid::Ulid;

use super::owner_review_surface::persist_owner_review;
use super::owner_review_tests::owner_review;
use super::{handle_terminal_message, AppState};
use crate::gmail::GmailConnector;
use crate::test_support::fixtures::{test_state, test_state_with_gmail};

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
        state.owner_principal_id,
        now + std::time::Duration::from_secs(3600),
        now,
        None,
    )
    .unwrap();
}

fn token(review: &openspine_schemas::owner_review::OwnerReviewRequest) -> String {
    review.binding_digest().as_str()["sha256:".len()..][..12].to_string()
}

#[tokio::test]
async fn terminal_review_resume_refusal_is_truthful() {
    let mut state = test_state_with_gmail(GmailConnector::new(
        "client-id".into(),
        "client-secret".into(),
        "refresh-token".into(),
        "owner@example.com".into(),
    ));
    let (reply_tx, mut reply_rx) = tokio::sync::mpsc::unbounded_channel();
    state.terminal_reply_tx = Some(reply_tx);
    let review = owner_review(
        Ulid::new(),
        state.owner_principal_id,
        "Resume refusal review".into(),
    );
    persist(&state, &review);
    let token = token(&review);

    handle_terminal_message(&state, format!("/review {} {} approve", review.id, token))
        .await
        .unwrap();
    let _approval = reply_rx.recv().await.expect("approval receipt");
    let (rule_id, version) = state
        .store
        .owner_review_standing_rule(review.id)
        .unwrap()
        .expect("approval binds a standing rule");
    assert_eq!(version, 1);

    assert!(matches!(
        state
            .store
            .pause_standing_rule(&rule_id, Timestamp::now())
            .unwrap(),
        crate::store::PauseStandingRuleOutcome::Paused
    ));
    for _ in 0..10 {
        state.connectors.record_connector_outcome("gmail", false);
    }
    let decisions_before = state
        .store
        .count_audit_events_of_kind("owner_review.decision")
        .unwrap();

    handle_terminal_message(&state, format!("/review {} {} resume", review.id, token))
        .await
        .unwrap();
    let refusal = reply_rx.recv().await.expect("resume refusal");
    assert!(refusal.contains("Review decision refused"), "{refusal}");
    assert!(refusal.contains("resume_refused_unavailable"), "{refusal}");
    assert!(state
        .store
        .paused_standing_rule(&rule_id, version)
        .unwrap()
        .is_some());
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner_review.decision")
            .unwrap(),
        decisions_before,
        "a refused resume must not write a successful owner-review decision"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("standing_rule.resume_refused_unavailable")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn terminal_review_pause_refusal_is_truthful() {
    let (state, mut reply_rx) = terminal_state();
    let review = owner_review(
        Ulid::new(),
        state.owner_principal_id,
        "Pause refusal review".into(),
    );
    persist(&state, &review);
    let token = token(&review);

    handle_terminal_message(&state, format!("/review {} {} approve", review.id, token))
        .await
        .unwrap();
    let _approval = reply_rx.recv().await.expect("approval receipt");
    let (rule_id, _version) = state
        .store
        .owner_review_standing_rule(review.id)
        .unwrap()
        .expect("approval binds a standing rule");

    // An external pause is an unchanged replay when the owner submits Pause
    // for the first time through this review.
    assert!(matches!(
        state
            .store
            .pause_standing_rule(&rule_id, Timestamp::now())
            .unwrap(),
        crate::store::PauseStandingRuleOutcome::Paused
    ));
    handle_terminal_message(&state, format!("/review {} {} pause", review.id, token))
        .await
        .unwrap();
    let replay = reply_rx.recv().await.expect("already-paused replay");
    assert!(replay.contains("replay: unchanged"), "{replay}");

    handle_terminal_message(&state, format!("/review {} {} resume", review.id, token))
        .await
        .unwrap();
    let _resume = reply_rx.recv().await.expect("resume receipt");

    // Force the active rule through the production expiry transition. The
    // subsequent Pause must refuse `needs_review`, not report a no-op.
    assert!(state
        .store
        .active_standing_rule_for_action(
            &ActionId::new("email.create_draft"),
            Timestamp::now() + std::time::Duration::from_secs(8 * 24 * 3600),
        )
        .unwrap()
        .is_none());
    let decisions_before = state
        .store
        .count_audit_events_of_kind("owner_review.decision")
        .unwrap();
    handle_terminal_message(&state, format!("/review {} {} pause", review.id, token))
        .await
        .unwrap();
    let refusal = reply_rx.recv().await.expect("pause refusal");
    assert!(refusal.contains("Review decision refused"), "{refusal}");
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner_review.decision")
            .unwrap(),
        decisions_before,
        "a refused pause must not write a successful owner-review decision"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("standing_rule.pause_refused")
            .unwrap(),
        1
    );
    assert!(state
        .store
        .owner_review_standing_rule(review.id)
        .unwrap()
        .is_some());
}

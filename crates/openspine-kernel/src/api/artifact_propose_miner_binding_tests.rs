use crate::reflection_miner_runtime::{
    reflection_miner_tick, set_dispatch_test_mutation, DispatchTestMutation,
};
use jiff::Timestamp;
use openspine_schemas::owner_review::OwnerReviewRequest;
use ulid::Ulid;

async fn run_dispatch_control(mutation: DispatchTestMutation) {
    let harness = super::miner_tick_harness().await;
    let (context, target_ref, scope_ref, _) =
        super::resolved_email_context(&harness.state, "thread-1", 'a', '1', Ulid::from(11_u128));
    super::append_approval(&harness, &context, &target_ref, &scope_ref, 'f');
    super::append_approval(&harness, &context, &target_ref, &scope_ref, '0');
    set_dispatch_test_mutation(&harness.state, Some(mutation));
    let result = reflection_miner_tick(&harness.state).await;
    assert!(result.is_err(), "dispatch control must refuse: {result:?}");
    assert_eq!(
        harness.state.store.count_owner_reviews().unwrap(),
        0,
        "a refused dispatch must not persist an owner-review row"
    );
    assert!(
        harness
            .state
            .store
            .active_standing_rules_for_action(
                &openspine_schemas::action::ActionId::new("email.create_draft"),
                Timestamp::now(),
            )
            .unwrap()
            .is_empty(),
        "a refused dispatch must not activate a standing rule"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn miner_proposal_missing_verdict_refuses_without_review_or_activation() {
    run_dispatch_control(DispatchTestMutation::MissingVerdict).await;
}

#[tokio::test(flavor = "current_thread")]
async fn miner_proposal_mismatched_verdict_refuses_without_review_or_activation() {
    run_dispatch_control(DispatchTestMutation::MismatchedDigest).await;
}

#[tokio::test(flavor = "current_thread")]
async fn miner_proposal_non_review_required_refuses_without_review_or_activation() {
    run_dispatch_control(DispatchTestMutation::NonReviewRequired).await;
}

#[tokio::test(flavor = "current_thread")]
async fn miner_proposal_stale_verdict_refuses_without_review_or_activation() {
    run_dispatch_control(DispatchTestMutation::StaleEpoch).await;
}

#[tokio::test(flavor = "current_thread")]
async fn miner_proposal_denied_verdict_refuses_without_review_or_activation() {
    run_dispatch_control(DispatchTestMutation::DeniedVerdict).await;
}

#[tokio::test]
async fn miner_review_approval_denied_verdict_refuses_without_activation() {
    let harness = super::miner_tick_harness().await;
    let (context, target_ref, scope_ref, _) =
        super::resolved_email_context(&harness.state, "thread-1", 'a', '1', Ulid::from(11_u128));
    super::append_approval(&harness, &context, &target_ref, &scope_ref, 'f');
    super::append_approval(&harness, &context, &target_ref, &scope_ref, '0');
    assert_eq!(reflection_miner_tick(&harness.state).await.unwrap(), 1);

    let review_id: Ulid = harness.state.store.with_conn_for_test(|conn| {
        conn.query_row(
            "SELECT id FROM owner_reviews ORDER BY created_at DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
        .parse()
        .unwrap()
    });
    let row = harness
        .state
        .store
        .owner_review_row(review_id)
        .unwrap()
        .expect("dispatch must persist the review before approval");
    let review: OwnerReviewRequest =
        serde_json::from_slice(&harness.state.artifacts.get(&row.artifact_ref).unwrap()).unwrap();
    let binding = review
        .evaluation_binding
        .as_ref()
        .expect("miner review must carry evaluation binding");
    harness.state.store.with_conn_for_test(|conn| {
        conn.execute(
            "UPDATE eval_verdicts SET verdict = 'denied' WHERE id = ?1",
            rusqlite::params![binding.replay_verdict_id.to_string()],
        )
        .unwrap();
    });
    let binding_digest = review.binding_digest();
    let error = crate::pipeline::owner_review_decision::submit_owner_review_decision_async(
        &harness.state,
        &crate::test_support::owner_surface(&harness.state),
        review.id,
        binding_digest,
        openspine_schemas::owner_review::DecisionIntent::Approve,
        None,
        Timestamp::now(),
    )
    .await
    .expect_err("a denied bound verdict must refuse approval");
    assert!(matches!(
        error,
        crate::pipeline::owner_review_decision::OwnerReviewDecisionError::EvaluationBindingRefused(
            _
        )
    ));
    assert!(
        harness
            .state
            .store
            .active_standing_rules_for_action(
                &openspine_schemas::action::ActionId::new("email.create_draft"),
                Timestamp::now(),
            )
            .unwrap()
            .is_empty(),
        "a denied approval must not activate a standing rule"
    );
    assert_eq!(
        harness
            .state
            .store
            .owner_review_row(review.id)
            .unwrap()
            .unwrap()
            .state,
        openspine_schemas::owner_review::OwnerReviewState::Pending,
        "a denied approval leaves the owner review pending"
    );
}

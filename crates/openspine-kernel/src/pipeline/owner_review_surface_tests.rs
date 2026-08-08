//! Production-path tests for channel-neutral owner review surfaces.

use jiff::Timestamp;
use openspine_schemas::action::ReviewedScopeDimension;
use openspine_schemas::digest::Digest;
use openspine_schemas::owner_review::DecisionIntent;
use openspine_schemas::owner_surface::OwnerSurfaceRef;
use openspine_schemas::reviewed_scope::{ReviewedActionScope, ReviewedScopeValue};
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::owner_review_decision::{submit_owner_review_decision, OwnerReviewDecisionError};
use super::owner_review_surface::{
    persist_owner_review, OwnerReviewRenderer, OwnerReviewSurfaceError,
    TelegramOwnerReviewRenderer, TerminalOwnerReviewRenderer,
};
use super::owner_review_tests::{
    email_context_with_target_count, owner_review, owner_review_with_target_count,
};
use super::{handle_owner_update, handle_terminal_message};
use crate::telegram::{CallbackQueryUpdate, TelegramConnector};
use crate::test_support::fixtures::{owner_update, test_state, test_state_with_telegram};

#[test]
fn forged_principal_and_wrong_digest_are_refused_before_review_mutation() {
    let state = test_state();
    let now = Timestamp::now();
    let review = owner_review(
        Ulid::new(),
        state.owner_principal_id,
        "Refusal checks".into(),
    );
    persist_owner_review(
        &state,
        &review,
        state.owner_principal_id,
        now + jiff::SignedDuration::from_hours(1),
        now,
        None,
    )
    .unwrap();

    let forged = OwnerSurfaceRef::authenticated_terminal(Ulid::new());
    assert!(matches!(
        submit_owner_review_decision(
            &state,
            &forged,
            review.id,
            review.binding_digest(),
            DecisionIntent::Approve,
            None,
            now,
        ),
        Err(OwnerReviewDecisionError::PrincipalMismatch)
    ));
    let correct = OwnerSurfaceRef::authenticated_terminal(state.owner_principal_id);
    let wrong_digest = Digest::parse(format!("sha256:{}", "f".repeat(64))).unwrap();
    assert!(matches!(
        submit_owner_review_decision(
            &state,
            &correct,
            review.id,
            &wrong_digest,
            DecisionIntent::Approve,
            None,
            now,
        ),
        Err(OwnerReviewDecisionError::DigestMismatch)
    ));
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner_review.decision_refused")
            .unwrap(),
        2
    );
    assert!(matches!(
        submit_owner_review_decision(
            &state,
            &correct,
            review.id,
            review.binding_digest(),
            DecisionIntent::Inspect,
            None,
            now,
        ),
        Ok(super::owner_review_decision::OwnerReviewDecisionOutcome::Inspected(_))
    ));
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner_review.inspected")
            .unwrap(),
        1
    );

    let row = state.store.owner_review_row(review.id).unwrap().unwrap();
    assert_eq!(
        row.state,
        openspine_schemas::owner_review::OwnerReviewState::Pending
    );
    assert!(state
        .store
        .owner_review_decision_digest(review.id)
        .unwrap()
        .is_none());
    assert!(state
        .store
        .owner_review_standing_rule(review.id)
        .unwrap()
        .is_none());
}

#[test]
fn expired_review_cannot_activate_authority() {
    let state = test_state();
    let now = Timestamp::now();
    let review = owner_review(
        Ulid::new(),
        state.owner_principal_id,
        "Expired review".into(),
    );
    persist_owner_review(&state, &review, state.owner_principal_id, now, now, None).unwrap();
    let surface = OwnerSurfaceRef::authenticated_terminal(state.owner_principal_id);

    assert!(matches!(
        submit_owner_review_decision(
            &state,
            &surface,
            review.id,
            review.binding_digest(),
            DecisionIntent::Approve,
            None,
            now,
        ),
        Err(OwnerReviewDecisionError::Expired)
    ));
    assert_eq!(
        state
            .store
            .owner_review_row(review.id)
            .unwrap()
            .unwrap()
            .state,
        openspine_schemas::owner_review::OwnerReviewState::Expired
    );
    assert!(state
        .store
        .owner_review_standing_rule(review.id)
        .unwrap()
        .is_none());
}
#[test]
fn narrow_creates_a_new_digest_and_preserves_untouched_dimensions() {
    let original_scope = ReviewedActionScope::derive(&email_context_with_target_count(2)).unwrap();
    let mut dimensions = original_scope.dimensions().clone();
    let ReviewedScopeValue::Target(target) = dimensions
        .get_mut(&ReviewedScopeDimension::Target)
        .expect("target is reviewed")
    else {
        panic!("target has the canonical value kind");
    };
    target.refs.truncate(1);
    let narrowed_scope = original_scope.narrowed(dimensions).unwrap();
    assert_ne!(
        narrowed_scope.context_class_digest(),
        original_scope.context_class_digest()
    );
    for (dimension, value) in original_scope.dimensions() {
        if *dimension != ReviewedScopeDimension::Target {
            assert_eq!(narrowed_scope.dimensions().get(dimension), Some(value));
        }
    }
    let original =
        owner_review_with_target_count(Ulid::new(), Ulid::new(), "Original review".into(), 2);
    let mut replacement_dimensions = original.reviewed_scope.dimensions().clone();
    let ReviewedScopeValue::Target(target) = replacement_dimensions
        .get_mut(&ReviewedScopeDimension::Target)
        .unwrap()
    else {
        panic!("target has the canonical value kind");
    };
    target.refs.truncate(1);
    let replacement_scope = original
        .reviewed_scope
        .narrowed(replacement_dimensions)
        .unwrap();
    let replacement = original
        .narrowed_review(Ulid::new(), replacement_scope)
        .unwrap();
    assert_ne!(replacement.id, original.id);
    assert_ne!(replacement.binding_digest(), original.binding_digest());
    assert!(replacement.binding_is_valid());
}

#[tokio::test]
async fn terminal_narrow_delta_constructs_the_replacement_digest_in_kernel() {
    let mut state = test_state();
    let (reply_tx, mut reply_rx) = tokio::sync::mpsc::unbounded_channel();
    state.terminal_reply_tx = Some(reply_tx);
    let review = owner_review_with_target_count(
        Ulid::new(),
        state.owner_principal_id,
        "Two reviewed targets".into(),
        2,
    );
    let now = Timestamp::now();
    persist_owner_review(
        &state,
        &review,
        state.owner_principal_id,
        now + std::time::Duration::from_secs(3600),
        now,
        None,
    )
    .unwrap();
    let token = &review.binding_digest().as_str()["sha256:".len()..][..12];
    let command = format!("/review  {}  {}  narrow  target=thread-1", review.id, token);
    handle_terminal_message(&state, command).await.unwrap();
    let receipt = reply_rx.recv().await.expect("narrow receipt");
    assert!(receipt.contains("created narrowed review"));
    let broader_approve = format!("/review {} {} approve", review.id, token);
    handle_terminal_message(&state, broader_approve)
        .await
        .unwrap();
    let refused = reply_rx.recv().await.expect("broader approval refusal");
    assert!(refused.contains("Review decision refused"));
    assert_eq!(
        state
            .store
            .owner_review_row(review.id)
            .unwrap()
            .unwrap()
            .state,
        openspine_schemas::owner_review::OwnerReviewState::Narrowed
    );
}

#[tokio::test]
async fn terminal_cannot_submit_a_decision_absent_from_the_stored_review() {
    let mut state = test_state();
    let (reply_tx, mut reply_rx) = tokio::sync::mpsc::unbounded_channel();
    state.terminal_reply_tx = Some(reply_tx);
    let review = owner_review(
        Ulid::new(),
        state.owner_principal_id,
        "No Edit decision is stored".into(),
    );
    let now = Timestamp::now();
    persist_owner_review(
        &state,
        &review,
        state.owner_principal_id,
        now + std::time::Duration::from_secs(3600),
        now,
        None,
    )
    .unwrap();
    let token = &review.binding_digest().as_str()["sha256:".len()..][..12];
    handle_terminal_message(&state, format!("/review {} {} edit", review.id, token))
        .await
        .unwrap();
    let refusal = reply_rx.recv().await.expect("membership refusal");
    assert!(refusal.contains("decision intent is not available"));
    assert_eq!(
        state
            .store
            .owner_review_row(review.id)
            .unwrap()
            .unwrap()
            .state,
        openspine_schemas::owner_review::OwnerReviewState::Pending
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner_review.decision_refused")
            .unwrap(),
        1
    );
}

#[test]
fn oversized_review_is_not_persisted_as_approvable() {
    let state = test_state();
    let review = owner_review(
        Ulid::new(),
        state.owner_principal_id,
        "x".repeat(crate::api::telegram_truncate::TELEGRAM_MAX_MESSAGE_UTF16_UNITS + 1),
    );
    let now = Timestamp::now();
    let result = persist_owner_review(
        &state,
        &review,
        state.owner_principal_id,
        now + std::time::Duration::from_secs(3600),
        now,
        None,
    );
    assert!(matches!(
        result,
        Err(OwnerReviewSurfaceError::RenderingTooLarge { .. })
    ));
    assert!(state.store.owner_review_row(review.id).unwrap().is_none());
}

#[tokio::test]
async fn pause_resume_and_revoke_commands_are_replay_safe() {
    let mut state = test_state();
    let (reply_tx, mut reply_rx) = tokio::sync::mpsc::unbounded_channel();
    state.terminal_reply_tx = Some(reply_tx);
    let now = Timestamp::now();
    let review = owner_review(
        Ulid::new(),
        state.owner_principal_id,
        "Lifecycle review".into(),
    );
    let pending_rendered = persist_owner_review(
        &state,
        &review,
        state.owner_principal_id,
        now + std::time::Duration::from_secs(3600),
        now,
        None,
    )
    .unwrap();
    assert!(!pending_rendered.intents.contains(&DecisionIntent::Pause));
    let token = &review.binding_digest().as_str()["sha256:".len()..][..12];
    handle_terminal_message(&state, format!("/review {} {} approve", review.id, token))
        .await
        .unwrap();
    let approval = reply_rx.recv().await.expect("approval receipt");
    assert!(approval.contains("activated standing rule"));
    assert!(state
        .store
        .owner_review_standing_rule(review.id)
        .unwrap()
        .is_some());
    let active_rendered = TelegramOwnerReviewRenderer::render(&state, &review).unwrap();
    assert!(active_rendered.intents.contains(&DecisionIntent::Pause));
    for intent in ["pause", "resume", "revoke"] {
        let command = format!("/review {} {} {intent}", review.id, token);
        handle_terminal_message(&state, command.clone())
            .await
            .unwrap();
        let first = reply_rx.recv().await.expect("first lifecycle receipt");
        assert!(first.contains(review.binding_digest().as_str()));
        handle_terminal_message(&state, command).await.unwrap();
        let replay = reply_rx.recv().await.expect("replay lifecycle receipt");
        assert!(replay.contains("replay: unchanged"));
    }
    for kind in [
        "standing_rule.paused",
        "standing_rule.resumed",
        "standing_rule.revoked",
    ] {
        assert_eq!(state.store.count_audit_events_of_kind(kind).unwrap(), 1);
    }
    assert_eq!(
        state
            .store
            .owner_review_row(review.id)
            .unwrap()
            .unwrap()
            .state,
        openspine_schemas::owner_review::OwnerReviewState::Revoked
    );
    handle_terminal_message(&state, format!("/review {} {} expire", review.id, token))
        .await
        .unwrap();
    let expiry = reply_rx.recv().await.expect("expiry receipt");
    assert!(
        expiry.contains(review.binding_digest().as_str()),
        "{expiry}"
    );
    assert_eq!(
        state
            .store
            .owner_review_row(review.id)
            .unwrap()
            .unwrap()
            .state,
        openspine_schemas::owner_review::OwnerReviewState::Expired
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("standing_rule.revoked")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn telegram_and_terminal_decide_the_same_digest_bound_review_row() {
    let telegram = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/AnswerCallbackQuery"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "result": true
        })))
        .mount(&telegram)
        .await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 1,
                "chat": {"id": 555, "type": "private"},
                "text": "ok"
            }
        })))
        .mount(&telegram)
        .await;
    let connector =
        TelegramConnector::with_api_url("test-token".into(), telegram.uri().parse().unwrap());
    let mut state = test_state_with_telegram(connector);
    let (reply_tx, mut reply_rx) = tokio::sync::mpsc::unbounded_channel();
    state.terminal_reply_tx = Some(reply_tx);
    let review = owner_review(
        Ulid::new(),
        state.owner_principal_id,
        "Create drafts only inside the exact reviewed scope.".into(),
    );
    let now = Timestamp::now();
    let rendered = persist_owner_review(
        &state,
        &review,
        state.owner_principal_id,
        now + std::time::Duration::from_secs(3600),
        now,
        None,
    )
    .unwrap();
    let terminal_rendered = TerminalOwnerReviewRenderer::render(&state, &review).unwrap();
    assert_eq!(terminal_rendered.review_id, rendered.review_id);
    assert_eq!(terminal_rendered.binding_digest, rendered.binding_digest);
    let readiness = if state.is_execution_backed(review.reviewed_scope.action_id()) {
        "Executor: ready"
    } else {
        "Executor: unavailable"
    };
    assert!(rendered.text.contains(readiness));
    assert!(rendered.text.contains("Sending remains denied"));
    assert!(rendered.text.contains(&review.provenance.summary));
    assert!(rendered.text.contains(review.binding_digest().as_str()));

    let token = &review.binding_digest().as_str()["sha256:".len()..][..12];
    let mut update = owner_update("");
    update.text = None;
    update.callback_query = Some(CallbackQueryUpdate {
        id: "owner-review-approve".into(),
        data: Some(format!("or:a:{}:{token}", review.id)),
    });
    handle_owner_update(&state, &update).await.unwrap();
    assert_eq!(
        state
            .store
            .owner_review_row(review.id)
            .unwrap()
            .unwrap()
            .state,
        openspine_schemas::owner_review::OwnerReviewState::Approved
    );
    assert_eq!(
        state.store.owner_review_decision_digest(review.id).unwrap(),
        Some(review.binding_digest().clone())
    );
    let (rule_id, rule_version) = state
        .store
        .owner_review_standing_rule(review.id)
        .unwrap()
        .expect("approval binds the activated rule");
    assert!(state
        .store
        .standing_rule_is_current(&rule_id, rule_version)
        .unwrap());

    let command = format!(
        "/review {} {} approve",
        review.id,
        review.binding_digest().as_str()
    );
    handle_terminal_message(&state, command).await.unwrap();
    let terminal_receipt = reply_rx.recv().await.expect("terminal receipt");
    assert!(terminal_receipt.contains("replay: unchanged"));
    assert!(terminal_receipt.contains(review.binding_digest().as_str()));
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner_review.decision")
            .unwrap(),
        1
    );
}

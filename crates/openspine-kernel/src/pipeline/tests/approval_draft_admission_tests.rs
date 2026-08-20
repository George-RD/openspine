//! Write-admission ordering and pre-effect refusal cases for the approved-
//! draft executor, split from `approval_draft_reconcile_tests.rs` for the
//! 500-line gate.

use super::approval_draft_reconcile_tests::total_pending_draft_write_rows;
use super::*;
use crate::api::effect_executors::EffectOutcome;

#[tokio::test]
async fn rate_limited_write_admission_is_refused_without_a_fence_row() {
    // #127 ordering invariant: the executor takes the write's connector permit
    // BEFORE recording the pending-write fence, so a rejected write admission
    // — which has polled no WRITE future and attempted no provider write — is a
    // true pre-effect refusal that leaves NO fence row at all. An
    // inserted-then-resolved row would falsely record an attempted Gmail write
    // in the reconciliation table.
    //
    // Setup: the default token bucket holds 10 permits refilling every 100ms.
    // Holding 9 leaves exactly one for the executor's live thread fetch, so the
    // subsequent write admission is rate-limited. The breaker stays Closed, so
    // holding the permits has no drop side effects.
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(thread_with_sender("alice@example.com")),
        )
        .mount(&api_server)
        .await;
    // No drafts mock: the rejection must happen before any write is attempted.
    let gmail = gmail_with_token_mock(&token_server, &api_server).await;
    let state = test_state_with_gmail(gmail);
    let grant = approval_fixture_grant();
    let request = approval_fixture_request(
        &state,
        grant.id,
        "Re: invoice",
        "sounds good",
        "alice@example.com",
    );
    let _held: Vec<_> = (0..9)
        .map(|_| {
            state
                .connectors
                .acquire_connector_with_generation("gmail")
                .expect("draining the gmail rate-limit bucket")
        })
        .collect();

    let outcome = crate::pipeline::approval::create_approved_draft(
        &state,
        &grant,
        &request,
        &crate::test_support::owner_surface(&state),
    )
    .await
    .unwrap();

    assert_eq!(
        outcome,
        EffectOutcome::RefusedPreEffect,
        "a rejected write admission never attempted a provider write"
    );
    assert_eq!(
        total_pending_draft_write_rows(&state),
        0,
        "the fence must be recorded only after the write permit is held"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.creation_failed")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.created")
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn definite_write_failure_is_failed_after_attempt_and_resolves_the_fence() {
    // The provider explicitly reported the draft write as failed, so no effect
    // took hold: the outcome is `FailedAfterAttempt` (not `DeliveryUnknown`,
    // which would fence the row open for reconciliation) and the fence it
    // recorded before the call is resolved.
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(thread_with_sender("alice@example.com")),
        )
        .mount(&api_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {"code": 400, "message": "invalid draft"}
        })))
        .mount(&api_server)
        .await;
    let gmail = gmail_with_token_mock(&token_server, &api_server).await;
    let state = test_state_with_gmail(gmail);
    let grant = approval_fixture_grant();
    let request = approval_fixture_request(
        &state,
        grant.id,
        "Re: invoice",
        "sounds good",
        "alice@example.com",
    );

    let outcome = crate::pipeline::approval::create_approved_draft(
        &state,
        &grant,
        &request,
        &crate::test_support::owner_surface(&state),
    )
    .await
    .unwrap();

    assert_eq!(outcome, EffectOutcome::FailedAfterAttempt);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.creation_failed")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.created")
            .unwrap(),
        0
    );
    assert_eq!(
        state.store.count_pending_draft_writes().unwrap(),
        0,
        "a confirmed failure resolves the fence instead of leaving it open"
    );
    assert_eq!(
        total_pending_draft_write_rows(&state),
        1,
        "the fence was recorded before the attempted write"
    );
}

#[tokio::test]
async fn provider_5xx_write_is_delivery_unknown_and_leaves_the_fence_open() {
    // #173: a 502 from drafts.create is NOT a confirmed failure — the draft
    // may have been created before the intermediary answered — so the outcome
    // is `DeliveryUnknown`, the pending-write fence stays `pending` (never
    // resolved), and no retry is auto-sent. Previously a 5xx was collapsed to
    // `FailedAfterAttempt`, resolving the fence and permitting a duplicate
    // provider write on the delegated retry path.
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(thread_with_sender("alice@example.com")),
        )
        .mount(&api_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(ResponseTemplate::new(502).set_body_json(json!({
            "error": {"code": 502, "message": "bad gateway"}
        })))
        .mount(&api_server)
        .await;
    let gmail = gmail_with_token_mock(&token_server, &api_server).await;
    let state = test_state_with_gmail(gmail);
    let grant = approval_fixture_grant();
    let request = approval_fixture_request(
        &state,
        grant.id,
        "Re: invoice",
        "sounds good",
        "alice@example.com",
    );

    let outcome = crate::pipeline::approval::create_approved_draft(
        &state,
        &grant,
        &request,
        &crate::test_support::owner_surface(&state),
    )
    .await
    .unwrap();

    assert_eq!(outcome, EffectOutcome::DeliveryUnknown);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.delivery_unknown")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.creation_failed")
            .unwrap(),
        0,
        "a 5xx is never a confirmed creation failure"
    );
    assert_eq!(
        state.store.count_pending_draft_writes().unwrap(),
        1,
        "the reconciliation fence stays open on an unconfirmed write"
    );
    assert_eq!(
        total_pending_draft_write_rows(&state),
        1,
        "the fence was recorded before the attempted write and left pending"
    );
}

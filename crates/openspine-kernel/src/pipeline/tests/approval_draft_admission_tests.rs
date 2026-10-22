//! Write-outcome classification cases for the approved-draft executor, split
//! from `approval_draft_reconcile_tests.rs` for the 500-line gate. The
//! write-admission ordering pin lives in `write_admission_ordering_tests.rs`.

use super::approval_draft_reconcile_tests::total_pending_draft_write_rows;
use super::*;
use crate::api::effect_executors::EffectDisposition;

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

    assert_eq!(outcome, EffectDisposition::ConfirmedFailure);
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

    assert_eq!(outcome, EffectDisposition::DeliveryUnknown);
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

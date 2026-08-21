//! Reachability proof (#177) for the Telegram approval-callback pending-write
//! fence and the single-flight action-request fence.
//!
//! Census site: the DeliveryUnknown branch of
//! `pipeline::approval_draft::create_approved_draft` (leaves the pending-write
//! fence open) and the `Store::try_consume_action_request` single-flight guard
//! in `pipeline::approval::handle_draft_approval_callback`.
//! Non-test caller: `handle_draft_approval_callback`, routed from the Telegram
//! dispatch entry point `pipeline::handle_owner_update`.
//! Test entry: `handle_owner_update` driven with a raw `approve_draft:{id}`
//! callback update.
//!
//! The kernel scoped-action path is proven by
//! `pending_delivery_unknown_fences_scoped_retry_before_reservation`; this
//! proves the same fence discipline on the *callback* entry point, which does
//! not run through `resolve_scoped_admission`'s reservation lifecycle at all.
//! DeliveryUnknown is forced deterministically with a 200 that carries no draft
//! id (mirroring `delivery_unknown_retains_reservation_and_leaves_fence_open`),
//! so there is no timing dependence.

use super::approval::{
    approval_fixture_grant, approval_fixture_request, approve_callback_update,
    gmail_with_token_mock, thread_with_sender,
};
use crate::pipeline::handle_owner_update;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::test_state_with_gmail_and_telegram;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn count(state: &crate::pipeline::AppState, kind: &str) -> usize {
    state.store.count_audit_events_of_kind(kind).unwrap()
}

#[tokio::test]
async fn callback_delivery_unknown_leaves_fence_open_and_redelivery_is_noop() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(thread_with_sender("alice@example.com")),
        )
        .mount(&api_server)
        .await;
    // 200 OK with no draft id -> DispatchError::DeliveryUnknown (a response
    // that does not prove the write did not land). `.expect(1)` is a second,
    // independent witness that the redelivered callback never reaches a write.
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"no_id": true})))
        .expect(1)
        .mount(&api_server)
        .await;

    let gmail = gmail_with_token_mock(&token_server, &api_server).await;
    let telegram_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/AnswerCallbackQuery"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(&telegram_server)
        .await;
    let state = test_state_with_gmail_and_telegram(
        gmail,
        TelegramConnector::with_api_url(
            "test-token".to_string(),
            telegram_server.uri().parse().unwrap(),
        ),
    );

    let grant = approval_fixture_grant();
    let pending_ref = state.artifacts.put(b"hi").unwrap();
    state
        .store
        .insert_task_grant(
            &grant,
            &pending_ref,
            &crate::test_support::owner_surface(&state),
        )
        .unwrap();
    let request = approval_fixture_request(
        &state,
        grant.id,
        "Re: invoice",
        "sounds good",
        "alice@example.com",
    );
    state.store.insert_action_request(&request).unwrap();
    let update = approve_callback_update(request.id);

    // First tap: the write returns delivery-unknown. The pending-write fence
    // is left OPEN for reconciliation and audited; nothing is reported created.
    assert!(handle_owner_update(&state, &update)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        state.store.count_pending_draft_writes().unwrap(),
        1,
        "a delivery-unknown callback write leaves the reconciliation fence open"
    );
    assert_eq!(
        count(&state, "draft.delivery_unknown"),
        1,
        "the delivery-unknown outcome is durably audited"
    );
    assert_eq!(
        count(&state, "draft.created"),
        0,
        "an unconfirmed write is never reported as created"
    );
    assert!(
        state
            .store
            .find_approval_for_request(request.id)
            .unwrap()
            .is_some(),
        "the first tap did record the digest-bound approval"
    );

    // Second, redelivered callback for the SAME action request (Telegram
    // re-sends the still-live inline button's update). It is a no-op: the
    // action request was already consumed, so it audits as already-handled and
    // never mints a second approval, opens a second pending row, or writes.
    assert!(handle_owner_update(&state, &update)
        .await
        .unwrap()
        .is_none());

    // Pin: this proves two guards at the callback entry point. (1) Remove the
    // `try_consume_action_request` single-flight in
    // `handle_draft_approval_callback` and the redelivery mints a second
    // approval and re-enters `create_approved_draft` — `draft.approval_already_handled`
    // drops to 0 and the `.expect(1)` drafts mock is exceeded. (2) Mutate the
    // DeliveryUnknown branch of `create_approved_draft` to resolve/close the
    // pending row and `count_pending_draft_writes()` drops to 0.
    assert_eq!(
        count(&state, "draft.approval_already_handled"),
        1,
        "the single-flight fence turns the redelivered callback into a no-op"
    );
    assert_eq!(
        state.store.count_pending_draft_writes().unwrap(),
        1,
        "the redelivery neither closes nor duplicates the pending-write fence"
    );
    assert_eq!(
        count(&state, "draft.delivery_unknown"),
        1,
        "no second write is attempted, so no second delivery-unknown is audited"
    );
    assert_eq!(
        count(&state, "draft.created"),
        0,
        "still nothing created after the redelivery"
    );
}

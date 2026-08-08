//! Approval-callback replay and single-use consumption cases, split from
//! `approval.rs` for the 500-line gate.

use super::approval::{
    approval_fixture_grant, approval_fixture_request, approve_callback_update,
    gmail_with_token_mock, thread_with_sender,
};
use super::*;
use crate::telegram::TelegramConnector;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn approval_audit_never_contains_the_plaintext_draft_body() {
    // PRD §18 / D-011: private payloads must be stored as encrypted
    // artifact refs, never written directly into the audit event.
    const SUBJECT: &str = "Re: a rather distinctive invoice subject";
    const BODY: &str = "a rather distinctive draft body sentence";

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
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "draft-1"})))
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
    let request = approval_fixture_request(&state, grant.id, SUBJECT, BODY, "alice@example.com");
    state.store.insert_action_request(&request).unwrap();
    let update = approve_callback_update(request.id);
    assert!(handle_owner_update(&state, &update)
        .await
        .unwrap()
        .is_none());

    let events = state.store.all_audit_event_jsons().unwrap();
    assert!(!events.is_empty());
    for event in &events {
        assert!(
            !event.contains(SUBJECT),
            "audit event leaked the plaintext subject: {event}"
        );
        assert!(
            !event.contains(BODY),
            "audit event leaked the plaintext body: {event}"
        );
    }
}

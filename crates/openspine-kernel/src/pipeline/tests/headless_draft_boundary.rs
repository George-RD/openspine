//! The live webhook headless lane's `email.create_draft` boundary (#127, D-153).
//!
//! Sibling of `headless.rs` so neither file approaches the 500-line gate; it
//! reuses that module's route/signature fixtures.

use super::headless::{install_headless_route, signed_request};
use super::*;
use crate::pipeline::handle_owner_update;
use crate::pipeline::headless::{run_headless_hook, HeadlessHookOutcome};
use crate::telegram::{CallbackQueryUpdate, TelegramConnector};
use jiff::Timestamp;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// D-153's boundary, pinned as behaviour rather than prose.
///
/// The headless approved lane routes ANY approved digest-bound
/// `email.create_draft` through the shared Gmail executor — that is what
/// `effect_executor_tests::headless_and_non_headless_approval_converge_on_gmail_executor`
/// proves. But the request `run_headless_hook` mints from a webhook binds the
/// raw webhook body as its payload and carries `target_ref: None`
/// (`pipeline/headless.rs`), so the executor re-derives an empty Gmail thread
/// id and refuses BEFORE any write: no draft is created and no
/// `headless.approved_dispatched` is appended.
///
/// Resolving a webhook envelope into a draft payload plus a reviewed thread
/// target is `mine-and-match-reusable-authority-by-scope` (#128) resolved-context
/// work, deliberately outside #127. This test exists so that boundary is
/// enforced, not merely documented: if a future change makes the webhook mint
/// produce a draft, it must do so deliberately and update this test.
#[tokio::test]
async fn webhook_minted_headless_draft_refuses_before_any_write() {
    let telegram_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "result": {"message_id": 1, "date": 0, "chat": {"id": 42, "type": "private"}, "text": "escalated"}
        })))
        .mount(&telegram_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/AnswerCallbackQuery"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&telegram_server)
        .await;
    // A Gmail connector IS configured and its OAuth token exchange succeeds, so
    // the refusal cannot be attributed to a missing connector or a credential
    // failure. The thread lookup is mounted for the EMPTY thread id the
    // executor re-derives from `target_ref: None`, and returns a thread with no
    // non-owner recipient — the deterministic pre-effect refusal. The drafts
    // endpoint is mounted with `.expect(0)`: wiremock fails the test if any
    // draft write is ever attempted.
    let token_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "test-access-token",
            "expires_in": 3600,
        })))
        .mount(&token_server)
        .await;
    let api_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "messages": [{
                "payload": {
                    "mimeType": "text/plain",
                    "headers": [{"name": "From", "value": "owner@example.com"}],
                    "body": {"data": "aGk"}
                }
            }]
        })))
        .mount(&api_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "never"})))
        .expect(0)
        .mount(&api_server)
        .await;
    let gmail = crate::gmail::GmailConnector::new(
        "client-id".to_string(),
        "client-secret".to_string(),
        "refresh-token".to_string(),
        "owner@example.com".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), api_server.uri());
    let state = test_state_with_gmail_and_telegram(
        gmail,
        TelegramConnector::with_api_url(
            "test-token".to_string(),
            telegram_server.uri().parse().unwrap(),
        ),
    );
    install_headless_route(&state, "email.create_draft", true, None);
    let now = Timestamp::now();

    let outcome = run_headless_hook(
        &state,
        signed_request(&state, now, "headless-draft-boundary", "email.create_draft"),
        now,
    )
    .await
    .expect("an approval-required headless draft escalates to the owner");
    assert!(
        matches!(outcome, HeadlessHookOutcome::Escalated(_)),
        "a webhook-minted draft must reach the owner, not run silently: {outcome:?}"
    );

    let persisted = state
        .store
        .latest_action_request()
        .unwrap()
        .expect("the headless lane persisted an approval request");
    // The premise of this boundary, asserted rather than assumed: the webhook
    // mint binds no thread target, so the executor has nothing to draft into.
    assert!(
        persisted.target_ref.is_none(),
        "the webhook mint must not fabricate a Gmail thread target"
    );
    assert_eq!(
        persisted.params.get("headless").map(String::as_str),
        Some("true"),
        "the request must route to the headless approved lane"
    );
    let request_id = persisted.id;
    let mut tap = crate::test_support::fixtures::owner_update("");
    tap.text = None;
    tap.chat_id = state.owner_user_id;
    tap.callback_query = Some(CallbackQueryUpdate {
        id: "headless-draft-boundary-approve".to_string(),
        data: Some(format!("approve_draft:{request_id}")),
    });
    handle_owner_update(&state, &tap)
        .await
        .expect("the approval callback completes");

    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.created")
            .unwrap(),
        0,
        "a webhook-minted headless request carries no thread target, so no draft may be created"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("headless.approved_dispatched")
            .unwrap(),
        0,
        "a pre-effect refusal must never be recorded as a dispatched headless effect"
    );
    // Pin the exact branch: the empty re-derived thread yields no non-owner
    // recipient. Without this, an unmatched thread mock would refuse via the
    // fetch-error branch and the test would pass for the wrong reason.
    let events = state.store.all_audit_event_jsons().unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.contains("no non-owner recipient found in thread")),
        "expected the recipient re-derivation refusal, got: {events:?}"
    );
    // TOTAL fence rows, not just open ones: a definite write failure would also
    // leave zero OPEN rows, so only the total distinguishes "refused before the
    // write" from "attempted the write and failed".
    let total_fence_rows: i64 = state.store.with_conn_for_test(|conn| {
        conn.query_row("SELECT COUNT(*) FROM pending_draft_writes", [], |row| {
            row.get(0)
        })
        .unwrap()
    });
    assert_eq!(
        total_fence_rows, 0,
        "refusing before the write records no reconciliation fence at all"
    );
}

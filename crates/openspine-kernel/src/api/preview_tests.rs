//! End-to-end tests for `lyra.ui.preview`: split out of `dispatch_tests.rs`
//! (which now covers only `email.read_thread:selected_no_attachments`)
//! purely to keep that file under the 500-line gate. Proves
//! `lyra.ui.preview` sends to the grant-bound Telegram chat, truncates to
//! Telegram's UTF-16 limit, and — per D-045 (WYSIWYS) — never attaches an
//! approval button (or persists a pending `ActionRequest`) to a preview
//! the owner was not shown in full.

use jiff::Timestamp;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::dispatch_tests::{mint_grant_with_selection_token, OWNER_CHAT_ID};
use super::tests::{post_action, start_server};
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::test_state_with_telegram;

#[tokio::test]
async fn lyra_ui_preview_sends_telegram_reply_to_grant_bound_chat() {
    let server = MockServer::start().await;
    let token = "test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{}/SendMessage", token)))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 7,
                "date": 0,
                "chat": {"id": OWNER_CHAT_ID, "type": "private"},
                "text": "sent",
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let connector =
        TelegramConnector::with_api_url(token.to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);
    let (grant, _token) = mint_grant_with_selection_token(
        &state,
        &["lyra.ui.preview"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );

    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &grant.task_token,
        "lyra.ui.preview",
        Some(json!({
            "subject": "Re: invoice",
            "body": "Here's a draft reply.",
        })),
    )
    .await;
    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "allow");
    assert_eq!(body["result"]["sent"], true);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let request_body: Value = requests[0].body_json().unwrap();
    assert_eq!(request_body["chat_id"], OWNER_CHAT_ID);
    let text = request_body["text"].as_str().unwrap();
    assert!(text.contains("Draft preview"));
    assert!(text.contains("Subject: Re: invoice"));
    assert!(text.contains("Here's a draft reply."));

    handle.abort();
}

#[tokio::test]
async fn lyra_ui_preview_truncates_long_body_to_utf16_limit() {
    let server = MockServer::start().await;
    let token = "test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{}/SendMessage", token)))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 8,
                "date": 0,
                "chat": {"id": OWNER_CHAT_ID, "type": "private"},
                "text": "sent",
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let connector =
        TelegramConnector::with_api_url(token.to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);
    let (grant, _token) = mint_grant_with_selection_token(
        &state,
        &["lyra.ui.preview"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );

    let long_body = "🚀".repeat(3000);
    assert!(
        long_body.encode_utf16().count() > 4000,
        "test body must exceed 4000 UTF-16 units"
    );

    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &grant.task_token,
        "lyra.ui.preview",
        Some(json!({
            "subject": "Re: long thread",
            "body": long_body,
        })),
    )
    .await;
    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "allow");
    assert_eq!(body["result"]["sent"], true);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let request_body: Value = requests[0].body_json().unwrap();
    assert!(
        request_body.get("reply_markup").is_none(),
        "a truncated preview must never carry an approval button (D-045)"
    );
    let text = request_body["text"].as_str().unwrap();
    let notice = "\n\n[Draft too long to approve via Telegram — ask for a shorter draft.]";
    assert!(text.ends_with(notice));
    assert!(
        text.encode_utf16().count() <= 4000,
        "truncated preview + notice must still fit under Telegram's UTF-16 limit"
    );
    assert!(text.encode_utf16().count() < long_body.encode_utf16().count());

    handle.abort();
}

#[tokio::test]
async fn truncated_preview_carries_no_approval_button_and_persists_no_action_request() {
    let server = MockServer::start().await;
    let token = "test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{}/SendMessage", token)))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 9,
                "date": 0,
                "chat": {"id": OWNER_CHAT_ID, "type": "private"},
                "text": "sent",
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let connector =
        TelegramConnector::with_api_url(token.to_string(), server.uri().parse().unwrap());
    let mut state = test_state_with_telegram(connector);
    // D-045's guarantee is about persistence, not just the wire response —
    // re-point the store at a temp file (instead of `:memory:`) so this test
    // can open a second connection after the HTTP round-trip and assert
    // directly against `action_requests`, the same table
    // `propose_draft_creation` would have written to had it been called.
    let db_path = tempfile::tempdir().unwrap().keep().join("kernel.db");
    state.store = crate::store::Store::open(&db_path).unwrap();
    let (grant, _token) = mint_grant_with_selection_token(
        &state,
        &["lyra.ui.preview"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );

    let long_body = "🚀".repeat(3000);
    assert!(
        long_body.encode_utf16().count() > 4000,
        "test body must exceed 4000 UTF-16 units"
    );

    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &grant.task_token,
        "lyra.ui.preview",
        Some(json!({
            "subject": "Re: long thread",
            "body": long_body,
        })),
    )
    .await;
    assert_eq!(resp.status(), 200);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let request_body: Value = requests[0].body_json().unwrap();
    assert!(
        request_body.get("reply_markup").is_none(),
        "a truncated preview must never carry an approval button"
    );

    let reopened = crate::store::Store::open(&db_path).unwrap();
    assert_eq!(
        reopened.count_action_requests().unwrap(),
        0,
        "a truncated preview must never persist a pending ActionRequest"
    );

    handle.abort();
}

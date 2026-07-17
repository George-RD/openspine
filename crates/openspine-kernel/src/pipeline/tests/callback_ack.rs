use crate::pipeline::handle_owner_update;
use crate::telegram::{CallbackQueryUpdate, TelegramConnector, TelegramUpdate};
use crate::test_support::fixtures::{owner_update, test_state_with_gmail_and_telegram};
use serde_json::json;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn failed_callback_ack_does_not_abort_approval_and_is_durable() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "test-token", "expires_in": 3600
        })))
        .mount(&token_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messages": [{"payload": {"mimeType": "text/plain", "headers": [{"name": "From", "value": "alice@example.com"}], "body": {"data": "aGk"}}}]
        })))
        .mount(&api_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "draft-1"})))
        .mount(&api_server)
        .await;
    let gmail = crate::gmail::GmailConnector::new(
        "id".to_string(),
        "secret".to_string(),
        "refresh".to_string(),
        "owner@example.com".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), api_server.uri());

    let telegram_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "result": {"message_id": 1, "date": 0, "chat": {"id": 555, "type": "private"}, "text": "sent"}
        })))
        .mount(&telegram_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/AnswerCallbackQuery"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&telegram_server)
        .await;
    let telegram = TelegramConnector::with_api_url(
        "test-token".to_string(),
        telegram_server.uri().parse().unwrap(),
    );
    let state = test_state_with_gmail_and_telegram(gmail, telegram);
    let grant = super::approval::approval_fixture_grant();
    let pending_ref = state.artifacts.put(b"hi").unwrap();
    state
        .store
        .insert_task_grant(&grant, &pending_ref, 555)
        .unwrap();
    let request = super::approval::approval_fixture_request(
        &state,
        grant.id,
        "Re: invoice",
        "sounds good",
        "alice@example.com",
    );
    state.store.insert_action_request(&request).unwrap();
    let mut update: TelegramUpdate = owner_update("");
    update.text = None;
    update.callback_query = Some(CallbackQueryUpdate {
        id: "cb-failing-ack".to_string(),
        data: Some(format!("approve_draft:{}", request.id)),
    });

    handle_owner_update(&state, &update).await.unwrap();
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.created")
            .unwrap(),
        1
    );
    assert!(state
        .store
        .owner_digest_items()
        .unwrap()
        .iter()
        .any(|item| item.class == "connector"
            && item.summary.contains("callback acknowledgement failed")));
}

//! Shared mock/state helpers for the bot-identity boundary tests
//! (`bot_identity.rs`, `token_rotation.rs`). Split out so no single test
//! module exceeds the 500-line gate.

use crate::telegram::BOT_TOKEN_SLOT;
use crate::test_support::fixtures::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Mount a Telegram `getMe` (for the candidate token) and a permissive
/// `SendMessage` (for the live connector token) so the production
/// `initialize_telegram_bot_id` / `handle_owner_update` paths can run
/// against a mocked bot API.
pub(crate) async fn mount_telegram_with_getme(
    server: &MockServer,
    live_token: &str,
    candidate_token: &str,
    candidate_bot_id: i64,
) {
    Mock::given(method("POST"))
        .and(path(format!("/bot{candidate_token}/GetMe")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "result": {
                "id": candidate_bot_id,
                "is_bot": true,
                "first_name": "test",
                "can_join_groups": true,
                "can_read_all_group_messages": true,
                "supports_inline_queries": false,
                "has_main_web_app": false
            }
        })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/bot{live_token}/SendMessage")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"ok": true, "result": {"message_id": 1}})),
        )
        .mount(server)
        .await;
}

/// Build a state whose Telegram connector shares the AppState vault (same
/// `Arc<SecretStore>`) and points at `server`. Sharing the vault means that
/// after a token rotation the live `current_bot()` reads the promoted token
/// from the vault and polls Telegram with it — exercising production
/// promotion end to end.
pub(crate) fn telegram_state_with_token(
    server: &MockServer,
    live_token: &str,
) -> crate::pipeline::AppState {
    let mut state = test_state();
    let connector = crate::telegram::TelegramConnector::with_store_and_api_url(
        live_token.to_string(),
        state.secrets.clone(),
        crate::telegram::BOT_TOKEN_SLOT.to_string(),
        server.uri().parse().unwrap(),
    );
    state.connectors = crate::connectors::ConnectorRegistry::new(connector, None)
        .expect("built-in egress ratings are conflict-free");
    state
        .secrets
        .put(BOT_TOKEN_SLOT, live_token.as_bytes())
        .unwrap();
    state
}

/// Assert a pasted secret appears in NO kernel-persisted surface and in no
/// model-gateway request body — the production code paths under test must
/// never forward the raw token to the LLM provider.
pub(crate) async fn assert_secret_never_leaks(
    state: &crate::pipeline::AppState,
    sentinel: &str,
    provider: &MockServer,
) {
    for (key, value) in state.store.all_kv_for_test() {
        assert!(
            !value.contains(sentinel),
            "secret leaked into kv_state key {key}: {value}"
        );
    }
    for event in state.store.all_audit_event_jsons().expect("audit rows") {
        assert!(
            !event.contains(sentinel),
            "secret leaked into audit: {event}"
        );
    }
    for request in provider.received_requests().await.unwrap() {
        assert!(
            !String::from_utf8_lossy(&request.body).contains(sentinel),
            "secret leaked into model-gateway payload: {:?}",
            request.body
        );
    }
}

/// Build a Telegram `getUpdates` response body (Telegram Bot API schema)
/// that teloxide can project into `Vec<TelegramUpdate>`. Each entry is a
/// private owner message from user 42.
pub(crate) fn getupdates_body(updates: &[(i64, &str)]) -> serde_json::Value {
    let result: Vec<serde_json::Value> = updates
        .iter()
        .map(|(id, text)| {
            serde_json::json!({
                "update_id": id,
                "message": {
                    "message_id": id,
                    "date": 1,
                    "chat": {"id": 555, "type": "private"},
                    "from": {"id": 42, "is_bot": false, "first_name": "Owner"},
                    "text": text
                }
            })
        })
        .collect();
    serde_json::json!({"ok": true, "result": result})
}

/// Mount a `getUpdates` endpoint returning `updates`, so the production
/// `poll_once` path (real wire + teloxide projection) can be exercised.
pub(crate) async fn mount_getupdates(
    server: &MockServer,
    live_token: &str,
    updates: &[(i64, &str)],
) {
    Mock::given(method("POST"))
        .and(path(format!("/bot{live_token}/GetUpdates")))
        .respond_with(ResponseTemplate::new(200).set_body_json(getupdates_body(updates)))
        .mount(server)
        .await;
}

/// Assert the single `getUpdates` call used exactly `expected` as its
/// `offset` body field (None = offset omitted, proving a fresh namespace).
/// teloxide POSTs request params as a JSON body, not URL query params.
pub(crate) async fn assert_poll_offset(server: &MockServer, expected: Option<i64>) {
    let requests = server.received_requests().await.unwrap();
    let updates: Vec<&wiremock::Request> = requests
        .iter()
        .filter(|r| r.url.path().ends_with("/GetUpdates"))
        .collect();
    assert_eq!(updates.len(), 1, "expected exactly one getUpdates call");
    let body: serde_json::Value = serde_json::from_slice(&updates[0].body)
        .expect("getUpdates request body must be valid JSON");
    match expected {
        Some(o) => assert_eq!(
            body.get("offset").and_then(|v| v.as_i64()),
            Some(o),
            "getUpdates body offset was: {body}"
        ),
        None => assert!(
            body.get("offset").is_none(),
            "fresh namespace must omit offset, body was: {body}"
        ),
    }
}

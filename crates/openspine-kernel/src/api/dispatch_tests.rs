//! End-to-end tests for Phase-2 action dispatch: `email.read_thread:selected_no_attachments`
//! and `lyra.ui.preview`. These exercises go through the real axum router and the real
//! SQLite store to prove the single-use selection token, grant binding, expiry, and payload
//! validation all surface as HTTP-level 400s, and that `lyra.ui.preview` truncates to
//! Telegram's UTF-16 limit before sending.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::event::{AccountRole, Connector};
use openspine_schemas::grant::{GrantLimits, TaskGrant};
use openspine_schemas::selection::{
    SelectionScope, SelectionToken, SelectionTokenType, SelectionVerificationMethod,
};
use serde_json::{json, Value};
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::tests::{post_action, start_server};
use crate::gmail::GmailConnector;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::{test_state_with_gmail, test_state_with_telegram};

const OWNER_CHAT_ID: i64 = 555;

fn sample_gmail_thread_json() -> Value {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    json!({
        "messages": [{
            "payload": {
                "mimeType": "multipart/mixed",
                "headers": [
                    {"name": "From", "value": "alice@example.com"},
                    {"name": "Subject", "value": "Re: invoice"},
                ],
                "parts": [
                    {
                        "mimeType": "text/plain",
                        "body": {"data": URL_SAFE_NO_PAD.encode(b"hello owner")},
                    },
                    {
                        "mimeType": "application/pdf",
                        "filename": "invoice.pdf",
                        "body": {"data": URL_SAFE_NO_PAD.encode(b"not-a-real-pdf")},
                    },
                ],
            },
        }],
    })
}

async fn mount_gmail_token_endpoint(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "test-access-token",
            "expires_in": 3600,
        })))
        .mount(server)
        .await;
}

async fn mount_gmail_thread_endpoint(server: &MockServer, thread_id: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/gmail/v1/users/me/threads/{}", thread_id)))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_gmail_thread_json()))
        .mount(server)
        .await;
}

fn gmail_connector(token_server: &MockServer, api_server: &MockServer) -> GmailConnector {
    GmailConnector::new(
        "client-id".to_string(),
        "client-secret".to_string(),
        "refresh-token".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), api_server.uri())
}

fn mint_grant_with_selection_token(
    state: &crate::pipeline::AppState,
    allowed_actions: &[&str],
    token_expires_at: Timestamp,
) -> (TaskGrant, SelectionToken) {
    let now = Timestamp::now();
    let user = state.owner_user_id.to_string();
    let token = SelectionToken {
        id: Ulid::new(),
        schema_version: 1,
        token_type: SelectionTokenType::EmailThreadSelection,
        user: user.clone(),
        target_id: "thread-1".to_string(),
        selected_by: user.clone(),
        selected_at: now,
        issued_by: "kernel".to_string(),
        expires_at: token_expires_at,
        verified_source: true,
        verification_method: SelectionVerificationMethod::ApprovedOwnerControlSelection,
        connector: Some(Connector::GmailPrimaryConnector),
        account_role: Some(AccountRole::OwnerMailbox),
        scope: SelectionScope {
            read_thread: true,
            attachments_allowed: false,
            max_messages: 20,
            include_headers: true,
            include_recipients: true,
            include_body: true,
        },
        single_use: true,
    };
    state.store.insert_selection_token(&token).unwrap();

    let grant = TaskGrant {
        id: Ulid::new(),
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user,
        purpose: "selected_thread_email_reply_draft".to_string(),
        issued_by: "kernel".to_string(),
        issued_at: now,
        expires_at: now + std::time::Duration::from_secs(120),
        event_id: Ulid::new(),
        route_id: "owner_email_selected_thread".to_string(),
        agent_id: "email_reply_drafter".to_string(),
        workflow_id: "selected_thread_email_reply_draft".to_string(),
        capability_pack_id: "selected_thread_email_draft_pack".to_string(),
        authority_sources: vec![],
        selection_tokens: vec![token.id],
        allowed_actions: allowed_actions.iter().map(|a| ActionId::new(*a)).collect(),
        approval_required_actions: vec![],
        denied_actions: vec![],
        output_channels: vec!["telegram.owner.reply".to_string()],
        limits: GrantLimits {
            max_model_calls: 8,
            max_artifacts: 20,
            max_runtime_seconds: 120,
        },
        task_token: Ulid::new().to_string(),
    };
    let pending_ref = state.artifacts.put(b"test pending".as_slice()).unwrap();
    state
        .store
        .insert_task_grant(&grant, &pending_ref, OWNER_CHAT_ID)
        .unwrap();

    (grant, token)
}

#[tokio::test]
async fn email_read_selected_thread_returns_thread_via_mocked_gmail() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    mount_gmail_token_endpoint(&token_server).await;
    mount_gmail_thread_endpoint(&api_server, "thread-1").await;

    let state = test_state_with_gmail(gmail_connector(&token_server, &api_server));
    let (grant, token) = mint_grant_with_selection_token(
        &state,
        &["email.read_thread:selected_no_attachments"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );

    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({ "selection_token_id": token.id.to_string() })),
    )
    .await;
    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "allow");
    assert_eq!(body["result"]["thread_id"], "thread-1");
    let messages = body["result"]["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["from"], "alice@example.com");
    assert_eq!(messages[0]["subject"], "Re: invoice");
    assert_eq!(messages[0]["body_text"], "hello owner");
    assert!(!messages[0]["body_text"]
        .as_str()
        .unwrap()
        .contains("not-a-real-pdf"));

    handle.abort();
}

#[tokio::test]
async fn email_read_selected_thread_rejects_second_use() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    mount_gmail_token_endpoint(&token_server).await;
    mount_gmail_thread_endpoint(&api_server, "thread-1").await;

    let state = test_state_with_gmail(gmail_connector(&token_server, &api_server));
    let (grant, token) = mint_grant_with_selection_token(
        &state,
        &["email.read_thread:selected_no_attachments"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );

    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({ "selection_token_id": token.id.to_string() })),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "allow");

    let resp = post_action(
        addr,
        &grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({ "selection_token_id": token.id.to_string() })),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "selection token has already been used");

    handle.abort();
}

#[tokio::test]
async fn email_read_selected_thread_rejects_foreign_grant() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    mount_gmail_token_endpoint(&token_server).await;
    mount_gmail_thread_endpoint(&api_server, "thread-1").await;

    let state = test_state_with_gmail(gmail_connector(&token_server, &api_server));
    let (grant, token) = mint_grant_with_selection_token(
        &state,
        &["email.read_thread:selected_no_attachments"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );

    let mut foreign_grant = grant.clone();
    foreign_grant.id = Ulid::new();
    foreign_grant.task_token = Ulid::new().to_string();
    foreign_grant.selection_tokens = vec![];
    let pending_ref = state.artifacts.put(b"foreign pending".as_slice()).unwrap();
    state
        .store
        .insert_task_grant(&foreign_grant, &pending_ref, OWNER_CHAT_ID)
        .unwrap();

    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &foreign_grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({ "selection_token_id": token.id.to_string() })),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(
        body["error"],
        "selection_token_id is not bound to this task grant"
    );

    handle.abort();
}

#[tokio::test]
async fn email_read_selected_thread_rejects_expired_token() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    mount_gmail_token_endpoint(&token_server).await;
    mount_gmail_thread_endpoint(&api_server, "thread-1").await;

    let state = test_state_with_gmail(gmail_connector(&token_server, &api_server));
    let (grant, token) = mint_grant_with_selection_token(
        &state,
        &["email.read_thread:selected_no_attachments"],
        Timestamp::now() - std::time::Duration::from_secs(1),
    );

    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({ "selection_token_id": token.id.to_string() })),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "selection token has expired");

    handle.abort();
}

#[tokio::test]
async fn email_read_selected_thread_rejects_malformed_payload() {
    let state = test_state_with_gmail(GmailConnector::new(
        "client-id".to_string(),
        "client-secret".to_string(),
        "refresh-token".to_string(),
    ));
    let (grant, _token) = mint_grant_with_selection_token(
        &state,
        &["email.read_thread:selected_no_attachments"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );

    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({})),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(
        body["error"],
        "email.read_thread:selected_no_attachments payload must be exactly {\"selection_token_id\": string}"
    );

    let resp = post_action(
        addr,
        &grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({ "selection_token_id": "not-a-ulid" })),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "selection_token_id is not a valid id");

    // D-036: the shell has no way to name a thread directly — only a
    // selection token it was handed. A `thread_id` field is unknown to
    // `ReadThreadPayload` (`deny_unknown_fields`), so this is rejected the
    // same way as any other malformed payload, never silently ignored.
    let resp = post_action(
        addr,
        &grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({ "thread_id": "thread-1" })),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(
        body["error"],
        "email.read_thread:selected_no_attachments payload must be exactly {\"selection_token_id\": string}"
    );

    handle.abort();
}

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
    let text = request_body["text"].as_str().unwrap();
    assert!(text.ends_with("… [truncated]"));

    let prefix = text.strip_suffix("… [truncated]").unwrap();
    assert_eq!(
        prefix.encode_utf16().count(),
        4000,
        "truncation must keep exactly 4000 UTF-16 units before the marker"
    );
    assert!(text.encode_utf16().count() < long_body.encode_utf16().count());

    handle.abort();
}

use super::*;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::json;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn connector(token_server: &MockServer, api_server: &MockServer) -> GmailConnector {
    GmailConnector::new(
        "client-id".to_string(),
        "client-secret".to_string(),
        "refresh-token".to_string(),
        "owner@example.com".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), api_server.uri())
}

async fn mount_token_endpoint(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "test-access-token",
            "expires_in": 3600,
        })))
        .mount(server)
        .await;
}

fn sample_thread_json() -> Value {
    json!({
        "messages": [{
            "payload": {
                "mimeType": "multipart/mixed",
                "headers": [],
                "parts": [
                    {
                        "mimeType": "text/plain",
                        "headers": [
                            {"name": "From", "value": "alice@example.com"},
                            {"name": "Subject", "value": "Re: invoice"},
                        ],
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

#[tokio::test]
async fn fetch_thread_extracts_text_and_skips_attachments() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    mount_token_endpoint(&token_server).await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_thread_json()))
        .mount(&api_server)
        .await;

    let connector = connector(&token_server, &api_server);
    let thread = connector.fetch_thread("thread-1").await.unwrap();

    assert_eq!(thread.thread_id, "thread-1");
    assert_eq!(thread.messages.len(), 1);
    assert_eq!(thread.messages[0].body_text, "hello owner");
    assert!(!thread.messages[0].body_text.contains("not-a-real-pdf"));
}

#[tokio::test]
async fn thread_exists_is_true_for_a_real_thread() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    mount_token_endpoint(&token_server).await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_thread_json()))
        .mount(&api_server)
        .await;

    let connector = connector(&token_server, &api_server);
    assert!(connector.fetch_thread("thread-1").await.is_ok());
}

#[tokio::test]
async fn thread_exists_is_false_for_a_missing_thread() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    mount_token_endpoint(&token_server).await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/missing"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": "not found"})))
        .mount(&api_server)
        .await;

    let connector = connector(&token_server, &api_server);
    assert!(matches!(
        connector.fetch_thread("missing").await,
        Err(GmailError {
            class: GmailFailureClass::ThreadNotFound,
            ..
        })
    ));
}

#[tokio::test]
async fn a_non_404_api_error_is_not_treated_as_missing() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    mount_token_endpoint(&token_server).await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&api_server)
        .await;

    let connector = connector(&token_server, &api_server);
    let err = connector.fetch_thread("thread-1").await.unwrap_err();
    assert!(matches!(
        err,
        GmailError {
            status: Some(500),
            class: GmailFailureClass::Api
        }
    ));
}

#[tokio::test]
async fn a_failed_token_refresh_surfaces_as_an_error() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(401).set_body_string("invalid_grant"))
        .mount(&token_server)
        .await;

    let connector = connector(&token_server, &api_server);
    let err = connector.fetch_thread("thread-1").await.unwrap_err();
    assert!(matches!(
        err,
        GmailError {
            status: Some(401),
            class: GmailFailureClass::TokenRefresh
        }
    ));
}

#[tokio::test]
async fn the_access_token_is_cached_across_calls() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    // Only expect exactly one token POST despite two thread fetches.
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "test-access-token",
            "expires_in": 3600,
        })))
        .expect(1)
        .mount(&token_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_thread_json()))
        .mount(&api_server)
        .await;

    let connector = connector(&token_server, &api_server);
    connector.fetch_thread("thread-1").await.unwrap();
    connector.fetch_thread("thread-1").await.unwrap();
}

#[tokio::test]
async fn rotated_vault_credentials_bypass_live_access_token_cache() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("refresh_token=refresh-b"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "rotated-access-token",
            "expires_in": 3600,
        })))
        .expect(1)
        .mount(&token_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "first-access-token",
            "expires_in": 3600,
        })))
        .expect(1)
        .mount(&token_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_thread_json()))
        .mount(&api_server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let store = std::sync::Arc::new(
        crate::secret_store::SecretStore::open(dir.path().join("credentials"), [9; 32])
            .expect("open secret store"),
    );
    store
        .put("gmail.client_secret", b"client-a")
        .expect("seed client");
    store
        .put("gmail.refresh_token", b"refresh-a")
        .expect("seed refresh");
    let connector = GmailConnector::new_with_store(
        "client-id".to_string(),
        store.clone(),
        "gmail.client_secret".to_string(),
        "gmail.refresh_token".to_string(),
        "owner@example.com".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), api_server.uri());
    connector
        .fetch_thread("thread-1")
        .await
        .expect("first call");
    store
        .put("gmail.refresh_token", b"refresh-b")
        .expect("rotate refresh");
    connector
        .fetch_thread("thread-1")
        .await
        .expect("rotated call");
}

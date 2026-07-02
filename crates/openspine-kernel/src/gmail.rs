//! Gmail connector (build plan Step 5 / D-029, D-036, D-037): reads a
//! single, owner-selected thread, bounded and with attachments stripped
//! (PRD §21.1 steps 10-14). This is the first external-communication
//! connector — the shell never talks to Google directly, only the kernel
//! does, and only after `gate()` has already authorized
//! `email.read_thread:selected_no_attachments` for the calling grant.
//!
//! OAuth (D-037): the kernel holds a long-lived refresh token (env var,
//! same documented secret-intake shortcut as the bot token/artifact key)
//! and exchanges it for short-lived access tokens itself via a plain HTTP
//! POST to Google's token endpoint — the shell never sees a Google
//! credential of any kind, and no interactive OAuth flow runs inside this
//! process (a human completes Google's consent screen once, out of band;
//! see `docs/telegram-setup.md`).

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use jiff::Timestamp;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::Value;

const DEFAULT_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const DEFAULT_API_BASE_URL: &str = "https://gmail.googleapis.com";
const GMAIL_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// PRD §15's selection-scope `max_messages: 20` default — this connector
/// enforces its own hard ceiling independent of any caller-supplied scope,
/// so a forged/malformed scope can never widen what one fetch returns.
const MAX_MESSAGES: usize = 20;
/// Refresh the cached access token this many seconds before Google's own
/// `expires_in` would lapse, so a request never races an about-to-expire
/// token across the wire.
const TOKEN_REFRESH_SKEW_SECONDS: i64 = 60;

#[derive(Debug, thiserror::Error)]
pub enum GmailError {
    #[error("gmail HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("gmail token refresh failed: HTTP {status}: {body}")]
    TokenRefresh { status: u16, body: String },
    #[error("gmail API returned HTTP {status}: {body}")]
    Api { status: u16, body: String },
    #[error("gmail thread {0} not found")]
    ThreadNotFound(String),
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
}

/// One message inside a bounded thread. Never carries attachment payloads
/// (PRD §21.1 step 12: "reads bounded selected thread content without
/// attachments") — [`parse_thread`] skips any MIME part with a filename
/// entirely rather than merely omitting its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GmailMessage {
    pub from: String,
    pub subject: String,
    pub body_text: String,
}

/// A bounded, attachment-free thread (build plan Step 5 / PRD §21.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GmailThread {
    pub thread_id: String,
    pub messages: Vec<GmailMessage>,
}

struct CachedToken {
    access_token: String,
    expires_at: Timestamp,
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(GMAIL_REQUEST_TIMEOUT)
        .build()
        .expect("reqwest client with a fixed timeout always builds")
}

/// The live Gmail connector. `token_url`/`api_base_url` default to Google's
/// real endpoints but are overridable so tests point them at a `wiremock`
/// server — mirrors `model_gateway::providers::ProviderClient`'s pattern
/// (plain HTTP client, no vendor SDK, configurable base URL for testing).
pub struct GmailConnector {
    http: reqwest::Client,
    client_id: String,
    client_secret: String,
    refresh_token: String,
    token_url: String,
    api_base_url: String,
    cached: Mutex<Option<CachedToken>>,
}

impl GmailConnector {
    pub fn new(client_id: String, client_secret: String, refresh_token: String) -> Self {
        Self {
            http: http_client(),
            client_id,
            client_secret,
            refresh_token,
            token_url: DEFAULT_TOKEN_URL.to_string(),
            api_base_url: DEFAULT_API_BASE_URL.to_string(),
            cached: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_urls(mut self, token_url: String, api_base_url: String) -> Self {
        self.token_url = token_url;
        self.api_base_url = api_base_url;
        self
    }

    async fn access_token(&self) -> Result<String, GmailError> {
        if let Some(cached) = self.cached.lock().as_ref() {
            if cached.expires_at > Timestamp::now() {
                return Ok(cached.access_token.clone());
            }
        }

        let resp = self
            .http
            .post(&self.token_url)
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("refresh_token", self.refresh_token.as_str()),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(GmailError::TokenRefresh {
                status: status.as_u16(),
                body,
            });
        }
        let parsed: TokenResponse = resp.json().await?;
        let ttl = (parsed.expires_in - TOKEN_REFRESH_SKEW_SECONDS).max(0) as u64;
        let expires_at = Timestamp::now() + Duration::from_secs(ttl);
        *self.cached.lock() = Some(CachedToken {
            access_token: parsed.access_token.clone(),
            expires_at,
        });
        Ok(parsed.access_token)
    }

    /// Fetch one thread, bounded to [`MAX_MESSAGES`] messages with
    /// attachments stripped (PRD §21.1 step 12). `thread_id` must already
    /// be the kernel's own validated selection — this function does not
    /// itself decide whether the caller was authorized to ask; `gate()`
    /// already did, before this is ever called.
    pub async fn fetch_thread(&self, thread_id: &str) -> Result<GmailThread, GmailError> {
        let token = self.access_token().await?;
        let url = format!(
            "{}/gmail/v1/users/me/threads/{thread_id}?format=full",
            self.api_base_url
        );
        let resp = self.http.get(&url).bearer_auth(token).send().await?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(GmailError::ThreadNotFound(thread_id.to_string()));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(GmailError::Api {
                status: status.as_u16(),
                body,
            });
        }
        let json: Value = resp.json().await?;
        Ok(parse_thread(thread_id, &json))
    }

    /// Whether `thread_id` exists and is readable. Step 5's selection-mint
    /// path (`pipeline::handle_thread_selection`) validates this before
    /// ever minting a [`openspine_schemas::selection::SelectionToken`] — the
    /// kernel must never mint a selection token for a thread that turns out
    /// not to exist. Uses `format=minimal` rather than [`Self::fetch_thread`]'s
    /// `format=full` — this call only needs a 404-vs-200 answer, not the
    /// thread's messages, so there is no reason to pay for (and parse) the
    /// full payload.
    pub async fn thread_exists(&self, thread_id: &str) -> Result<bool, GmailError> {
        let token = self.access_token().await?;
        let url = format!(
            "{}/gmail/v1/users/me/threads/{thread_id}?format=minimal",
            self.api_base_url
        );
        let resp = self.http.get(&url).bearer_auth(token).send().await?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(GmailError::Api {
                status: status.as_u16(),
                body,
            });
        }
        Ok(true)
    }
}

fn header_value(headers: &[Value], name: &str) -> String {
    headers
        .iter()
        .find(|h| {
            h["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
        .and_then(|h| h["value"].as_str())
        .unwrap_or_default()
        .to_string()
}

/// Depth-first search for the first non-attachment `text/plain` part.
/// Recurses through `parts` (multipart MIME) but skips any part carrying a
/// non-empty `filename` entirely — an attachment's bytes are never even
/// base64-decoded, let alone returned (PRD §21.1: "without attachments").
fn extract_body_text(payload: &Value) -> String {
    let filename = payload["filename"].as_str().unwrap_or("");
    if !filename.is_empty() {
        return String::new();
    }
    let mime_type = payload["mimeType"].as_str().unwrap_or("");
    if mime_type == "text/plain" {
        if let Some(data) = payload["body"]["data"].as_str() {
            if let Ok(bytes) = URL_SAFE_NO_PAD.decode(data) {
                return String::from_utf8_lossy(&bytes).into_owned();
            }
        }
    }
    if let Some(parts) = payload["parts"].as_array() {
        for part in parts {
            let text = extract_body_text(part);
            if !text.is_empty() {
                return text;
            }
        }
    }
    String::new()
}

fn parse_thread(thread_id: &str, json: &Value) -> GmailThread {
    let messages = json["messages"].as_array().cloned().unwrap_or_default();
    let mut out = Vec::new();
    for msg in messages.into_iter().take(MAX_MESSAGES) {
        let headers = msg["payload"]["headers"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        out.push(GmailMessage {
            from: header_value(&headers, "From"),
            subject: header_value(&headers, "Subject"),
            body_text: extract_body_text(&msg["payload"]),
        });
    }
    GmailThread {
        thread_id: thread_id.to_string(),
        messages: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn connector(token_server: &MockServer, api_server: &MockServer) -> GmailConnector {
        GmailConnector::new(
            "client-id".to_string(),
            "client-secret".to_string(),
            "refresh-token".to_string(),
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
        assert!(connector.thread_exists("thread-1").await.unwrap());
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
        assert!(!connector.thread_exists("missing").await.unwrap());
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
        assert!(matches!(err, GmailError::Api { status: 500, .. }));
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
        assert!(matches!(err, GmailError::TokenRefresh { status: 401, .. }));
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
}

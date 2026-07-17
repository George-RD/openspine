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

mod credentials;

use std::time::Duration;

use jiff::Timestamp;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::Value;

mod helpers;
use helpers::{build_raw_reply_message, header_value, parse_thread};

const DEFAULT_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const DEFAULT_API_BASE_URL: &str = "https://gmail.googleapis.com";
const GMAIL_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// PRD §15's selection-scope `max_messages: 20` default — this connector
/// enforces its own hard ceiling independent of any caller-supplied scope,
/// so a forged/malformed scope can never widen what one fetch returns.
const MAX_MESSAGES: usize = 20;
/// Hard response bound for metadata-only preflight reads. A provider response
/// exceeding this limit fails closed before deserialization.
const MAX_METADATA_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadRecipient {
    Address(String),
    ThreadNotFound,
    Unavailable,
}
/// Refresh the cached access token this many seconds before Google's own
/// `expires_in` would lapse, so a request never races an about-to-expire
/// token across the wire.
const TOKEN_REFRESH_SKEW_SECONDS: i64 = 60;

/// Fixed failure class for Gmail connector errors. This is the stable label
/// that is audited/persisted; the provider's response body is NEVER carried
/// in the error (D-012: no sensitive/provider text persisted or displayed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GmailFailureClass {
    #[error("transport")]
    Transport,
    #[error("token_refresh")]
    TokenRefresh,
    #[error("api")]
    Api,
    #[error("malformed_response")]
    MalformedResponse,
    #[error("thread_not_found")]
    ThreadNotFound,
}

/// A Gmail connector failure. Carries only an optional HTTP status and a
/// fixed [`GmailFailureClass`] — never the provider response body, so it is
/// safe to persist/display via `Display` (D-012).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("gmail {class} failure (status: {status:?})")]
pub struct GmailError {
    pub status: Option<u16>,
    pub class: GmailFailureClass,
}

impl From<reqwest::Error> for GmailError {
    fn from(_: reqwest::Error) -> Self {
        GmailError {
            status: None,
            class: GmailFailureClass::Transport,
        }
    }
}

async fn bounded_json_response(mut resp: reqwest::Response) -> Result<Value, GmailError> {
    if resp
        .content_length()
        .is_some_and(|length| length > MAX_METADATA_RESPONSE_BYTES as u64)
    {
        return Err(GmailError {
            status: None,
            class: GmailFailureClass::MalformedResponse,
        });
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if body.len().saturating_add(chunk.len()) > MAX_METADATA_RESPONSE_BYTES {
            return Err(GmailError {
                status: None,
                class: GmailFailureClass::MalformedResponse,
            });
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| GmailError {
        status: None,
        class: GmailFailureClass::MalformedResponse,
    })
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
    /// The `Message-ID` header, used (D-042) to set `In-Reply-To`/
    /// `References` on a draft reply for correct Gmail threading. Empty
    /// if the header was absent — [`GmailConnector::create_draft`] omits
    /// the reply headers entirely in that case rather than fabricating one.
    pub message_id: String,
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
    client_secret_version: Option<openspine_schemas::digest::Digest>,
    refresh_token_version: Option<openspine_schemas::digest::Digest>,
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(GMAIL_REQUEST_TIMEOUT)
        .build()
        .expect("reqwest client with a fixed timeout always builds")
}

/// The live Gmail connector. `token_url`/`api_base_url` default to Google's
/// real endpoints but are overridable so tests point them at a `wiremock`
pub struct GmailConnector {
    http: reqwest::Client,
    client_id: String,
    client_secret: String,
    refresh_token: String,
    secrets: Option<std::sync::Arc<crate::secret_store::SecretStore>>,
    client_secret_slot: String,
    refresh_token_slot: String,
    token_url: String,
    api_base_url: String,
    cached: Mutex<Option<CachedToken>>,
    /// The owner's own Gmail address (D-042) — see [`newest_non_owner_recipient`].
    mailbox_address: String,
}

impl GmailConnector {
    pub fn new(
        client_id: String,
        client_secret: String,
        refresh_token: String,
        mailbox_address: String,
    ) -> Self {
        Self {
            http: http_client(),
            client_id,
            client_secret,
            refresh_token,
            secrets: None,
            client_secret_slot: String::new(),
            refresh_token_slot: String::new(),
            token_url: DEFAULT_TOKEN_URL.to_string(),
            api_base_url: DEFAULT_API_BASE_URL.to_string(),
            cached: Mutex::new(None),
            mailbox_address,
        }
    }
    pub fn new_with_store(
        client_id: String,
        secrets: std::sync::Arc<crate::secret_store::SecretStore>,
        client_secret_slot: String,
        refresh_token_slot: String,
        mailbox_address: String,
    ) -> Self {
        let mut connector = Self::new(client_id, String::new(), String::new(), mailbox_address);
        connector.secrets = Some(secrets);
        connector.client_secret_slot = client_secret_slot;
        connector.refresh_token_slot = refresh_token_slot;
        connector
    }

    /// The owner's own address (D-042), used by [`newest_non_owner_recipient`].
    pub fn mailbox_address(&self) -> &str {
        &self.mailbox_address
    }

    #[cfg(test)]
    pub(crate) fn with_urls(mut self, token_url: String, api_base_url: String) -> Self {
        self.token_url = token_url;
        self.api_base_url = api_base_url;
        self
    }

    async fn access_token(&self) -> Result<String, GmailError> {
        let (client_secret, refresh_token, client_secret_version, refresh_token_version) =
            if let Some(secrets) = &self.secrets {
                let (client_bytes, client_version) = secrets
                    .get_with_version(&self.client_secret_slot)
                    .map_err(|_| GmailError {
                        status: Some(0),
                        class: GmailFailureClass::TokenRefresh,
                    })?
                    .ok_or(GmailError {
                        status: Some(0),
                        class: GmailFailureClass::TokenRefresh,
                    })?;
                let (refresh_bytes, refresh_version) = secrets
                    .get_with_version(&self.refresh_token_slot)
                    .map_err(|_| GmailError {
                        status: Some(0),
                        class: GmailFailureClass::TokenRefresh,
                    })?
                    .ok_or(GmailError {
                        status: Some(0),
                        class: GmailFailureClass::TokenRefresh,
                    })?;
                let client_secret = String::from_utf8(client_bytes).map_err(|_| GmailError {
                    status: Some(0),
                    class: GmailFailureClass::TokenRefresh,
                })?;
                let refresh_token = String::from_utf8(refresh_bytes).map_err(|_| GmailError {
                    status: Some(0),
                    class: GmailFailureClass::TokenRefresh,
                })?;
                (
                    client_secret,
                    refresh_token,
                    Some(client_version),
                    Some(refresh_version),
                )
            } else {
                (
                    self.client_secret.clone(),
                    self.refresh_token.clone(),
                    None,
                    None,
                )
            };
        if let Some(cached) = self.cached.lock().as_ref() {
            if cached.expires_at > Timestamp::now()
                && cached.client_secret_version == client_secret_version
                && cached.refresh_token_version == refresh_token_version
            {
                return Ok(cached.access_token.clone());
            }
        }
        let resp = self
            .http
            .post(&self.token_url)
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", client_secret.as_str()),
                ("refresh_token", refresh_token.as_str()),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(GmailError {
                status: Some(status.as_u16()),
                class: GmailFailureClass::TokenRefresh,
            });
        }
        let parsed: TokenResponse = resp.json().await?;
        let ttl = (parsed.expires_in - TOKEN_REFRESH_SKEW_SECONDS).max(0) as u64;
        let expires_at = Timestamp::now() + Duration::from_secs(ttl);
        *self.cached.lock() = Some(CachedToken {
            access_token: parsed.access_token.clone(),
            expires_at,
            client_secret_version,
            refresh_token_version,
        });
        Ok(parsed.access_token)
    }

    /// Fetch one thread, bounded to [`MAX_MESSAGES`] messages with
    /// attachments stripped (PRD §21.1 step 12). `thread_id` must already
    /// be the kernel's own validated selection — this function does not
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
            return Err(GmailError {
                status: Some(404),
                class: GmailFailureClass::ThreadNotFound,
            });
        }
        if !status.is_success() {
            return Err(GmailError {
                status: Some(status.as_u16()),
                class: GmailFailureClass::Api,
            });
        }
        let json: Value = resp.json().await?;
        Ok(parse_thread(thread_id, &json))
    }

    /// Narrow, pre-gate recipient snapshot: one bounded minimal thread
    /// response followed by at most [`MAX_MESSAGES`] metadata-only reads.
    pub async fn fetch_thread_recipient(
        &self,
        thread_id: &str,
    ) -> Result<ThreadRecipient, GmailError> {
        let token = self.access_token().await?;
        let thread_url = format!(
            "{}/gmail/v1/users/me/threads/{thread_id}?format=minimal",
            self.api_base_url
        );
        let resp = self
            .http
            .get(&thread_url)
            .bearer_auth(token.clone())
            .send()
            .await?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(ThreadRecipient::ThreadNotFound);
        }
        if !status.is_success() {
            return Err(GmailError {
                status: Some(status.as_u16()),
                class: GmailFailureClass::Api,
            });
        }
        let json = bounded_json_response(resp).await?;
        let message_ids: Vec<String> = json
            .get("messages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .rev()
            .take(MAX_MESSAGES)
            .filter_map(|msg| msg.get("id").and_then(Value::as_str).map(str::to_owned))
            .collect();
        let owner = self.mailbox_address.trim().to_lowercase();
        for msg_id in message_ids {
            let msg_url = format!(
                "{}/gmail/v1/users/me/messages/{msg_id}?format=metadata&metadataHeaders=From",
                self.api_base_url
            );
            let msg_resp = self
                .http
                .get(&msg_url)
                .bearer_auth(token.clone())
                .send()
                .await?;
            let msg_status = msg_resp.status();
            if !msg_status.is_success() {
                return Err(GmailError {
                    status: Some(msg_status.as_u16()),
                    class: GmailFailureClass::Api,
                });
            }
            let msg_json = bounded_json_response(msg_resp).await?;
            let headers = msg_json
                .get("payload")
                .and_then(|p| p.get("headers"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if let Some(addr) = extract_email_address(&header_value(&headers, "From")) {
                if addr != owner {
                    return Ok(ThreadRecipient::Address(addr));
                }
            }
        }
        Ok(ThreadRecipient::Unavailable)
    }

    /// Create a Gmail draft replying to `thread_id` (D-044 / PRD's
    /// `email.create_draft`). This method performs no authorization of its
    /// own — it is the connector's raw effect, only ever reached after
    /// `gate()` has confirmed a matching, unexpired, digest-bound approval
    /// (D-011, D-041). Returns the provider's own draft id.
    pub async fn create_draft(
        &self,
        thread_id: &str,
        target: &ReplyTarget,
        subject: &str,
        body: &str,
    ) -> Result<String, GmailError> {
        let token = self.access_token().await?;
        let raw = build_raw_reply_message(
            &target.recipient,
            subject,
            body,
            target.in_reply_to_message_id.as_deref(),
        );
        let url = format!("{}/gmail/v1/users/me/drafts", self.api_base_url);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(token)
            .json(&serde_json::json!({
                "message": { "raw": raw, "threadId": thread_id }
            }))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(GmailError {
                status: Some(status.as_u16()),
                class: GmailFailureClass::Api,
            });
        }
        let json: Value = resp.json().await?;
        json["id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| GmailError {
                status: Some(status.as_u16()),
                class: GmailFailureClass::MalformedResponse,
            })
    }
}

/// Who a drafted reply should go to, and (if known) which message it is
/// replying to — the latter sets `In-Reply-To`/`References` for correct
/// Gmail threading (D-042).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyTarget {
    pub recipient: String,
    pub in_reply_to_message_id: Option<String>,
}

/// D-042: find who a reply should actually go to by walking the thread
/// newest-first and skipping the owner's own messages — a naive "last
/// message's sender" rule breaks when the owner sent the most recent
/// message (a self-addressed follow-up). Returns `None` if every message
/// is from the owner, or no message has a parseable `From` address.
pub fn newest_non_owner_recipient(
    thread: &GmailThread,
    owner_mailbox: &str,
) -> Option<ReplyTarget> {
    let owner = owner_mailbox.trim().to_lowercase();
    thread.messages.iter().rev().find_map(|m| {
        let addr = extract_email_address(&m.from)?;
        (addr != owner).then(|| ReplyTarget {
            recipient: addr,
            in_reply_to_message_id: (!m.message_id.is_empty()).then(|| m.message_id.clone()),
        })
    })
}

/// Extract the bare address from a `From`-style header value — either
/// `"Name <addr@example.com>"` or a bare `"addr@example.com"`. `None` if
/// nothing `@`-shaped is found at all (not a parseable address).
fn extract_email_address(header: &str) -> Option<String> {
    let candidate = header
        .rsplit_once('<')
        .and_then(|(_, rest)| rest.split_once('>'))
        .map(|(addr, _)| addr)
        .unwrap_or(header)
        .trim();
    candidate.contains('@').then(|| candidate.to_lowercase())
}

#[cfg(test)]
mod tests;

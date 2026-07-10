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
            token_url: DEFAULT_TOKEN_URL.to_string(),
            api_base_url: DEFAULT_API_BASE_URL.to_string(),
            cached: Mutex::new(None),
            mailbox_address,
        }
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

    /// Whether `thread_id` exists and is readable. The email-preview lane's
    /// preflight (`pipeline::lanes::email_preflight`) validates this before
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
            let body = resp.text().await.unwrap_or_default();
            return Err(GmailError::Api {
                status: status.as_u16(),
                body,
            });
        }
        let json: Value = resp.json().await?;
        json["id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| GmailError::Api {
                status: status.as_u16(),
                body: "draft create response missing \"id\"".to_string(),
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

/// Build a base64url-encoded RFC 2822 message for Gmail's `raw` draft
/// field. Minimal by design (PRD/Step 6 scope: single plain-text reply,
/// no attachments, no Cc/Bcc) — sets `In-Reply-To`/`References` only when
/// the original message's `Message-ID` was captured; omits them rather
/// than fabricating a value when it wasn't (better a plainly-unthreaded
/// draft than one carrying a made-up header).
///
/// `subject`/`body` are written byte-for-byte, never reformatted here
/// (e.g. no "Re: " prefixing — the shell already composes the final
/// subject before it is ever previewed): D-041's approval binds a digest
/// over the exact reviewed payload, so what gets written to Gmail must
/// match what the owner approved exactly, not a value this function
/// silently touches up after the fact.
///
/// Headers are written as raw ASCII with no RFC 2047 encoding — a
/// non-ASCII subject/recipient will produce a technically invalid header.
/// Out of scope for this phase (English-language usage assumed); a future
/// phase adding non-ASCII support must encode `Subject` per RFC 2047.
fn build_raw_reply_message(
    to: &str,
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
) -> String {
    let mut headers =
        format!("To: {to}\r\nSubject: {subject}\r\nContent-Type: text/plain; charset=UTF-8\r\n");
    if let Some(id) = in_reply_to.filter(|s| !s.is_empty()) {
        headers.push_str(&format!("In-Reply-To: {id}\r\nReferences: {id}\r\n"));
    }
    URL_SAFE_NO_PAD.encode(format!("{headers}\r\n{body}").as_bytes())
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
            message_id: header_value(&headers, "Message-ID"),
        });
    }
    GmailThread {
        thread_id: thread_id.to_string(),
        messages: out,
    }
}

#[cfg(test)]
mod tests;

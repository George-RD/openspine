use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde_json::Value;

use super::{GmailMessage, GmailThread, MAX_MESSAGES};

pub(super) fn build_raw_reply_message(
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

pub(super) fn parse_thread(thread_id: &str, json: &Value) -> GmailThread {
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

pub(super) fn header_value(headers: &[Value], name: &str) -> String {
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

//! ChatGPT backend Responses transport for OpenAI Codex OAuth grants.
//!
//! A Codex subscription token is only accepted by
//! `chatgpt.com/backend-api/codex/responses`, addressed with the
//! `chatgpt-account-id` header derived from the token itself at login time.
//! The endpoint mandates `store: false` and `stream: true`, so the kernel
//! consumes the SSE stream and returns the assembled text: the gateway's
//! request/response contract is unchanged, streaming never leaves this
//! module.
//!
//! Wire contract verified against pi's working implementation and the
//! first-party Codex client. `max_tokens` has no equivalent on this
//! endpoint; the spend cap and grant budgets remain the cost bound.

use serde_json::{json, Value};

use super::providers::GatewayError;
use super::{PromptRole, ResolvedPrompt};
use crate::codex_fingerprint;

const PROVIDER: &str = "openai-codex";

/// The exact request body the Responses endpoint accepts. User turns carry
/// `input_text` content items, assistant turns `output_text`; the template's
/// system text travels as `instructions`, byte for byte.
fn request_body(model: &str, prompt: &ResolvedPrompt) -> Value {
    let input: Vec<Value> = prompt
        .messages
        .iter()
        .map(|message| {
            let (role, content_type) = match message.role {
                PromptRole::User => ("user", "input_text"),
                PromptRole::Assistant => ("assistant", "output_text"),
            };
            json!({
                "type": "message",
                "role": role,
                "content": [{ "type": content_type, "text": message.content }],
            })
        })
        .collect();
    json!({
        "model": model,
        "store": false,
        "stream": true,
        "instructions": prompt.system,
        "input": input,
        "text": { "verbosity": "low" },
        "include": ["reasoning.encrypted_content"],
        "tool_choice": "auto",
        "parallel_tool_calls": true,
    })
}

/// Assemble the reply text from a complete SSE body.
///
/// Line-based SSE framing: `data:` lines accumulate into the current event
/// and a blank line ends it. `str::lines` strips the `\r` of CRLF
/// terminators, so LF and CRLF streams frame identically. Text accumulates
/// from `response.output_text.delta` events; every terminal event
/// (`response.completed`/`done`/`incomplete`) supplies the output items as
/// a fallback when no deltas arrived. `response.failed` and `error` events
/// fail the call with the upstream message; an `incomplete` terminal with
/// no usable text fails with its reported reason. A
/// stream that ends without a terminal event is truncated and fails closed
/// rather than returning a partial reply.
fn text_from_sse(raw: &str) -> Result<String, GatewayError> {
    let mut deltas = String::new();
    let mut event_data: Vec<&str> = Vec::new();
    // The trailing chained blank line flushes a final event even when the
    // stream was cut off after its data line.
    for line in raw.lines().chain(std::iter::once("")) {
        if let Some(data) = line.strip_prefix("data:") {
            event_data.push(data.trim());
            continue;
        }
        if !line.is_empty() || event_data.is_empty() {
            continue;
        }
        let payload = event_data.join("\n");
        event_data.clear();
        // A bare `data:` line or heartbeat carries nothing to parse.
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }
        let event: Value = serde_json::from_str(&payload)
            .map_err(|_| GatewayError::MissingContent(PROVIDER.to_string()))?;
        match event.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => {
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    deltas.push_str(delta);
                }
            }
            // An in-band failure event is an upstream failure on a 200
            // stream; 502 mirrors the Onyx in-band `error_msg` precedent.
            Some("response.failed" | "error") => {
                return Err(GatewayError::ProviderError {
                    provider: PROVIDER.to_string(),
                    status: 502,
                    body: upstream_error_message(&event),
                });
            }
            // Every terminal participates in text extraction, `incomplete`
            // included: a token-cap cut still carries genuine, owner-visible
            // output (WYSIWYS binds what the owner sees). An incomplete
            // terminal with NOTHING usable fails with the upstream reason —
            // typically a content filter — instead of a generic error.
            Some("response.completed" | "response.done" | "response.incomplete") => {
                // The nested status outranks the event name: a terminal
                // whose response says failed/cancelled is a failure whatever
                // frame it arrived in, and its fragment is not an answer.
                let status = event
                    .get("response")
                    .and_then(|r| r.get("status"))
                    .and_then(Value::as_str);
                if matches!(status, Some("failed" | "cancelled")) {
                    return Err(GatewayError::ProviderError {
                        provider: PROVIDER.to_string(),
                        status: 502,
                        body: format!(
                            "terminal response status {}: {}",
                            status.unwrap_or_default(),
                            upstream_error_message(&event)
                        ),
                    });
                }
                let text = if deltas.is_empty() {
                    completed_output_text(&event)
                } else {
                    std::mem::take(&mut deltas)
                };
                if !text.is_empty() {
                    return Ok(text);
                }
                let reason = event
                    .get("response")
                    .and_then(|r| r.get("incomplete_details"))
                    .and_then(|d| d.get("reason"))
                    .and_then(Value::as_str);
                return Err(match reason {
                    Some(reason) => GatewayError::ProviderError {
                        provider: PROVIDER.to_string(),
                        status: 502,
                        body: format!("response incomplete: {reason}"),
                    },
                    None => GatewayError::MissingContent(PROVIDER.to_string()),
                });
            }
            _ => {}
        }
    }
    // No terminal event: the stream was cut off. A partial reply must not
    // pass as the provider's answer.
    Err(GatewayError::MissingContent(PROVIDER.to_string()))
}

/// The failure message an `error` or `response.failed` event carries, from
/// either the flat or the nested shape.
fn upstream_error_message(event: &Value) -> String {
    let nested = event.get("response").and_then(|r| r.get("error"));
    let error = event.get("error").or(nested);
    let message = error
        .and_then(|e| e.get("message"))
        .or_else(|| event.get("message"))
        .and_then(Value::as_str);
    let code = error
        .and_then(|e| e.get("code"))
        .or_else(|| event.get("code"))
        .and_then(Value::as_str);
    match (message, code) {
        (Some(message), _) => message.to_string(),
        (None, Some(code)) => code.to_string(),
        (None, None) => event.to_string(),
    }
}

/// Text of every `output_text` content item in a terminal event's output.
fn completed_output_text(event: &Value) -> String {
    let mut text = String::new();
    let items = event
        .get("response")
        .and_then(|r| r.get("output"))
        .and_then(Value::as_array);
    for item in items.into_iter().flatten() {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let contents = item.get("content").and_then(Value::as_array);
        for content in contents.into_iter().flatten() {
            if content.get("type").and_then(Value::as_str) == Some("output_text") {
                if let Some(piece) = content.get("text").and_then(Value::as_str) {
                    text.push_str(piece);
                }
            }
        }
    }
    text
}

pub(super) async fn generate_codex_responses(
    client: &reqwest::Client,
    access_token: &str,
    account_id: Option<&str>,
    base_url: &str,
    model: &str,
    prompt: &ResolvedPrompt,
) -> Result<String, GatewayError> {
    // Refuse before the wire: without the account header the backend rejects
    // the request opaquely, and the stored credential is the actual problem.
    let account_id =
        account_id.ok_or_else(|| GatewayError::MissingAccountId(PROVIDER.to_string()))?;

    // The exact secret strings this call transmitted never leave in an
    // error: a backend body or failure event echoing the bearer or account
    // id would otherwise flow verbatim into kernel logs.
    let scrub = |mut message: String| -> String {
        for secret in [access_token, account_id] {
            if !secret.is_empty() {
                message = message.replace(secret, "<redacted>");
            }
        }
        message
    };

    let response = client
        .post(format!("{base_url}{}", codex_fingerprint::RESPONSES_PATH))
        .bearer_auth(access_token)
        .header("chatgpt-account-id", account_id)
        .header("OpenAI-Beta", codex_fingerprint::OPENAI_BETA)
        .header("originator", codex_fingerprint::ORIGINATOR)
        .header("User-Agent", codex_fingerprint::USER_AGENT)
        .header("Accept", codex_fingerprint::ACCEPT)
        .json(&request_body(model, prompt))
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        return Err(GatewayError::ProviderError {
            provider: PROVIDER.to_string(),
            status: status.as_u16(),
            body: scrub(text),
        });
    }
    text_from_sse(&text).map_err(|error| match error {
        GatewayError::ProviderError {
            provider,
            status,
            body,
        } => GatewayError::ProviderError {
            provider,
            status,
            body: scrub(body),
        },
        other => other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_gateway::PromptMessage;
    use openspine_schemas::workflow::ReasoningTier;

    fn prompt() -> ResolvedPrompt {
        ResolvedPrompt {
            system: "You are Lyra.".to_string(),
            messages: vec![
                PromptMessage {
                    role: PromptRole::User,
                    content: "hello".to_string(),
                },
                PromptMessage {
                    role: PromptRole::Assistant,
                    content: "hi".to_string(),
                },
            ],
            max_tokens: 100,
            reasoning_tier: ReasoningTier::Standard,
        }
    }

    #[test]
    fn the_request_body_carries_the_mandated_shape() {
        let body = request_body("gpt-5-codex", &prompt());
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["instructions"], "You are Lyra.");
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(body["input"][1]["role"], "assistant");
        assert_eq!(body["input"][1]["content"][0]["type"], "output_text");
    }

    #[test]
    fn deltas_accumulate_in_stream_order() {
        let sse = concat!(
            "data: {\"type\":\"response.created\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hel\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n",
            "data: [DONE]\n\n",
        );
        assert_eq!(text_from_sse(sse).expect("text"), "Hello");
    }

    /// CRLF-terminated SSE frames identically to LF: `str::lines` strips the
    /// carriage return, and event boundaries are blank lines either way.
    #[test]
    fn crlf_framed_streams_parse_identically() {
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hel\"}\r\n\r\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\r\n\r\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\r\n\r\n",
        );
        assert_eq!(text_from_sse(sse).expect("text"), "Hello");
    }

    /// Heartbeat comments and bare `data:` lines are framing noise, not
    /// events.
    #[test]
    fn heartbeats_and_empty_data_lines_are_ignored() {
        let sse = concat!(
            ": keep-alive\n\n",
            "data:\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n",
        );
        assert_eq!(text_from_sse(sse).expect("text"), "ok");
    }

    #[test]
    fn a_terminal_event_supplies_text_when_no_deltas_arrived() {
        let sse = "data: {\"type\":\"response.completed\",\"response\":{\"output\":[\
                   {\"type\":\"reasoning\",\"summary\":[]},\
                   {\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"final answer\"}]}\
                   ]}}\n\n";
        assert_eq!(text_from_sse(sse).expect("text"), "final answer");
    }

    #[test]
    fn a_failed_response_carries_the_upstream_message() {
        let sse = "data: {\"type\":\"response.failed\",\"response\":{\"error\":\
                   {\"code\":\"rate_limit\",\"message\":\"quota exhausted\"}}}\n\n";
        let error = text_from_sse(sse).expect_err("must fail");
        assert!(
            matches!(&error, GatewayError::ProviderError { status: 502, .. }),
            "an in-stream failure maps to an upstream provider error: {error}"
        );
        assert!(error.to_string().contains("quota exhausted"), "{error}");
    }

    #[test]
    fn a_truncated_stream_fails_closed_instead_of_returning_a_partial_reply() {
        let sse = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hel\"}\n\n";
        assert!(matches!(
            text_from_sse(sse),
            Err(GatewayError::MissingContent(_))
        ));
    }

    /// A token-cap cut still carries genuine, owner-visible output; the
    /// fragment is returned rather than discarded.
    #[test]
    fn an_incomplete_terminal_with_text_returns_the_fragment() {
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial an\"}\n\n",
            "data: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\",\
             \"incomplete_details\":{\"reason\":\"max_output_tokens\"}}}\n\n",
        );
        assert_eq!(text_from_sse(sse).expect("text"), "partial an");
    }

    /// An incomplete terminal with nothing usable names the upstream reason
    /// instead of a generic missing-content error.
    #[test]
    fn an_incomplete_terminal_without_text_fails_with_its_reason() {
        let sse =
            "data: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\",\
                   \"output\":[],\"incomplete_details\":{\"reason\":\"content_filter\"}}}\n\n";
        let error = text_from_sse(sse).expect_err("must fail");
        assert!(error.to_string().contains("content_filter"), "{error}");
    }

    /// The nested status outranks the event name: a `response.done` whose
    /// response reports `failed` is a failure, and its fragment is not an
    /// answer.
    #[test]
    fn a_terminal_event_with_failed_status_is_rejected_despite_deltas() {
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"poisoned\"}\n\n",
            "data: {\"type\":\"response.done\",\"response\":{\"status\":\"failed\",\"error\":\
             {\"message\":\"upstream exploded\"}}}\n\n",
        );
        let error = text_from_sse(sse).expect_err("must fail");
        assert!(error.to_string().contains("upstream exploded"), "{error}");
    }
}

//! Onyx chat transport: the normal non-streaming Onyx chat API with a
//! scoped Personal Access Token. Split from `providers.rs` for the 500-line
//! module gate.

use serde_json::{json, Value};

use super::providers::GatewayError;
use super::ResolvedPrompt;

fn onyx_request_parts(prompt: &ResolvedPrompt) -> Result<(String, String), GatewayError> {
    let user_index = prompt
        .messages
        .iter()
        .rposition(|message| matches!(message.role, super::PromptRole::User))
        .ok_or_else(|| GatewayError::MissingContent("onyx.request.user_message".to_string()))?;
    let message = prompt.messages[user_index].content.trim().to_string();
    if message.is_empty() {
        return Err(GatewayError::MissingContent(
            "onyx.request.user_message".to_string(),
        ));
    }

    let mut context = format!("OpenSpine system instructions:\n{}", prompt.system);
    if user_index > 0 {
        context.push_str("\n\nConversation history:");
        for item in &prompt.messages[..user_index] {
            let role = match item.role {
                super::PromptRole::User => "USER",
                super::PromptRole::Assistant => "ASSISTANT",
            };
            context.push_str(&format!("\n{role}: {}", item.content));
        }
    }
    Ok((message, context))
}

pub(super) async fn generate_onyx(
    client: &reqwest::Client,
    pat: &str,
    base_url: &str,
    model: &str,
    prompt: &ResolvedPrompt,
) -> Result<String, GatewayError> {
    let (message, additional_context) = onyx_request_parts(prompt)?;
    let body = json!({
        "message": message,
        "llm_override": { "model_version": model },
        "allowed_tool_ids": [],
        "origin": "api",
        "stream": false,
        "include_citations": false,
        "additional_context": additional_context,
    });
    let response = client
        .post(format!("{base_url}/chat/send-chat-message"))
        .bearer_auth(pat)
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        return Err(GatewayError::ProviderError {
            provider: "onyx".to_string(),
            status: status.as_u16(),
            body: text,
        });
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| GatewayError::MissingContent("onyx".to_string()))?;
    if let Some(error) = value
        .get("error_msg")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|error| !error.is_empty())
    {
        return Err(GatewayError::ProviderError {
            provider: "onyx".to_string(),
            status: 502,
            body: error.to_string(),
        });
    }
    value
        .get("answer_citationless")
        .or_else(|| value.get("answer"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|answer| !answer.is_empty())
        .map(str::to_string)
        .ok_or_else(|| GatewayError::MissingContent("onyx".to_string()))
}

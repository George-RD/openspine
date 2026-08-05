//! Provider HTTP clients (build plan 4c).
//!
//! Provider kinds are enum-dispatched (no `dyn`/`async_trait` — this repo's
//! no-new-deps convention and the small, closed set of kinds don't justify
//! either): `anthropic` calls the Messages API; `openai_compat` calls
//! `/v1/chat/completions`; `onyx` calls the normal non-streaming Onyx chat API
//! with a scoped Personal Access Token.

use serde_json::{json, Value};

use crate::config::{ProviderConfig, ProviderKind};

use super::ResolvedPrompt;

const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com";
const DEFAULT_ONYX_BASE_URL: &str = "http://127.0.0.1:8080";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("provider HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("provider {provider} returned HTTP {status}: {body}")]
    ProviderError {
        provider: String,
        status: u16,
        body: String,
    },
    #[error("provider {0} response did not contain the expected content field")]
    MissingContent(String),
}
/// One configured provider, ready to call. Built once from
/// [`ProviderConfig`] + the resolved API key (config.rs's `provider_api_key`)
/// at kernel startup. Cloning is cheap: `reqwest::Client` is internally
/// shared; the clone is used to snapshot a provider under an AppState read
/// lock before awaiting network I/O.
#[derive(Clone)]
pub enum ProviderClient {
    Anthropic {
        client: reqwest::Client,
        api_key: String,
        base_url: String,
        model: String,
    },
    OpenAiCompat {
        client: reqwest::Client,
        api_key: String,
        base_url: String,
        model: String,
    },
    Onyx {
        client: reqwest::Client,
        pat: String,
        base_url: String,
        model: String,
    },
}

/// A provider call is effectful and gate-mediated; it must never hang the
/// task indefinitely if a provider stalls — the sandbox's own
/// `max_runtime_seconds` is the outer bound, but a per-request timeout well
/// under that keeps one bad provider call from burning the whole task
/// budget silently.
const PROVIDER_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(PROVIDER_REQUEST_TIMEOUT)
        .build()
        .expect("reqwest client with a fixed timeout always builds")
}

impl ProviderClient {
    pub fn from_config(config: &ProviderConfig, api_key: String) -> Self {
        match config.kind {
            ProviderKind::Anthropic => ProviderClient::Anthropic {
                client: http_client(),
                api_key,
                base_url: config
                    .base_url
                    .clone()
                    .unwrap_or_else(|| DEFAULT_ANTHROPIC_BASE_URL.to_string()),
                model: config.model.clone(),
            },
            ProviderKind::OpenaiCompat => ProviderClient::OpenAiCompat {
                client: http_client(),
                api_key,
                base_url: config
                    .base_url
                    .clone()
                    .unwrap_or_else(|| DEFAULT_OPENAI_BASE_URL.to_string()),
                model: config.model.clone(),
            },
            ProviderKind::Onyx => ProviderClient::Onyx {
                client: http_client(),
                pat: api_key,
                base_url: config
                    .base_url
                    .clone()
                    .unwrap_or_else(|| DEFAULT_ONYX_BASE_URL.to_string()),
                model: config.model.clone(),
            },
            ProviderKind::GoogleAntigravity => ProviderClient::OpenAiCompat {
                client: http_client(),
                api_key,
                base_url: config
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "https://generativelanguage.googleapis.com".to_string()),
                model: config.model.clone(),
            },
        }
    }

    pub async fn generate(&self, prompt: &ResolvedPrompt) -> Result<String, GatewayError> {
        self.generate_with_secret_store(prompt, None, None, None)
            .await
    }

    pub async fn generate_with_secret_store(
        &self,
        prompt: &ResolvedPrompt,
        secret_store: Option<&crate::secret_store::SecretStore>,
        provider_id: Option<&str>,
        token_url_override: Option<&str>,
    ) -> Result<String, GatewayError> {
        let mut key = match self {
            ProviderClient::Anthropic { api_key, .. } => api_key.clone(),
            ProviderClient::OpenAiCompat { api_key, .. } => api_key.clone(),
            ProviderClient::Onyx { pat, .. } => pat.clone(),
        };

        let pid = provider_id.unwrap_or(match self {
            ProviderClient::Anthropic { .. } => "anthropic",
            ProviderClient::OpenAiCompat { .. } => "openai-codex",
            ProviderClient::Onyx { .. } => "onyx",
        });

        // The configured auth mode decides, and `config::provider_api_key`
        // encodes it: OAuth resolves to the `oauth:<id>` sentinel, an API key to
        // the key itself.
        let is_oauth = key.starts_with("oauth:");

        // Only an OAuth-configured provider reads the vault. A leftover token
        // must not silently upgrade an `api_key` provider, because the request
        // would then carry the OAuth client fingerprint while
        // `provider_config_digest` omits it for API-key auth: the approved
        // identity would stop describing the wire.
        if is_oauth {
            if let Some(store) = secret_store {
                if let Ok(Some(tokens)) = store.get_oauth_tokens(pid) {
                    if !tokens.access_token.is_empty() && !tokens.disabled {
                        key = tokens.access_token;
                    }
                }
            }
        }

        let res = self.generate_raw(prompt, &key, is_oauth).await;

        if let Err(GatewayError::ProviderError { status: 401, .. }) = &res {
            if is_oauth {
                if let Some(store) = secret_store {
                    let refresher = crate::oauth::refresher::OAuthRefresher::new(store.clone());
                    if let Ok(new_token) = refresher
                        .refresh_provider_now(pid, token_url_override)
                        .await
                    {
                        return self.generate_raw(prompt, &new_token, is_oauth).await;
                    }
                }
            }
        }

        res
    }

    async fn generate_raw(
        &self,
        prompt: &ResolvedPrompt,
        key: &str,
        is_oauth: bool,
    ) -> Result<String, GatewayError> {
        match self {
            ProviderClient::Anthropic {
                client,
                base_url,
                model,
                ..
            } => generate_anthropic(client, key, base_url, model, prompt, is_oauth).await,
            ProviderClient::OpenAiCompat {
                client,
                base_url,
                model,
                ..
            } => generate_openai_compat(client, key, base_url, model, prompt).await,
            ProviderClient::Onyx {
                client,
                base_url,
                model,
                ..
            } => generate_onyx(client, key, base_url, model, prompt).await,
        }
    }
}

fn messages_json(prompt: &ResolvedPrompt) -> Vec<Value> {
    prompt
        .messages
        .iter()
        .map(|m| {
            let role = match m.role {
                super::PromptRole::User => "user",
                super::PromptRole::Assistant => "assistant",
            };
            json!({ "role": role, "content": m.content })
        })
        .collect()
}

async fn generate_anthropic(
    client: &reqwest::Client,
    api_key: &str,
    base_url: &str,
    model: &str,
    prompt: &ResolvedPrompt,
    is_oauth: bool,
) -> Result<String, GatewayError> {
    // An OAuth grant is only honoured for the first-party client surface, which
    // includes a leading client system block. The agent's own preamble, which
    // is what the prompt template digest covers, follows it unchanged.
    let system = if is_oauth {
        json!([
            { "type": "text", "text": crate::anthropic_fingerprint::OAUTH_CLIENT_INSTRUCTION },
            { "type": "text", "text": prompt.system },
        ])
    } else {
        json!(prompt.system)
    };
    let body = json!({
        "model": model,
        "max_tokens": prompt.max_tokens,
        "system": system,
        "messages": messages_json(prompt),
    });

    let mut req = client.post(format!("{base_url}/v1/messages"));
    if is_oauth {
        // An OAuth grant is only honoured for the first-party client surface.
        // Bearer alone is rejected: `anthropic-beta: oauth-2025-04-20` is what
        // admits the token, and the two client markers accompany it.
        req = req
            .bearer_auth(api_key)
            .header("anthropic-beta", crate::anthropic_fingerprint::OAUTH_BETA)
            .header(
                "anthropic-dangerous-direct-browser-access",
                crate::anthropic_fingerprint::OAUTH_DIRECT_BROWSER_ACCESS,
            )
            .header("x-app", crate::anthropic_fingerprint::OAUTH_APP)
            .header("user-agent", crate::anthropic_fingerprint::OAUTH_USER_AGENT);
    } else {
        req = req.header("x-api-key", api_key);
    }

    let response = req
        .header("anthropic-version", ANTHROPIC_API_VERSION)
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        return Err(GatewayError::ProviderError {
            provider: "anthropic".to_string(),
            status: status.as_u16(),
            body: text,
        });
    }

    let value: Value = serde_json::from_str(&text)
        .map_err(|_| GatewayError::MissingContent("anthropic".to_string()))?;
    value
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|blocks| blocks.first())
        .and_then(|block| block.get("text"))
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .ok_or_else(|| GatewayError::MissingContent("anthropic".to_string()))
}

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

async fn generate_onyx(
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

async fn generate_openai_compat(
    client: &reqwest::Client,
    api_key: &str,
    base_url: &str,
    model: &str,
    prompt: &ResolvedPrompt,
) -> Result<String, GatewayError> {
    let mut messages = vec![json!({ "role": "system", "content": prompt.system })];
    messages.extend(messages_json(prompt));
    let body = json!({
        "model": model,
        "max_tokens": prompt.max_tokens,
        "messages": messages,
    });

    let response = client
        .post(format!("{base_url}/v1/chat/completions"))
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        return Err(GatewayError::ProviderError {
            provider: "openai_compat".to_string(),
            status: status.as_u16(),
            body: text,
        });
    }

    let value: Value = serde_json::from_str(&text)
        .map_err(|_| GatewayError::MissingContent("openai_compat".to_string()))?;
    value
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .ok_or_else(|| GatewayError::MissingContent("openai_compat".to_string()))
}

#[cfg(test)]
mod tests;

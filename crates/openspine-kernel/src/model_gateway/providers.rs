//! Provider HTTP clients (build plan 4c).
//!
//! Two provider kinds, enum-dispatched (no `dyn`/`async_trait` — this repo's
//! no-new-deps convention and the small, closed set of kinds don't justify
//! either): `anthropic` calls the Messages API; `openai_compat` calls
//! `/v1/chat/completions`, the shape most OpenAI-compatible providers share.

use serde_json::{json, Value};

use crate::config::{ProviderConfig, ProviderKind};

use super::ResolvedPrompt;

const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com";
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
        }
    }

    pub async fn generate(&self, prompt: &ResolvedPrompt) -> Result<String, GatewayError> {
        match self {
            ProviderClient::Anthropic {
                client,
                api_key,
                base_url,
                model,
            } => generate_anthropic(client, api_key, base_url, model, prompt).await,
            ProviderClient::OpenAiCompat {
                client,
                api_key,
                base_url,
                model,
            } => generate_openai_compat(client, api_key, base_url, model, prompt).await,
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
) -> Result<String, GatewayError> {
    let body = json!({
        "model": model,
        "max_tokens": prompt.max_tokens,
        "system": prompt.system,
        "messages": messages_json(prompt),
    });

    let response = client
        .post(format!("{base_url}/v1/messages"))
        .header("x-api-key", api_key)
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
mod tests {
    use super::*;
    use crate::model_gateway::GatewayTierMap;
    use crate::model_gateway::PromptMessage;
    use openspine_schemas::workflow::ReasoningTier;
    use std::collections::HashMap;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn prompt() -> ResolvedPrompt {
        ResolvedPrompt {
            system: "You are Lyra.".to_string(),
            messages: vec![PromptMessage {
                role: super::super::PromptRole::User,
                content: "hello".to_string(),
            }],
            max_tokens: 100,
            reasoning_tier: openspine_schemas::workflow::ReasoningTier::Standard,
        }
    }

    #[tokio::test]
    async fn anthropic_client_parses_the_reply_text() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "test-key"))
            .and(header("anthropic-version", ANTHROPIC_API_VERSION))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "hi owner"}]
            })))
            .mount(&server)
            .await;

        let client = ProviderClient::Anthropic {
            client: http_client(),
            api_key: "test-key".to_string(),
            base_url: server.uri(),
            model: "test-model".to_string(),
        };
        let text = client.generate(&prompt()).await.unwrap();
        assert_eq!(text, "hi owner");
    }

    #[tokio::test]
    async fn openai_compat_client_parses_the_reply_text() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": "hi owner"}}]
            })))
            .mount(&server)
            .await;

        let client = ProviderClient::OpenAiCompat {
            client: http_client(),
            api_key: "test-key".to_string(),
            base_url: server.uri(),
            model: "test-model".to_string(),
        };
        let text = client.generate(&prompt()).await.unwrap();
        assert_eq!(text, "hi owner");
    }

    #[tokio::test]
    async fn provider_error_status_surfaces_as_provider_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;

        let client = ProviderClient::Anthropic {
            client: http_client(),
            api_key: "bad-key".to_string(),
            base_url: server.uri(),
            model: "test-model".to_string(),
        };
        let err = client.generate(&prompt()).await.unwrap_err();
        assert!(matches!(
            err,
            GatewayError::ProviderError { status: 401, .. }
        ));
    }

    #[tokio::test]
    async fn malformed_response_is_missing_content_not_a_panic() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"unexpected": true})))
            .mount(&server)
            .await;

        let client = ProviderClient::Anthropic {
            client: http_client(),
            api_key: "test-key".to_string(),
            base_url: server.uri(),
            model: "test-model".to_string(),
        };
        let err = client.generate(&prompt()).await.unwrap_err();
        assert!(matches!(err, GatewayError::MissingContent(_)));
    }
    #[tokio::test]
    async fn declared_high_tier_selects_high_provider_endpoint() {
        let standard_server = MockServer::start().await;
        let high_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "high reply"}]
            })))
            .mount(&high_server)
            .await;
        let mut pool = HashMap::new();
        pool.insert(
            "standard-provider".to_string(),
            ProviderClient::Anthropic {
                client: http_client(),
                api_key: "test-key".to_string(),
                base_url: standard_server.uri(),
                model: "standard-model".to_string(),
            },
        );
        pool.insert(
            "high-provider".to_string(),
            ProviderClient::Anthropic {
                client: http_client(),
                api_key: "test-key".to_string(),
                base_url: high_server.uri(),
                model: "high-model".to_string(),
            },
        );
        let map = GatewayTierMap::new().with_route(ReasoningTier::High, "high-provider");
        let provider = map
            .resolve(ReasoningTier::High, "standard-provider", &pool)
            .expect("high tier route must resolve");
        let response = provider
            .generate(&ResolvedPrompt {
                reasoning_tier: ReasoningTier::High,
                ..prompt()
            })
            .await
            .unwrap();
        assert_eq!(response, "high reply");
        assert_eq!(high_server.received_requests().await.unwrap().len(), 1);
        assert_eq!(standard_server.received_requests().await.unwrap().len(), 0);
    }
}

use super::*;
use crate::model_gateway::GatewayTierMap;
use crate::model_gateway::PromptMessage;
use openspine_schemas::workflow::ReasoningTier;
use std::collections::HashMap;
use wiremock::matchers::{body_json, header, method, path};
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
async fn onyx_client_uses_scoped_chat_api_and_parses_answer() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/send-chat-message"))
        .and(header("authorization", "Bearer pat-token"))
        .and(body_json(json!({
            "message": "hello",
            "llm_override": {"model_version": "test-model"},
            "allowed_tool_ids": [],
            "origin": "api",
            "stream": false,
            "include_citations": false,
            "additional_context": "OpenSpine system instructions:\nYou are Lyra."
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "answer": "hi owner",
            "answer_citationless": "hi owner",
            "error_msg": null
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = ProviderClient::Onyx {
        client: http_client(),
        pat: "pat-token".to_string(),
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

#[tokio::test]
async fn gateway_injects_oauth_bearer_token_from_vault() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("authorization", "Bearer vault-bearer-token-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "oauth reply"}]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let store = crate::secret_store::SecretStore::open(dir.path().join("credentials"), [18; 32])
        .expect("open");

    store
        .store_oauth_tokens(
            "anthropic",
            "ref-token",
            "vault-bearer-token-123",
            "9999999999",
            None,
        )
        .expect("store oauth");

    let client = ProviderClient::Anthropic {
        client: http_client(),
        api_key: "oauth:anthropic".to_string(),
        base_url: server.uri(),
        model: "test-model".to_string(),
    };

    let res = client
        .generate_with_secret_store(&prompt(), Some(&store), Some("anthropic"), None)
        .await
        .expect("generate with store");

    assert_eq!(res, "oauth reply");
}

#[tokio::test]
async fn gateway_recovers_from_transient_401_via_inline_token_refresh() {
    let token_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "refreshed-bearer-401-token",
            "expires_in": 3600
        })))
        .mount(&token_server)
        .await;

    let api_server = MockServer::start().await;
    // First call fails 401
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("authorization", "Bearer stale-token"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .expect(1)
        .mount(&api_server)
        .await;

    // Second call with refreshed token succeeds 200
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("authorization", "Bearer refreshed-bearer-401-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "recovered reply"}]
        })))
        .expect(1)
        .mount(&api_server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let store = crate::secret_store::SecretStore::open(dir.path().join("credentials"), [19; 32])
        .expect("open");

    store
        .store_oauth_tokens("anthropic", "valid-refresh", "stale-token", "1000", None)
        .expect("store oauth");

    let client = ProviderClient::Anthropic {
        client: http_client(),
        api_key: "oauth:anthropic".to_string(),
        base_url: api_server.uri(),
        model: "test-model".to_string(),
    };

    let token_url = format!("{}/v1/oauth/token", token_server.uri());

    let res = client
        .generate_with_secret_store(&prompt(), Some(&store), Some("anthropic"), Some(&token_url))
        .await
        .expect("generate recover 401");

    assert_eq!(res, "recovered reply");
}

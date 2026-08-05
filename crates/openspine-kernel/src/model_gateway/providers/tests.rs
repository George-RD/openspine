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

    // The inline refresh presents the provider's registered client id, so this
    // test registers one instead of relying on a sibling to have set it.
    std::env::set_var("OPENSPINE_ANTHROPIC_CLIENT_ID", "test-client-id");

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

/// An Anthropic OAuth grant is only honoured for the client surface it was
/// issued against. Bearer alone is rejected, so the whole contract is pinned:
/// omitting any part of it fails against the live provider in a way a mock
/// would otherwise hide.
#[tokio::test]
async fn an_oauth_request_carries_the_full_first_party_client_contract() {
    use crate::anthropic_fingerprint;

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "oauth reply"}]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let store = crate::secret_store::SecretStore::open(dir.path().join("credentials"), [22; 32])
        .expect("open");
    store
        .store_oauth_tokens("anthropic", "ref", "bearer-token", "9999999999", None)
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
        .expect("generate");

    assert_eq!(res, "oauth reply");
    let request = &server.received_requests().await.expect("requests")[0];
    for (name, expected) in [
        ("anthropic-beta", anthropic_fingerprint::OAUTH_BETA),
        (
            "anthropic-dangerous-direct-browser-access",
            anthropic_fingerprint::OAUTH_DIRECT_BROWSER_ACCESS,
        ),
        ("x-app", anthropic_fingerprint::OAUTH_APP),
        ("user-agent", anthropic_fingerprint::OAUTH_USER_AGENT),
    ] {
        assert_eq!(
            request.headers.get(name).map(|v| v.to_str().unwrap()),
            Some(expected),
            "header {name}"
        );
    }
}

/// The client system block is prepended, and the agent's own preamble follows
/// it byte for byte: the prompt template digest still describes what was sent.
#[tokio::test]
async fn an_oauth_request_prepends_the_client_block_without_altering_the_preamble() {
    use crate::anthropic_fingerprint;

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "ok"}]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let store = crate::secret_store::SecretStore::open(dir.path().join("credentials"), [23; 32])
        .expect("open");
    store
        .store_oauth_tokens("anthropic", "ref", "bearer-token", "9999999999", None)
        .expect("store oauth");

    let client = ProviderClient::Anthropic {
        client: http_client(),
        api_key: "oauth:anthropic".to_string(),
        base_url: server.uri(),
        model: "test-model".to_string(),
    };
    let sent = prompt();
    client
        .generate_with_secret_store(&sent, Some(&store), Some("anthropic"), None)
        .await
        .expect("generate");

    let request = &server.received_requests().await.expect("requests")[0];
    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json body");
    let system = body["system"].as_array().expect("system blocks");
    assert_eq!(system.len(), 2, "{system:?}");
    assert_eq!(
        system[0]["text"].as_str(),
        Some(anthropic_fingerprint::OAUTH_CLIENT_INSTRUCTION)
    );
    assert_eq!(system[1]["text"].as_str(), Some(sent.system.as_str()));
}

/// An API-key request must not carry the OAuth client surface: it is a
/// different client, and the plain string system field is the API contract.
#[tokio::test]
async fn an_api_key_request_carries_no_oauth_client_surface() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "ok"}]
        })))
        .mount(&server)
        .await;

    let client = ProviderClient::Anthropic {
        client: http_client(),
        api_key: "sk-plain-api-key".to_string(),
        base_url: server.uri(),
        model: "test-model".to_string(),
    };
    client.generate(&prompt()).await.expect("generate");

    let request = &server.received_requests().await.expect("requests")[0];
    assert!(request.headers.get("anthropic-beta").is_none());
    assert!(request.headers.get("x-app").is_none());
    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json body");
    assert!(body["system"].is_string(), "{}", body["system"]);
}

/// An owner who logs in with OAuth and later switches the provider back to
/// `auth.mode: api_key` leaves a usable token in the vault. Honouring it would
/// send the OAuth client fingerprint on a request whose `provider_config_digest`
/// omits it, so the approved identity would stop describing the wire.
#[tokio::test]
async fn a_stale_vault_token_never_overrides_a_configured_api_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "ok"}]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let store = crate::secret_store::SecretStore::open(dir.path().join("credentials"), [24; 32])
        .expect("open");
    store
        .store_oauth_tokens("anthropic", "ref", "stale-vault-token", "9999999999", None)
        .expect("store oauth");

    // Configured as an API key, which is what `config::provider_api_key`
    // resolves for `auth.mode: api_key`.
    let client = ProviderClient::Anthropic {
        client: http_client(),
        api_key: "sk-configured-api-key".to_string(),
        base_url: server.uri(),
        model: "test-model".to_string(),
    };
    client
        .generate_with_secret_store(&prompt(), Some(&store), Some("anthropic"), None)
        .await
        .expect("generate");

    let request = &server.received_requests().await.expect("requests")[0];
    assert_eq!(
        request
            .headers
            .get("x-api-key")
            .map(|v| v.to_str().unwrap()),
        Some("sk-configured-api-key")
    );
    assert!(request.headers.get("authorization").is_none());
    assert!(
        request.headers.get("anthropic-beta").is_none(),
        "an api_key request must not carry the OAuth client fingerprint"
    );
    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json body");
    assert!(body["system"].is_string(), "{}", body["system"]);
}

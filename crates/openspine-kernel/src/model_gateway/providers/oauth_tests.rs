//! Anthropic OAuth wire-contract tests.
//!
//! Split from `tests.rs` for the 500-line module gate.

use super::*;
use crate::model_gateway::PromptMessage;
use crate::model_gateway::ProviderCredential;
use wiremock::matchers::{method, path};
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
        credential: ProviderCredential::Oauth,
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
        credential: ProviderCredential::Oauth,
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
        credential: ProviderCredential::ApiKey("sk-plain-api-key".to_string()),
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
        credential: ProviderCredential::ApiKey("sk-configured-api-key".to_string()),
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

/// Logging in against a provider still configured `api_key` is the ordinary
/// case: the login is the act of moving it to OAuth, and the configuration is
/// only rewritten after verification succeeds. The verification client must
/// therefore authenticate as OAuth regardless of what the entry currently says.
#[tokio::test]
async fn a_client_built_for_oauth_reads_the_vault_whatever_the_entry_said() {
    use crate::config::{ProviderAuth, ProviderConfig, ProviderKind};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "verified"}]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let store = crate::secret_store::SecretStore::open(dir.path().join("credentials"), [26; 32])
        .expect("open");
    store
        .store_oauth_tokens("anthropic", "ref", "vault-token", "9999999999", None)
        .expect("store oauth");

    let verifying = ProviderConfig {
        id: "anthropic".to_string(),
        kind: ProviderKind::Anthropic,
        base_url: Some(server.uri()),
        model: "test-model".to_string(),
        auth: ProviderAuth::Oauth,
    };
    // The resolved key is irrelevant for OAuth: the mode decides, not the string.
    let client = ProviderClient::from_config(&verifying, String::new());
    client
        .generate_with_secret_store(&prompt(), Some(&store), Some("anthropic"), None)
        .await
        .expect("generate");

    let request = &server.received_requests().await.expect("requests")[0];
    assert_eq!(
        request
            .headers
            .get("authorization")
            .map(|v| v.to_str().unwrap()),
        Some("Bearer vault-token")
    );
}

/// The inverse: an API key whose value equals the old sentinel is still an API
/// key, because the mode is carried by the type rather than the string.
#[tokio::test]
async fn an_api_key_equal_to_the_old_sentinel_is_still_an_api_key() {
    use crate::config::{ProviderAuth, ProviderConfig, ProviderKind};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "ok"}]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let store = crate::secret_store::SecretStore::open(dir.path().join("credentials"), [27; 32])
        .expect("open");
    store
        .store_oauth_tokens("anthropic", "ref", "vault-token", "9999999999", None)
        .expect("store oauth");

    let configured = ProviderConfig {
        id: "anthropic".to_string(),
        kind: ProviderKind::Anthropic,
        base_url: Some(server.uri()),
        model: "test-model".to_string(),
        auth: ProviderAuth::ApiKey {
            env: "ANTHROPIC_API_KEY".to_string(),
        },
    };
    let client = ProviderClient::from_config(&configured, "oauth:anthropic".to_string());
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
        Some("oauth:anthropic"),
        "the declared mode decides, not the credential's contents"
    );
    assert!(request.headers.get("authorization").is_none());
    assert!(request.headers.get("anthropic-beta").is_none());
}

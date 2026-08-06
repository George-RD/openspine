//! Codex Responses transport wire-contract tests.
//!
//! Split from `tests.rs` for the 500-line module gate. These pin the exact
//! surface the ChatGPT backend accepts: URL path, identifying headers,
//! mandated body fields, SSE consumption, and the 401 refresh-once recovery.

use super::*;
use crate::codex_fingerprint;
use crate::model_gateway::PromptMessage;
use crate::model_gateway::ProviderCredential;
use crate::secret_store::{OAuthIdentityMetadata, SecretStore};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
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

fn codex_client(base_url: String) -> ProviderClient {
    ProviderClient::CodexResponses {
        client: http_client(),
        credential: ProviderCredential::Oauth,
        base_url,
        model: "gpt-5-codex".to_string(),
    }
}

fn store_with_identity(dir: &std::path::Path, account_id: Option<&str>) -> SecretStore {
    let store = SecretStore::open(dir.join("credentials"), [24; 32]).expect("open");
    store
        .store_oauth_tokens(
            "openai-codex",
            "refresh-1",
            "access-1",
            "9999999999",
            account_id.map(|id| OAuthIdentityMetadata {
                account_email: None,
                account_id: Some(id.to_string()),
                identity_key: None,
            }),
        )
        .expect("store oauth");
    store
}

fn success_sse() -> &'static str {
    concat!(
        "data: {\"type\":\"response.created\",\"response\":{}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"codex \"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"reply\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n",
    )
}

#[tokio::test]
async fn codex_generate_sends_the_registered_wire_shape_and_reads_sse() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .and(header("chatgpt-account-id", "acct-777"))
        .and(header("OpenAI-Beta", codex_fingerprint::OPENAI_BETA))
        .and(header("originator", codex_fingerprint::ORIGINATOR))
        .and(header("Accept", "text/event-stream"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(success_sse()),
        )
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_with_identity(dir.path(), Some("acct-777"));

    let reply = codex_client(server.uri())
        .generate_with_secret_store(&prompt(), Some(&store), Some("openai-codex"), None)
        .await
        .expect("generate");
    assert_eq!(reply, "codex reply");

    let request = &server.received_requests().await.expect("requests")[0];
    assert_eq!(
        request
            .headers
            .get("authorization")
            .map(|v| v.to_str().unwrap()),
        Some("Bearer access-1"),
        "the vault access token must be spent"
    );
    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json body");
    assert_eq!(body["store"], false, "the endpoint mandates store:false");
    assert_eq!(body["stream"], true, "the endpoint mandates stream:true");
    assert_eq!(body["instructions"], "You are Lyra.");
    assert_eq!(body["model"], "gpt-5-codex");
    assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
}

#[tokio::test]
async fn codex_generate_falls_back_to_completed_output_when_no_deltas_arrive() {
    let server = MockServer::start().await;
    let sse = "data: {\"type\":\"response.completed\",\"response\":{\"output\":[\
               {\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"final only\"}]}\
               ]}}\n\n";
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_string(sse))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_with_identity(dir.path(), Some("acct-777"));

    let reply = codex_client(server.uri())
        .generate_with_secret_store(&prompt(), Some(&store), Some("openai-codex"), None)
        .await
        .expect("generate");
    assert_eq!(reply, "final only");
}

#[tokio::test]
async fn codex_generate_maps_response_failed_to_a_provider_error() {
    let server = MockServer::start().await;
    let sse = "data: {\"type\":\"response.failed\",\"response\":{\"error\":\
               {\"code\":\"usage_limit_reached\",\"message\":\"usage limit reached\"}}}\n\n";
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_string(sse))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_with_identity(dir.path(), Some("acct-777"));

    let error = codex_client(server.uri())
        .generate_with_secret_store(&prompt(), Some(&store), Some("openai-codex"), None)
        .await
        .expect_err("must surface the upstream failure");
    assert!(
        error.to_string().contains("usage limit reached"),
        "upstream message must survive verbatim: {error}"
    );
}

#[tokio::test]
async fn codex_generate_refuses_when_no_account_id_is_stored() {
    let server = MockServer::start().await;
    // No mock mounted: a request reaching the server would 404 and the
    // assertion below on received requests would catch it anyway.

    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_with_identity(dir.path(), None);

    let error = codex_client(server.uri())
        .generate_with_secret_store(&prompt(), Some(&store), Some("openai-codex"), None)
        .await
        .expect_err("a credential with no account id must be refused");
    assert!(
        matches!(error, GatewayError::MissingAccountId(_)),
        "{error}"
    );
    assert!(
        error.to_string().contains("openspine provider login"),
        "the remedy must be named: {error}"
    );
    assert!(
        server
            .received_requests()
            .await
            .expect("requests")
            .is_empty(),
        "no request may reach the backend without the account header"
    );
}

#[tokio::test]
async fn codex_generate_retries_once_after_a_401_via_inline_refresh() {
    let server = MockServer::start().await;

    // The refreshed access token is a JWT carrying the account claim, exactly
    // as the real token endpoint issues it.
    let header_b64 = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
    let payload_b64 = URL_SAFE_NO_PAD.encode(
        serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": "acct-777" }
        })
        .to_string()
        .as_bytes(),
    );
    let refreshed_access = format!("{header_b64}.{payload_b64}.sig");

    // First spend: the stale vault token draws a 401.
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .and(header("authorization", "Bearer access-1"))
        .respond_with(ResponseTemplate::new(401).set_body_string("expired"))
        .expect(1)
        .mount(&server)
        .await;

    // Inline refresh against the overridden token endpoint.
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": refreshed_access,
            "refresh_token": "refresh-2",
            "expires_in": 3600,
            "token_type": "Bearer"
        })))
        .expect(1)
        .mount(&server)
        .await;

    // Second spend with the refreshed token succeeds.
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .and(header(
            "authorization",
            format!("Bearer {refreshed_access}").as_str(),
        ))
        .and(header("chatgpt-account-id", "acct-777"))
        .respond_with(ResponseTemplate::new(200).set_body_string(success_sse()))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_with_identity(dir.path(), Some("acct-777"));

    let token_url = format!("{}/oauth/token", server.uri());
    let reply = codex_client(server.uri())
        .generate_with_secret_store(
            &prompt(),
            Some(&store),
            Some("openai-codex"),
            Some(&token_url),
        )
        .await
        .expect("generate after refresh");
    assert_eq!(reply, "codex reply");
}

/// A backend error body that echoes the bearer or account id must not reach
/// logs verbatim: the gateway error carries `<redacted>` in their place.
#[tokio::test]
async fn codex_error_bodies_never_echo_the_bearer_or_account_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_string("forbidden for token access-1 on account acct-777"),
        )
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_with_identity(dir.path(), Some("acct-777"));

    let error = codex_client(server.uri())
        .generate_with_secret_store(&prompt(), Some(&store), Some("openai-codex"), None)
        .await
        .expect_err("403 must fail");
    let rendered = error.to_string();
    assert!(!rendered.contains("access-1"), "{rendered}");
    assert!(!rendered.contains("acct-777"), "{rendered}");
    assert!(rendered.contains("<redacted>"), "{rendered}");
}

/// When another call refreshed the credential between this call's vault read
/// and its 401, the newer stored token is spent directly — no competing
/// refresh grant is submitted (with rotation, a duplicate grant draws
/// `invalid_grant` and would disable a freshly renewed credential).
#[tokio::test]
async fn a_401_spends_a_concurrently_refreshed_token_instead_of_refreshing_again() {
    use std::sync::Arc;
    use wiremock::{Request, Respond};

    /// Responds 401 to the stale bearer and, as a side effect, simulates a
    /// concurrent task completing a refresh by rotating the vault contents.
    struct RotateVaultOn401 {
        store: SecretStore,
    }
    impl Respond for RotateVaultOn401 {
        fn respond(&self, _: &Request) -> ResponseTemplate {
            self.store
                .store_oauth_tokens(
                    "openai-codex",
                    "refresh-2",
                    "token-fresh",
                    "9999999999",
                    Some(OAuthIdentityMetadata {
                        account_email: None,
                        account_id: Some("acct-777".to_string()),
                        identity_key: None,
                    }),
                )
                .expect("rotate vault");
            ResponseTemplate::new(401).set_body_string("expired")
        }
    }

    let server = MockServer::start().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_with_identity(dir.path(), Some("acct-777"));

    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .and(header("authorization", "Bearer access-1"))
        .respond_with(RotateVaultOn401 {
            store: store.clone(),
        })
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .and(header("authorization", "Bearer token-fresh"))
        .respond_with(ResponseTemplate::new(200).set_body_string(success_sse()))
        .expect(1)
        .mount(&server)
        .await;
    // No /oauth/token mock: a competing refresh grant would 404, fail the
    // refresh, and surface the original 401 — failing this test.
    let dead_token_url = format!("{}/oauth/token", server.uri());

    let reply = Arc::new(codex_client(server.uri()))
        .generate_with_secret_store(
            &prompt(),
            Some(&store),
            Some("openai-codex"),
            Some(&dead_token_url),
        )
        .await
        .expect("retry with the concurrently refreshed token");
    assert_eq!(reply, "codex reply");
}

/// Every refresher construction shares one process-wide single-flight set:
/// per-call construction must not defeat refresh coordination.
#[tokio::test]
async fn refreshers_constructed_separately_share_one_in_flight_set() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_with_identity(dir.path(), Some("acct-777"));
    let a = crate::oauth::refresher::OAuthRefresher::new(store.clone());
    let b = crate::oauth::refresher::OAuthRefresher::new(store);
    assert!(
        std::sync::Arc::ptr_eq(&a.in_flight, &b.in_flight),
        "single-flight only means anything when every construction shares it"
    );
}

//! Provider login, verification, and role-binding tests.

use super::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TEST_CLIENT_ID: &str = "test-client-id";

fn vault(tag: &str) -> (tempfile::TempDir, SecretStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SecretStore::open(dir.path().join(tag), [20; 32]).expect("open");
    (dir, store)
}

#[test]
fn every_supported_provider_builds_a_loopback_authorization_url() {
    for provider_id in OAUTH_PROVIDER_IDS {
        let port = default_port(provider_id).expect("port");
        let auth = begin_with_client_id(provider_id, port, TEST_CLIENT_ID).expect("begin");

        assert!(
            auth.url.contains(&format!("127.0.0.1%3A{port}%2Fcallback"))
                || auth.url.contains(&format!("127.0.0.1:{port}/callback")),
            "{provider_id}: {}",
            auth.url
        );
        assert!(auth.url.contains("code_challenge="), "{}", auth.url);
        assert!(auth.url.contains("S256"), "{}", auth.url);
        assert!(!auth.state().is_empty());
    }
}

#[test]
fn an_unsupported_provider_is_refused_before_any_network_call() {
    assert!(default_port("not-a-provider").is_err());
    assert!(begin_with_client_id("not-a-provider", 1234, TEST_CLIENT_ID).is_err());
}

/// Codex authorizes fine and then cannot be spent: its grant is only accepted by
/// a Responses transport against `chatgpt.com/backend-api` that the gateway does
/// not implement. Storing that credential would be the same dead end as a
/// placeholder client id, reached one step later.
#[test]
fn a_provider_whose_credential_cannot_be_spent_is_refused_before_a_url_exists() {
    for provider_id in ["openai-codex", "google-antigravity"] {
        let error = begin(provider_id, 1455).expect_err("must refuse");
        let rendered = error.to_string();
        assert!(
            rendered.contains("not available in this build"),
            "{rendered}"
        );
        assert!(rendered.contains("API-key or local provider"), "{rendered}");
        assert!(
            !OAUTH_PROVIDER_IDS.contains(&provider_id),
            "{provider_id} must not be offered"
        );
    }
}

/// The headless path is the one an SSH owner actually takes: the URL is printed,
/// the code is pasted back, and the exchange still has to carry the PKCE
/// verifier that `begin` generated.
#[tokio::test]
async fn a_pasted_authorization_code_completes_the_login_and_stores_the_credential() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "access-value",
            "refresh_token": "refresh-value",
            "expires_in": 3600,
            "account_email": "owner@example.com"
        })))
        .mount(&server)
        .await;
    let (_dir, store) = vault("credentials");
    let auth = begin_with_client_id("anthropic", 54545, TEST_CLIENT_ID).expect("begin");

    let stored = finish(
        &auth,
        "pasted-code",
        &store,
        &reqwest::Client::new(),
        Some(&format!("{}/token", server.uri())),
    )
    .await
    .expect("finish");

    assert_eq!(stored.account_email.as_deref(), Some("owner@example.com"));
    let tokens = store
        .get_oauth_tokens("anthropic")
        .unwrap()
        .expect("tokens");
    assert_eq!(tokens.access_token, "access-value");
    assert_eq!(tokens.refresh_token, "refresh-value");
    assert!(!tokens.disabled);
}

/// Storing a placeholder refresh token would leave a credential the background
/// refresher later kills with a confusing `invalid_grant`.
#[tokio::test]
async fn a_first_login_without_a_refresh_token_is_refused_and_stores_nothing() {
    let (_dir, store) = vault("credentials");

    let error = exchange_without_refresh_token(&store)
        .await
        .expect_err("must refuse");

    assert!(error.to_string().contains("no refresh token"), "{error:#}");
    assert_eq!(store.get_oauth_tokens("anthropic").unwrap(), None);
}

/// Providers commonly issue a refresh token only on the first authorization. A
/// re-login must keep the real stored token instead of failing or fabricating.
#[tokio::test]
async fn a_relogin_without_a_refresh_token_keeps_the_stored_one() {
    let (_dir, store) = vault("credentials");
    store
        .store_oauth_tokens("anthropic", "original-refresh", "old-access", "1", None)
        .expect("seed");

    let stored = exchange_without_refresh_token(&store)
        .await
        .expect("finish");

    assert_eq!(stored.provider_id, "anthropic");
    let tokens = store
        .get_oauth_tokens("anthropic")
        .unwrap()
        .expect("tokens");
    assert_eq!(tokens.refresh_token, "original-refresh");
    assert_eq!(tokens.access_token, "access-value");
}

async fn exchange_without_refresh_token(
    store: &SecretStore,
) -> Result<StoredCredential, anyhow::Error> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "access-value",
            "expires_in": 3600
        })))
        .mount(&server)
        .await;
    let auth = begin_with_client_id("anthropic", 54545, TEST_CLIENT_ID).expect("begin");

    finish(
        &auth,
        "pasted-code",
        store,
        &reqwest::Client::new(),
        Some(&format!("{}/token", server.uri())),
    )
    .await
}

#[tokio::test]
async fn setup_wizard_runs_preflight_verification_ping() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "content": [{"type": "text", "text": "verified ping response"}]
        })))
        .mount(&server)
        .await;

    let (_dir, store) = vault("credentials");
    store
        .store_oauth_tokens(
            "anthropic",
            "ref-token",
            "valid-ping-access-token",
            "9999999999",
            None,
        )
        .expect("store oauth");

    let client = ProviderClient::Anthropic {
        client: reqwest::Client::new(),
        api_key: "oauth:anthropic".to_string(),
        base_url: server.uri(),
        model: "test-model".to_string(),
    };

    let verified = run_preflight_verification_ping(&client, &store, "anthropic")
        .await
        .expect("ping");

    assert!(verified);
}

const OAUTH_CONFIG: &str = r#"
data_dir: data
sandbox:
  driver: process
owner:
  telegram_user_id: 123456789
  display_name: George
providers:
  - id: google-antigravity
    kind: google_antigravity
    model: gemini-2.5-flash
    auth:
      mode: oauth
spend_cap:
  model_calls_per_day: 100
  connector_calls_per_day: 500
unsafe_allow_uncontained_private_data: false
"#;

#[tokio::test]
async fn setup_wizard_binds_active_model_roles_only_on_successful_verification() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let yaml_path = dir.path().join("openspine.yaml");
    std::fs::write(&yaml_path, OAUTH_CONFIG).expect("write initial config");
    let before = std::fs::read_to_string(&yaml_path).expect("read");

    let store = SecretStore::open(dir.path().join("credentials"), [21; 32]).expect("open");
    store
        .store_oauth_tokens("anthropic", "ref", "access", "9999999999", None)
        .expect("store");
    let client = ProviderClient::Anthropic {
        client: reqwest::Client::new(),
        api_key: "oauth:anthropic".to_string(),
        base_url: server.uri(),
        model: "test-model".to_string(),
    };

    let verified = run_preflight_verification_ping(&client, &store, "anthropic")
        .await
        .unwrap_or(false);

    assert!(!verified, "a failing provider must not verify");
    // The wizard binds roles only on a successful ping, so a failed
    // verification leaves the configuration byte-identical.
    assert_eq!(std::fs::read_to_string(&yaml_path).expect("read"), before);
    assert!(
        store.get_oauth_tokens("anthropic").unwrap().is_some(),
        "the credential survives so a retry skips the authorization"
    );

    update_openspine_yaml_roles(
        &yaml_path,
        "google-antigravity",
        ProviderKind::GoogleAntigravity,
        "gemini-2.5-flash",
    )
    .expect("update roles");

    let reloaded = std::fs::read_to_string(&yaml_path).expect("read updated config");
    assert!(reloaded.contains("google-antigravity"));
    assert!(reloaded.contains("mode: oauth"));
}

#[test]
fn binding_an_unconfigured_provider_appends_it_with_the_named_model() {
    let dir = tempfile::tempdir().expect("tempdir");
    let yaml_path = dir.path().join("openspine.yaml");
    std::fs::write(&yaml_path, OAUTH_CONFIG).expect("write");

    update_openspine_yaml_roles(
        &yaml_path,
        "anthropic",
        ProviderKind::Anthropic,
        "claude-sonnet-4-6",
    )
    .expect("update roles");

    let config = Config::load(&yaml_path).expect("reload");
    let added = config
        .providers
        .iter()
        .find(|provider| provider.id == "anthropic")
        .expect("provider added");
    assert_eq!(added.model, "claude-sonnet-4-6");
    assert_eq!(added.auth, ProviderAuth::Oauth);
    assert_eq!(config.providers.len(), 2);
}

fn oauth_config(base_url: &str) -> Config {
    let yaml = format!(
        "data_dir: d\nsandbox:\n  driver: process\nowner:\n  telegram_user_id: 1\n  \
         display_name: o\nspend_cap: {{}}\nproviders:\n  - id: anthropic\n    kind: anthropic\n    \
         base_url: {base_url}\n    model: m\n    auth:\n      mode: oauth\n"
    );
    serde_yaml::from_str(&yaml).expect("config")
}

/// For an OAuth provider the resolved key is only the `oauth:<id>` sentinel: the
/// bearer actually sent is the vault's access token. A gateway error quotes the
/// provider response verbatim, so a provider that echoes the bearer would put it
/// straight into a report the owner may paste into an issue.
#[tokio::test]
async fn a_probe_failure_never_echoes_a_vault_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(401).set_body_string(
            "{\"error\":\"invalid bearer sk-access-SECRET-VALUE, refresh rt-SECRET-VALUE\"}",
        ))
        .mount(&server)
        .await;
    let (_dir, store) = vault("credentials");
    store
        .store_oauth_tokens(
            "anthropic",
            "rt-SECRET-VALUE",
            "sk-access-SECRET-VALUE",
            "9999999999",
            None,
        )
        .expect("seed");

    let check = verify_default_provider(&oauth_config(&server.uri()), Some(&store)).await;

    assert_eq!(check.state, crate::cli::readiness::CheckState::Fail);
    assert!(
        !check.detail.contains("sk-access-SECRET-VALUE"),
        "{}",
        check.detail
    );
    assert!(
        !check.detail.contains("rt-SECRET-VALUE"),
        "{}",
        check.detail
    );
    assert!(check.detail.contains("<redacted>"), "{}", check.detail);
}

/// The starter configuration's local key is the literal `local`. A length floor
/// on redaction would print it verbatim.
#[tokio::test]
async fn a_probe_failure_redacts_even_a_short_api_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad key: shortkey"))
        .mount(&server)
        .await;
    let (_dir, store) = vault("credentials");
    std::env::set_var("OPENSPINE_TEST_SHORT_KEY", "shortkey");
    let yaml = format!(
        "data_dir: d\nsandbox:\n  driver: process\nowner:\n  telegram_user_id: 1\n  \
         display_name: o\nspend_cap: {{}}\nproviders:\n  - id: local\n    kind: openai_compat\n    \
         base_url: {}\n    model: m\n    auth:\n      mode: api_key\n      env: \
         OPENSPINE_TEST_SHORT_KEY\n",
        server.uri()
    );
    let config: Config = serde_yaml::from_str(&yaml).expect("config");

    let check = verify_default_provider(&config, Some(&store)).await;

    std::env::remove_var("OPENSPINE_TEST_SHORT_KEY");
    assert_eq!(check.state, crate::cli::readiness::CheckState::Fail);
    assert!(!check.detail.contains("shortkey"), "{}", check.detail);
}

/// Anthropic's manual paste hands back `<code>#<state>`, which is what an owner
/// copies out of the browser when no loopback listener is reachable. That is the
/// primary path for an SSH install, so the split is not a nicety.
#[tokio::test]
async fn a_pasted_code_carrying_its_state_is_split_and_both_are_sent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "access-value",
            "refresh_token": "refresh-value",
            "expires_in": 3600
        })))
        .mount(&server)
        .await;
    let (_dir, store) = vault("credentials");
    let auth = begin_with_client_id("anthropic", 54545, TEST_CLIENT_ID).expect("begin");

    finish(
        &auth,
        "the-code#state-from-browser",
        &store,
        &reqwest::Client::new(),
        Some(&format!("{}/token", server.uri())),
    )
    .await
    .expect("finish");

    let request = &server.received_requests().await.expect("requests")[0];
    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json body");
    assert_eq!(body["code"].as_str(), Some("the-code"));
    assert_eq!(body["state"].as_str(), Some("state-from-browser"));
    // JSON, not form encoding: the token endpoint rejects a form body.
    assert_eq!(
        request
            .headers
            .get("content-type")
            .map(|v| v.to_str().unwrap()),
        Some("application/json")
    );
}

/// A code with no `#` keeps the state the authorization generated.
#[tokio::test]
async fn a_bare_pasted_code_uses_the_authorization_state() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "a",
            "refresh_token": "r",
            "expires_in": 3600
        })))
        .mount(&server)
        .await;
    let (_dir, store) = vault("credentials");
    let auth = begin_with_client_id("anthropic", 54545, TEST_CLIENT_ID).expect("begin");
    let expected_state = auth.state().to_string();

    finish(
        &auth,
        "plain-code",
        &store,
        &reqwest::Client::new(),
        Some(&format!("{}/token", server.uri())),
    )
    .await
    .expect("finish");

    let request = &server.received_requests().await.expect("requests")[0];
    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json body");
    assert_eq!(body["code"].as_str(), Some("plain-code"));
    assert_eq!(body["state"].as_str(), Some(expected_state.as_str()));
}

/// `code=true` and `user:inference` are the two parameters that decide whether
/// the grant can serve model calls at all.
#[test]
fn the_authorization_url_requests_an_inference_capable_grant() {
    let auth = begin_with_client_id("anthropic", 54545, TEST_CLIENT_ID).expect("begin");

    assert!(auth.url.contains("code=true"), "{}", auth.url);
    assert!(auth.url.contains("user%3Ainference"), "{}", auth.url);
    assert!(
        auth.url.starts_with("https://claude.ai/oauth/authorize"),
        "{}",
        auth.url
    );
}

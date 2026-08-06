//! Provider login flow tests: headless detection, kind mapping, legacy
//! transport cutover, stored-credential re-binding, and identity
//! backfill. Split from `login.rs` for the 500-line module gate.

use super::*;

/// An SSH session must take the printed-URL path: binding a loopback
/// listener the owner's browser cannot reach would hang the login.
#[test]
fn an_ssh_session_is_treated_as_headless() {
    let restore = std::env::var_os("SSH_CONNECTION");
    std::env::set_var("SSH_CONNECTION", "10.0.0.1 22 10.0.0.2 22");

    let headless = headless();

    match restore {
        Some(value) => std::env::set_var("SSH_CONNECTION", value),
        None => std::env::remove_var("SSH_CONNECTION"),
    }
    assert!(headless);
}

#[test]
fn every_supported_provider_maps_to_a_gateway_kind() {
    let dir = std::env::temp_dir().join(format!("openspine-wizard-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("openspine.yaml");
    std::fs::write(
        &config_path,
        "data_dir: d\nsandbox:\n  driver: process\nowner:\n  telegram_user_id: 1\n  \
             display_name: o\nspend_cap: {}\nproviders:\n  - id: anthropic\n    kind: anthropic\n \
             \x20  model: m\n    auth:\n      mode: oauth\n",
    )
    .unwrap();

    let (provider, kind) = provider_entry(&config_path, "anthropic").unwrap();

    assert_eq!(provider.model, "m");
    assert_eq!(kind, ProviderKind::Anthropic);
}

#[test]
fn an_unsupported_provider_is_refused_before_prompting() {
    let error = provider_entry(Path::new("/nonexistent.yaml"), "not-a-provider").unwrap_err();
    assert!(error.to_string().contains("unsupported provider"));
}

#[test]
fn codex_maps_to_the_responses_transport_kind() {
    let dir = std::env::temp_dir().join(format!("openspine-wizard-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("openspine.yaml");
    std::fs::write(
        &config_path,
        "data_dir: d\nsandbox:\n  driver: process\nowner:\n  telegram_user_id: 1\n  \
             display_name: o\nspend_cap: {}\nproviders:\n  - id: openai-codex\n    kind: \
             openai_codex\n    model: gpt-5-codex\n    auth:\n      mode: oauth\n",
    )
    .unwrap();

    let (provider, kind) = provider_entry(&config_path, "openai-codex").unwrap();

    assert_eq!(provider.model, "gpt-5-codex");
    assert_eq!(kind, ProviderKind::OpenaiCodex);
}

/// A configuration written by an older build carries `openai-codex` as
/// `openai_compat` with a chat-completions endpoint. The id namespace is
/// kernel-defined, so the entry is cut over to the Responses transport —
/// and the old kind's endpoint goes with it: the new transport's bearer
/// and account header must never reach a host configured for a
/// different wire contract.
#[test]
fn a_legacy_compat_codex_entry_is_cut_over_to_the_canonical_transport() {
    let dir = std::env::temp_dir().join(format!("openspine-wizard-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("openspine.yaml");
    std::fs::write(
        &config_path,
        "data_dir: d\nsandbox:\n  driver: process\nowner:\n  telegram_user_id: 1\n  \
             display_name: o\nspend_cap: {}\nproviders:\n  - id: openai-codex\n    kind: \
             openai_compat\n    base_url: http://127.0.0.1:9/legacy\n    model: gpt-5-codex\n    \
             auth:\n      mode: oauth\n",
    )
    .unwrap();

    let (provider, kind) = provider_entry(&config_path, "openai-codex").unwrap();
    assert_eq!(kind, ProviderKind::OpenaiCodex);
    assert_eq!(provider.kind, ProviderKind::OpenaiCodex);
    assert_eq!(
        provider.base_url, None,
        "the legacy transport's endpoint must not survive the cutover"
    );

    // The binding write applies the same cutover.
    setup::update_openspine_yaml_roles(
        &config_path,
        "openai-codex",
        ProviderKind::OpenaiCodex,
        "gpt-5-codex",
    )
    .unwrap();
    let bound = Config::load(&config_path).unwrap();
    assert_eq!(bound.providers[0].kind, ProviderKind::OpenaiCodex);
    assert_eq!(bound.providers[0].base_url, None);
}

/// Switching between held subscriptions: with credentials stored for
/// BOTH providers and Codex currently routed, `provider login anthropic`
/// re-verifies and promotes Anthropic with no authorization round trip
/// (and therefore no prompt, no browser, no loopback listener, no token
/// exchange).
#[tokio::test]
async fn login_with_a_stored_credential_rebinds_without_a_new_authorization() {
    use crate::secret_store::{OAuthIdentityMetadata, SecretStore};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let anthropic_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "content": [{"type": "text", "text": "pong"}]
        })))
        .expect(1)
        .mount(&anthropic_server)
        .await;
    let codex_server = MockServer::start().await;
    // No mock mounted on the codex server: any request there would 404
    // and fail the login, and the assertion below counts its requests.

    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("openspine.yaml");
    std::fs::write(
        &config_path,
        format!(
            "data_dir: d\nsandbox:\n  driver: process\nowner:\n  telegram_user_id: 1\n  \
                 display_name: o\nspend_cap: {{}}\nproviders:\n  - id: openai-codex\n    kind: \
                 openai_codex\n    base_url: {}\n    model: gpt-5-codex\n    auth:\n      mode: \
                 oauth\n  - id: anthropic\n    kind: anthropic\n    base_url: {}\n    model: \
                 claude-sonnet-4-6\n    auth:\n      mode: oauth\n",
            codex_server.uri(),
            anthropic_server.uri()
        ),
    )
    .unwrap();

    let store = SecretStore::open(dir.path().join("credentials"), [25; 32]).expect("open vault");
    store
        .store_oauth_tokens(
            "openai-codex",
            "codex-refresh",
            "codex-access",
            "9999999999",
            Some(OAuthIdentityMetadata {
                account_email: None,
                account_id: Some("acct-777".to_string()),
                identity_key: None,
            }),
        )
        .expect("store codex oauth");
    store
        .store_oauth_tokens(
            "anthropic",
            "anthropic-refresh",
            "anthropic-access",
            "9999999999",
            None,
        )
        .expect("store anthropic oauth");

    login_flow(
        &config_path,
        Some(&store),
        &reqwest::Client::new(),
        Some("anthropic"),
        false,
    )
    .await
    .expect("switch to the other stored credential");

    // The verification ping hit Anthropic exactly once (expect(1)),
    // nothing touched the Codex endpoint, and Anthropic was promoted to
    // the routed (first) position with OAuth mode.
    assert!(
        codex_server.received_requests().await.unwrap().is_empty(),
        "switching must not touch the previously routed provider"
    );
    let bound = Config::load(&config_path).expect("reload config");
    assert_eq!(bound.providers[0].id, "anthropic");
    assert_eq!(bound.providers[0].auth, ProviderAuth::Oauth);
    assert_eq!(bound.providers[1].id, "openai-codex");
}

/// An older build's Codex login stored tokens without the account id.
/// The rebind path backfills it from the stored access token's claim
/// instead of dead-ending on a credential verification can never spend.
#[tokio::test]
async fn rebind_backfills_a_missing_codex_account_id_from_the_stored_token() {
    use crate::secret_store::SecretStore;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let header_b64 = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
    let payload_b64 = URL_SAFE_NO_PAD.encode(
        serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": "acct-backfill" }
        })
        .to_string()
        .as_bytes(),
    );
    let stored_access = format!("{header_b64}.{payload_b64}.sig");

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .and(header("chatgpt-account-id", "acct-backfill"))
        .respond_with(ResponseTemplate::new(200).set_body_string(concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"pong\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n",
        )))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("openspine.yaml");
    std::fs::write(
        &config_path,
        format!(
            "data_dir: d\nsandbox:\n  driver: process\nowner:\n  telegram_user_id: 1\n  \
                 display_name: o\nspend_cap: {{}}\nproviders:\n  - id: openai-codex\n    kind: \
                 openai_codex\n    base_url: {}\n    model: gpt-5-codex\n    auth:\n      mode: \
                 oauth\n",
            server.uri()
        ),
    )
    .unwrap();

    let store = SecretStore::open(dir.path().join("credentials"), [26; 32]).expect("open vault");
    store
        .store_oauth_tokens(
            "openai-codex",
            "refresh-1",
            &stored_access,
            "9999999999",
            None,
        )
        .expect("store identity-less oauth");

    login_flow(
        &config_path,
        Some(&store),
        &reqwest::Client::new(),
        Some("openai-codex"),
        false,
    )
    .await
    .expect("rebind with backfilled identity");

    let tokens = store
        .get_oauth_tokens("openai-codex")
        .unwrap()
        .expect("tokens");
    assert_eq!(
        tokens.account_id.as_deref(),
        Some("acct-backfill"),
        "the identity must be backfilled into the vault"
    );
}

/// A stored credential must not outflank the spendability refusal: an
/// unsupported provider is refused before the rebind shortcut can
/// verify-and-bind a transport this build cannot serve.
#[tokio::test]
async fn a_stored_credential_for_an_unsupported_provider_is_still_refused() {
    use crate::secret_store::SecretStore;

    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("openspine.yaml");
    std::fs::write(
        &config_path,
        "data_dir: d\nsandbox:\n  driver: process\nowner:\n  telegram_user_id: 1\n  \
         display_name: o\nspend_cap: {}\nproviders:\n  - id: google-antigravity\n    kind: \
         google_antigravity\n    model: gemini-2.5-flash\n    auth:\n      mode: oauth\n",
    )
    .unwrap();
    let store = SecretStore::open(dir.path().join("credentials"), [27; 32]).expect("open vault");
    store
        .store_oauth_tokens(
            "google-antigravity",
            "refresh-1",
            "access-1",
            "9999999999",
            None,
        )
        .expect("store oauth");

    let error = login_flow(
        &config_path,
        Some(&store),
        &reqwest::Client::new(),
        Some("google-antigravity"),
        false,
    )
    .await
    .expect_err("must refuse before any verification");

    assert!(
        error.to_string().contains("not available in this build"),
        "{error}"
    );
}

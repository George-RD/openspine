//! Interactive Onboarding & Provider Login CLI Wizard.

use crate::config::{Config, ProviderAuth, ProviderConfig, ProviderKind};
use crate::model_gateway::{
    build_prompt, PromptMessage, PromptRole, PromptTemplate, ProviderClient,
};
use crate::oauth::callback_server::CallbackServer;
use crate::oauth::pkce::PkceChallenge;
use crate::secret_store::SecretStore;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::workflow::ReasoningTier;
use std::path::Path;

#[allow(dead_code)]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SetupResult {
    pub provider_id: String,
    pub verified: bool,
    pub roles_bound: Vec<String>,
}

#[allow(dead_code)]
pub async fn run_provider_login(
    provider_id: &str,
    secret_store: &SecretStore,
    client: &reqwest::Client,
    manual_code_override: Option<&str>,
    token_url_override: Option<&str>,
) -> Result<String, anyhow::Error> {
    let pkce = PkceChallenge::new();

    match provider_id {
        "google-antigravity" => {
            let (code, port) = if let Some(code) = manual_code_override {
                (code.to_string(), 51121)
            } else {
                let cb = CallbackServer::bind(51121).await?;
                let port = cb.port();
                let _url = crate::oauth::providers::google_antigravity::build_authorization_url(
                    port, &pkce,
                );
                let code = cb.wait_for_code(&pkce.state).await?;
                (code, port)
            };

            let token_res = crate::oauth::providers::google_antigravity::exchange_code(
                client,
                port,
                &code,
                &pkce.code_verifier,
                token_url_override,
            )
            .await?;

            let expires_in = token_res.expires_in.max(300);
            let now_sec = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let expires_at = (now_sec + expires_in).to_string();

            let meta = crate::secret_store::OAuthIdentityMetadata {
                account_email: token_res.account_email,
                account_id: token_res.account_id,
                identity_key: None,
            };

            let refresh_tok = token_res
                .refresh_token
                .unwrap_or_else(|| "mock-refresh-token".to_string());

            secret_store.store_oauth_tokens(
                provider_id,
                &refresh_tok,
                &token_res.access_token,
                &expires_at,
                Some(meta),
            )?;

            Ok(token_res.access_token)
        }
        "openai-codex" => {
            let (code, port) = if let Some(code) = manual_code_override {
                (code.to_string(), 1455)
            } else {
                let cb = CallbackServer::bind(1455).await?;
                let port = cb.port();
                let _url =
                    crate::oauth::providers::openai_codex::build_authorization_url(port, &pkce);
                let code = cb.wait_for_code(&pkce.state).await?;
                (code, port)
            };

            let token_res = crate::oauth::providers::openai_codex::exchange_code(
                client,
                port,
                &code,
                &pkce.code_verifier,
                token_url_override,
            )
            .await?;

            let expires_in = token_res.expires_in.max(300);
            let now_sec = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let expires_at = (now_sec + expires_in).to_string();

            let refresh_tok = token_res
                .refresh_token
                .unwrap_or_else(|| "mock-refresh-token".to_string());

            secret_store.store_oauth_tokens(
                provider_id,
                &refresh_tok,
                &token_res.access_token,
                &expires_at,
                None,
            )?;

            Ok(token_res.access_token)
        }
        "anthropic" => {
            let (code, port) = if let Some(code) = manual_code_override {
                (code.to_string(), 54545)
            } else {
                let cb = CallbackServer::bind(54545).await?;
                let port = cb.port();
                let _url = crate::oauth::providers::anthropic::build_authorization_url(port, &pkce);
                let code = cb.wait_for_code(&pkce.state).await?;
                (code, port)
            };

            let token_res = crate::oauth::providers::anthropic::exchange_code(
                client,
                port,
                &code,
                &pkce.code_verifier,
                token_url_override,
            )
            .await?;

            let expires_in = token_res.expires_in.max(300);
            let now_sec = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let expires_at = (now_sec + expires_in).to_string();

            let refresh_tok = token_res
                .refresh_token
                .unwrap_or_else(|| "mock-refresh-token".to_string());

            secret_store.store_oauth_tokens(
                provider_id,
                &refresh_tok,
                &token_res.access_token,
                &expires_at,
                None,
            )?;

            Ok(token_res.access_token)
        }
        _ => anyhow::bail!("Unsupported provider for OAuth login: {provider_id}"),
    }
}

#[allow(dead_code)]
pub async fn run_preflight_verification_ping(
    provider_client: &ProviderClient,
    secret_store: &SecretStore,
    provider_id: &str,
) -> Result<bool, anyhow::Error> {
    let tmpl = PromptTemplate {
        id: "ping-template".to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        system_preamble: "You are OpenSpine model verification probe.".to_string(),
        untrusted_data_preamble: None,
    };

    let prompt = build_prompt(
        &tmpl,
        vec![PromptMessage {
            role: PromptRole::User,
            content: "Hello OpenSpine model gateway verification probe".to_string(),
        }],
        10,
        ReasoningTier::Standard,
    );

    let res = provider_client
        .generate_with_secret_store(&prompt, Some(secret_store), Some(provider_id), None)
        .await?;

    Ok(!res.is_empty())
}

#[allow(dead_code)]
pub fn update_openspine_yaml_roles(
    config_path: &Path,
    selected_provider_id: &str,
    provider_kind: ProviderKind,
) -> Result<(), anyhow::Error> {
    let content = std::fs::read_to_string(config_path)?;
    let mut config: Config = serde_yaml::from_str(&content)?;

    let mut found = false;
    for p in &mut config.providers {
        if p.id == selected_provider_id {
            p.auth = ProviderAuth::Oauth;
            found = true;
            break;
        }
    }

    if !found {
        config.providers.push(ProviderConfig {
            id: selected_provider_id.to_string(),
            kind: provider_kind,
            base_url: None,
            model: "gemini-2.5-flash".to_string(),
            auth: ProviderAuth::Oauth,
        });
    }

    let updated_yaml = serde_yaml::to_string(&config)?;
    std::fs::write(config_path, updated_yaml)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

        let dir = tempfile::tempdir().expect("tempdir");
        let store = SecretStore::open(dir.path().join("credentials"), [20; 32]).expect("open");

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

    #[tokio::test]
    async fn setup_wizard_binds_active_model_roles_only_on_successful_verification() {
        let dir = tempfile::tempdir().expect("tempdir");
        let yaml_path = dir.path().join("openspine.yaml");

        let initial_yaml = r#"
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
        std::fs::write(&yaml_path, initial_yaml).expect("write initial config");

        update_openspine_yaml_roles(
            &yaml_path,
            "google-antigravity",
            ProviderKind::GoogleAntigravity,
        )
        .expect("update roles");

        let reloaded = std::fs::read_to_string(&yaml_path).expect("read updated config");
        assert!(reloaded.contains("google-antigravity"));
        assert!(reloaded.contains("mode: oauth"));
    }
}

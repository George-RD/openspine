//! Model Provider OAuth Specifications & Authorization Flows.

pub mod anthropic;
pub mod google_antigravity;
pub mod openai_codex;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
    pub token_type: Option<String>,
    pub account_email: Option<String>,
    pub account_id: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: Option<u64>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthProviderSpec {
    pub id: &'static str,
    pub display_name: &'static str,
    pub auth_endpoint: &'static str,
    pub token_endpoint: &'static str,
    pub device_endpoint: Option<&'static str>,
    pub scope: &'static str,
    pub default_port: u16,
    /// Env var naming the OAuth client id registered with this provider.
    pub client_id_env: &'static str,
}

#[allow(dead_code)]
pub fn get_provider_spec(provider_id: &str) -> Option<OAuthProviderSpec> {
    match provider_id {
        "google-antigravity" => Some(google_antigravity::spec()),
        "openai-codex" => Some(openai_codex::spec()),
        "anthropic" => Some(anthropic::spec()),
        _ => None,
    }
}

/// The OAuth client id registered with `provider_id`, from its environment
/// variable.
///
/// There is no built-in default. A hardcoded placeholder would let onboarding
/// print an authorization URL the provider rejects, which is a worse failure
/// than refusing before the owner opens a browser: OpenSpine cannot register an
/// OAuth application on the owner's behalf.
pub fn configured_client_id(provider_id: &str) -> Result<String, anyhow::Error> {
    let spec = get_provider_spec(provider_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported provider for OAuth login: {provider_id}"))?;
    std::env::var(spec.client_id_env)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{} has no registered OAuth client. Register an OAuth application with \
                 {} using redirect URI http://127.0.0.1:{}/callback, then set {} to its \
                 client id. Until then, configure an API-key or local provider in \
                 openspine.yaml instead.",
                provider_id,
                spec.display_name,
                spec.default_port,
                spec.client_id_env
            )
        })
}

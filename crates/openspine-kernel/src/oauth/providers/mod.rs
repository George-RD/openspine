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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthProviderSpec {
    pub display_name: &'static str,
    pub auth_endpoint: &'static str,
    pub token_endpoint: &'static str,
    pub scope: &'static str,
    pub default_port: u16,
    /// The provider's public OAuth client id.
    ///
    /// These are the first-party client ids the vendors' own CLIs present.
    /// Neither Anthropic nor OpenAI offers self-service registration for
    /// subscription OAuth, so a client id an owner could supply does not exist;
    /// the public one is the only value that authorizes. PKCE public clients
    /// carry no secret, so nothing confidential is embedded here.
    pub client_id: &'static str,
    /// Whether this build can serve inference on the resulting credential.
    ///
    /// A login that stores a working credential the gateway cannot use is the
    /// same dead end as a placeholder client id, reached one step later.
    pub login_supported: bool,
}

pub fn get_provider_spec(provider_id: &str) -> Option<OAuthProviderSpec> {
    match provider_id {
        "google-antigravity" => Some(google_antigravity::spec()),
        "openai-codex" => Some(openai_codex::spec()),
        "anthropic" => Some(anthropic::spec()),
        _ => None,
    }
}

/// The OAuth client id to present for `provider_id`.
///
/// Refuses a provider whose credential this build cannot spend. Antigravity
/// authorizes fine, but its grant needs a provider transport the gateway does
/// not implement; offering that login would store a credential no request
/// could use. Anthropic (Messages API) and OpenAI Codex (ChatGPT backend
/// Responses transport) both have serving transports.
pub fn client_id_for(provider_id: &str) -> Result<&'static str, anyhow::Error> {
    let spec = get_provider_spec(provider_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported provider for OAuth login: {provider_id}"))?;
    if !spec.login_supported {
        anyhow::bail!(
            "{} OAuth login is not available in this build: its credential needs a provider \
             transport OpenSpine does not implement yet. Configure an API-key or local \
             provider in openspine.yaml instead.",
            spec.display_name
        );
    }
    Ok(spec.client_id)
}

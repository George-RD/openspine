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
    pub client_id: &'static str,
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

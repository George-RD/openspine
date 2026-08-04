//! Google Antigravity OAuth integration.

use super::{OAuthProviderSpec, TokenResponse};
use crate::oauth::pkce::PkceChallenge;
use std::collections::HashMap;

#[allow(dead_code)]
pub fn spec() -> OAuthProviderSpec {
    OAuthProviderSpec {
        id: "google-antigravity",
        display_name: "Google Antigravity",
        auth_endpoint: "https://accounts.google.com/o/oauth2/v2/auth",
        token_endpoint: "https://oauth2.googleapis.com/token",
        device_endpoint: None,
        scope: "https://www.googleapis.com/auth/cloud-platform email",
        default_port: 51121,
        client_id_env: "OPENSPINE_ANTIGRAVITY_CLIENT_ID",
    }
}

#[allow(dead_code)]
pub(crate) fn url_encode(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    encoded
}

#[allow(dead_code)]
pub fn build_authorization_url(
    redirect_port: u16,
    pkce: &PkceChallenge,
    client_id: &str,
) -> String {
    let s = spec();
    let redirect_uri = format!("http://127.0.0.1:{redirect_port}/callback");
    format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method={}",
        s.auth_endpoint,
        url_encode(client_id),
        url_encode(&redirect_uri),
        url_encode(s.scope),
        url_encode(&pkce.state),
        url_encode(&pkce.code_challenge),
        pkce.code_challenge_method,
    )
}

#[allow(dead_code)]
pub async fn exchange_code(
    client: &reqwest::Client,
    client_id: &str,
    redirect_port: u16,
    code: &str,
    code_verifier: &str,
    token_url_override: Option<&str>,
) -> Result<TokenResponse, anyhow::Error> {
    let s = spec();
    let url = token_url_override.unwrap_or(s.token_endpoint);
    let redirect_uri = format!("http://127.0.0.1:{redirect_port}/callback");

    let mut params = HashMap::new();
    params.insert("grant_type", "authorization_code");
    params.insert("client_id", client_id);
    params.insert("code", code);
    params.insert("redirect_uri", &redirect_uri);
    params.insert("code_verifier", code_verifier);

    let res = client.post(url).form(&params).send().await?;
    if !res.status().is_success() {
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("Google token exchange failed: {text}");
    }

    let token_res: TokenResponse = res.json().await?;
    Ok(token_res)
}

pub async fn refresh_token(
    client: &reqwest::Client,
    client_id: &str,
    refresh_token: &str,
    token_url_override: Option<&str>,
) -> Result<TokenResponse, anyhow::Error> {
    let s = spec();
    let url = token_url_override.unwrap_or(s.token_endpoint);

    let mut params = HashMap::new();
    params.insert("grant_type", "refresh_token");
    params.insert("client_id", client_id);
    params.insert("refresh_token", refresh_token);

    let res = client.post(url).form(&params).send().await?;
    if !res.status().is_success() {
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("Google token refresh failed: {text}");
    }

    let token_res: TokenResponse = res.json().await?;
    Ok(token_res)
}

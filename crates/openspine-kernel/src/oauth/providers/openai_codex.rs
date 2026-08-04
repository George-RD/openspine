//! OpenAI Codex / ChatGPT OAuth & Device Code Flow integration.

use super::google_antigravity::url_encode;
use super::{DeviceCodeResponse, OAuthProviderSpec, TokenResponse};
use crate::oauth::pkce::PkceChallenge;
use std::collections::HashMap;

#[allow(dead_code)]
pub fn spec() -> OAuthProviderSpec {
    OAuthProviderSpec {
        id: "openai-codex",
        display_name: "OpenAI Codex",
        auth_endpoint: "https://auth.openai.com/authorize",
        token_endpoint: "https://auth.openai.com/oauth/token",
        device_endpoint: Some("https://auth.openai.com/oauth/device/code"),
        scope: "openid profile email offline_access",
        default_port: 1455,
        client_id_env: "OPENSPINE_CODEX_CLIENT_ID",
    }
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
        anyhow::bail!("OpenAI Codex token exchange failed: {text}");
    }

    let token_res: TokenResponse = res.json().await?;
    Ok(token_res)
}

#[allow(dead_code)]
pub async fn request_device_code(
    client: &reqwest::Client,
    client_id: &str,
    device_url_override: Option<&str>,
) -> Result<DeviceCodeResponse, anyhow::Error> {
    let s = spec();
    let url = device_url_override.unwrap_or(s.device_endpoint.unwrap_or_default());

    let mut params = HashMap::new();
    params.insert("client_id", client_id);
    params.insert("scope", s.scope);

    let res = client.post(url).form(&params).send().await?;
    if !res.status().is_success() {
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI Codex device code request failed: {text}");
    }

    let dev_res: DeviceCodeResponse = res.json().await?;
    Ok(dev_res)
}

#[allow(dead_code)]
pub async fn poll_device_token(
    client: &reqwest::Client,
    client_id: &str,
    device_code: &str,
    token_url_override: Option<&str>,
) -> Result<TokenResponse, anyhow::Error> {
    let s = spec();
    let url = token_url_override.unwrap_or(s.token_endpoint);

    let mut params = HashMap::new();
    params.insert("grant_type", "urn:ietf:params:oauth:grant-type:device_code");
    params.insert("client_id", client_id);
    params.insert("device_code", device_code);

    let res = client.post(url).form(&params).send().await?;
    if !res.status().is_success() {
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("authorization_pending: {text}");
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
        anyhow::bail!("OpenAI Codex token refresh failed: {text}");
    }

    let token_res: TokenResponse = res.json().await?;
    Ok(token_res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn oauth_device_code_flow_polls_until_token_granted() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/oauth/device/code"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "device_code": "dev-code-123",
                "user_code": "WD-40",
                "verification_uri": "https://auth.openai.com/activate",
                "expires_in": 300,
                "interval": 1
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "codex-access-token-99",
                "refresh_token": "codex-refresh-token-99",
                "expires_in": 3600,
                "token_type": "Bearer"
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let dev_url = format!("{}/oauth/device/code", server.uri());
        let token_url = format!("{}/oauth/token", server.uri());

        let dev_res = request_device_code(&http, "test-client-id", Some(&dev_url))
            .await
            .expect("request dev code");
        assert_eq!(dev_res.device_code, "dev-code-123");
        assert_eq!(dev_res.user_code, "WD-40");

        let token_res = poll_device_token(
            &http,
            "test-client-id",
            &dev_res.device_code,
            Some(&token_url),
        )
        .await
        .expect("poll token");
        assert_eq!(token_res.access_token, "codex-access-token-99");
        assert_eq!(
            token_res.refresh_token.as_deref(),
            Some("codex-refresh-token-99")
        );
    }
}

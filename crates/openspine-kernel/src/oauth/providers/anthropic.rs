//! Anthropic Claude OAuth integration.

use super::google_antigravity::url_encode;
use super::{OAuthProviderSpec, TokenResponse};
use crate::oauth::pkce::PkceChallenge;
use std::collections::HashMap;

#[allow(dead_code)]
pub fn spec() -> OAuthProviderSpec {
    OAuthProviderSpec {
        id: "anthropic",
        display_name: "Anthropic Claude",
        auth_endpoint: "https://claude.ai/oauth/authorize",
        token_endpoint: "https://api.anthropic.com/v1/oauth/token",
        device_endpoint: None,
        scope: "org:read user:read",
        default_port: 54545,
        client_id_env: "OPENSPINE_ANTHROPIC_CLIENT_ID",
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
        anyhow::bail!("Anthropic token exchange failed: {text}");
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
        anyhow::bail!("Anthropic token refresh failed: {text}");
    }

    let token_res: TokenResponse = res.json().await?;
    Ok(token_res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn oauth_login_fallback_accepts_manual_authorization_code_input() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/oauth/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "manual-pasted-access-token-123",
                "refresh_token": "manual-pasted-refresh-token-123",
                "expires_in": 3600,
                "token_type": "Bearer",
                "account_email": "claude-user@example.com"
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let manual_pasted_code = "manual-auth-code-xyz-999";
        let verifier = "test-pkce-verifier-string-32-bytes";
        let token_url = format!("{}/v1/oauth/token", server.uri());

        let res = exchange_code(
            &http,
            "test-client-id",
            54545,
            manual_pasted_code,
            verifier,
            Some(&token_url),
        )
        .await
        .expect("exchange manual code");

        assert_eq!(res.access_token, "manual-pasted-access-token-123");
        assert_eq!(
            res.refresh_token.as_deref(),
            Some("manual-pasted-refresh-token-123")
        );
        assert_eq!(
            res.account_email.as_deref(),
            Some("claude-user@example.com")
        );
    }
}

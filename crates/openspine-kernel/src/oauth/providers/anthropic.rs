//! Anthropic Claude OAuth integration.

use super::google_antigravity::url_encode;
use super::{OAuthProviderSpec, TokenResponse};
use crate::oauth::pkce::PkceChallenge;
use std::collections::HashMap;

/// Claude Code's public OAuth client id.
///
/// Anthropic offers no self-service registration for subscription OAuth, so
/// this first-party id is the only value the authorize endpoint accepts. A PKCE
/// public client holds no secret: this identifies the client rather than
/// authenticating it.
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

#[allow(dead_code)]
pub fn spec() -> OAuthProviderSpec {
    OAuthProviderSpec {
        id: "anthropic",
        display_name: "Anthropic Claude",
        auth_endpoint: "https://claude.ai/oauth/authorize",
        token_endpoint: "https://api.anthropic.com/v1/oauth/token",
        device_endpoint: None,
        // `user:inference` is load bearing: without it the grant cannot serve
        // model calls at all. `platform.claude.com/oauth/authorize` issues
        // console tokens carrying only `org:create_api_key` and never grants
        // inference, which is why the authorize endpoint above is `claude.ai`.
        scope: "org:create_api_key user:profile user:inference \
                user:sessions:claude_code user:mcp_servers user:file_upload",
        default_port: 54545,
        client_id: CLIENT_ID,
        login_supported: true,
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
    // `code=true` selects the flow that renders a pasteable `<code>#<state>`,
    // which is what makes the headless path work.
    format!(
        "{}?code=true&response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method={}",
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
    state: &str,
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
    // The token endpoint requires the authorization `state` alongside the code.
    params.insert("state", state);
    params.insert("redirect_uri", &redirect_uri);
    params.insert("code_verifier", code_verifier);

    // JSON, not form encoding: the Anthropic token endpoint rejects a
    // form-encoded body.
    let res = client.post(url).json(&params).send().await?;
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

    // The first-party client sends the OAuth beta and its SDK agent when
    // refreshing, and neither on the initial code exchange. A refresh without
    // them is what turns a working login into a disabled credential at the
    // first renewal.
    let res = client
        .post(url)
        .header("anthropic-beta", crate::anthropic_fingerprint::OAUTH_BETA)
        .header(
            "user-agent",
            crate::anthropic_fingerprint::OAUTH_REFRESH_USER_AGENT,
        )
        .json(&params)
        .send()
        .await?;
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
            "test-state",
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

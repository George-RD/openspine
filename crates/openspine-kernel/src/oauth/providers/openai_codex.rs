//! OpenAI Codex / ChatGPT OAuth integration.
//!
//! The registered public client (`app_EMoamEEZ73f0CkXaXp7hrann`, the id the
//! first-party Codex CLI presents) fixes the whole authorization surface:
//! `/oauth/authorize` on `auth.openai.com`, a redirect of exactly
//! `http://localhost:1455/auth/callback`, and the simplified-flow query
//! parameters. Deviating from any of them draws a redirect-URI rejection
//! before the owner ever sees a consent screen.
//!
//! The resulting credential carries its own identity: the access token is a
//! JWT whose `https://api.openai.com/auth` claim names the
//! `chatgpt_account_id` that inference requests must echo in the
//! `chatgpt-account-id` header. A token without that claim could never be
//! spent, so the exchange fails closed rather than storing it.

use super::google_antigravity::url_encode;
use super::{OAuthProviderSpec, TokenResponse};
use crate::oauth::pkce::PkceChallenge;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use std::collections::HashMap;

/// The JWT claim namespace OpenAI stores account routing under.
const AUTH_CLAIM: &str = "https://api.openai.com/auth";

/// Client name presented in the authorize URL. pi ships `originator=pi`
/// against the same endpoints, so non-enumerated originators are accepted;
/// this is the honest value for this runtime.
const ORIGINATOR: &str = "openspine";

#[allow(dead_code)]
pub fn spec() -> OAuthProviderSpec {
    OAuthProviderSpec {
        display_name: "OpenAI Codex",
        auth_endpoint: "https://auth.openai.com/oauth/authorize",
        token_endpoint: "https://auth.openai.com/oauth/token",
        scope: "openid profile email offline_access",
        default_port: 1455,
        client_id: "app_EMoamEEZ73f0CkXaXp7hrann",
        login_supported: true,
    }
}

/// The redirect URI registered for the public Codex client. Host, path, and
/// port are all load bearing: `localhost` (not `127.0.0.1`), `/auth/callback`
/// (not `/callback`), port 1455 exactly.
pub fn redirect_uri(redirect_port: u16) -> String {
    format!("http://localhost:{redirect_port}/auth/callback")
}

pub fn build_authorization_url(
    redirect_port: u16,
    pkce: &PkceChallenge,
    client_id: &str,
) -> String {
    let s = spec();
    let redirect_uri = redirect_uri(redirect_port);
    // `codex_cli_simplified_flow` selects the flow the registered client is
    // enrolled in; `id_token_add_organizations` matches the first-party
    // client so multi-org accounts resolve a workspace during consent.
    format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method={}&id_token_add_organizations=true&codex_cli_simplified_flow=true&originator={}",
        s.auth_endpoint,
        url_encode(client_id),
        url_encode(&redirect_uri),
        url_encode(s.scope),
        url_encode(&pkce.state),
        url_encode(&pkce.code_challenge),
        pkce.code_challenge_method,
        url_encode(ORIGINATOR),
    )
}

/// The `chatgpt_account_id` claim inside an access-token JWT, if present.
///
/// Decoded without signature verification: the value only routes requests
/// that are authenticated by the same token, so verifying the signature
/// would prove nothing the bearer check does not already prove.
pub fn access_token_account_id(access_token: &str) -> Option<String> {
    let payload_segment = access_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload_segment.trim_end_matches('='))
        .ok()?;
    let payload: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let account_id = payload.get(AUTH_CLAIM)?.get("chatgpt_account_id")?;
    account_id
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

/// Fill `account_id` from the access token, refusing a credential that
/// carries none: inference on this provider requires the
/// `chatgpt-account-id` header, so a token without the claim is a credential
/// no request could spend.
fn with_account_identity(mut tokens: TokenResponse) -> Result<TokenResponse, anyhow::Error> {
    let account_id = access_token_account_id(&tokens.access_token).ok_or_else(|| {
        anyhow::anyhow!(
            "the OpenAI access token carries no chatgpt_account_id claim, so its \
             credential could never serve inference; re-run the login with a \
             ChatGPT subscription account"
        )
    })?;
    tokens.account_id = Some(account_id);
    Ok(tokens)
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
    let redirect_uri = redirect_uri(redirect_port);

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
    with_account_identity(token_res)
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
    with_account_identity(token_res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// An unsigned JWT whose payload is `payload_json`, shaped like the real
    /// three-segment token.
    fn fake_jwt(payload_json: &serde_json::Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(payload_json.to_string().as_bytes());
        format!("{header}.{payload}.sig")
    }

    fn jwt_with_account(account_id: &str) -> String {
        fake_jwt(&serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": account_id }
        }))
    }

    #[test]
    fn codex_authorization_url_matches_registered_client_contract() {
        let pkce = PkceChallenge::new();
        let url = build_authorization_url(1455, &pkce, spec().client_id);

        assert!(
            url.starts_with("https://auth.openai.com/oauth/authorize?"),
            "authorize endpoint must carry the /oauth prefix: {url}"
        );
        assert!(
            url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback"),
            "redirect must be the registered localhost:1455/auth/callback: {url}"
        );
        assert!(url.contains("codex_cli_simplified_flow=true"), "{url}");
        assert!(url.contains("id_token_add_organizations=true"), "{url}");
        assert!(url.contains("originator=openspine"), "{url}");
        assert!(url.contains("code_challenge_method=S256"), "{url}");
        assert!(
            url.contains("scope=openid%20profile%20email%20offline_access"),
            "{url}"
        );
    }

    #[tokio::test]
    async fn codex_exchange_extracts_chatgpt_account_id_from_access_token() {
        let server = MockServer::start().await;
        let access = jwt_with_account("acct-777");

        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("grant_type=authorization_code"))
            .and(body_string_contains("code_verifier="))
            .and(body_string_contains(
                "redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": access,
                "refresh_token": "codex-refresh-1",
                "expires_in": 3600,
                "token_type": "Bearer"
            })))
            .mount(&server)
            .await;

        let token_url = format!("{}/oauth/token", server.uri());
        let tokens = exchange_code(
            &reqwest::Client::new(),
            "test-client-id",
            1455,
            "auth-code",
            "verifier",
            Some(&token_url),
        )
        .await
        .expect("exchange");

        assert_eq!(tokens.account_id.as_deref(), Some("acct-777"));
        assert_eq!(tokens.refresh_token.as_deref(), Some("codex-refresh-1"));
    }

    #[tokio::test]
    async fn codex_exchange_refuses_a_token_with_no_account_id() {
        let server = MockServer::start().await;
        let access = fake_jwt(&serde_json::json!({ "sub": "user-1" }));

        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": access,
                "refresh_token": "codex-refresh-2",
                "expires_in": 3600,
                "token_type": "Bearer"
            })))
            .mount(&server)
            .await;

        let token_url = format!("{}/oauth/token", server.uri());
        let error = exchange_code(
            &reqwest::Client::new(),
            "test-client-id",
            1455,
            "auth-code",
            "verifier",
            Some(&token_url),
        )
        .await
        .expect_err("a token with no account id must be refused");

        assert!(
            error.to_string().contains("chatgpt_account_id"),
            "error must name the missing claim: {error}"
        );
    }

    #[tokio::test]
    async fn codex_refresh_reissues_access_token_and_identity() {
        let server = MockServer::start().await;
        let access = jwt_with_account("acct-777");

        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("refresh_token=old-refresh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": access,
                "refresh_token": "new-refresh",
                "expires_in": 3600,
                "token_type": "Bearer"
            })))
            .mount(&server)
            .await;

        let token_url = format!("{}/oauth/token", server.uri());
        let tokens = refresh_token(
            &reqwest::Client::new(),
            "test-client-id",
            "old-refresh",
            Some(&token_url),
        )
        .await
        .expect("refresh");

        assert_eq!(tokens.account_id.as_deref(), Some("acct-777"));
        assert_eq!(tokens.refresh_token.as_deref(), Some("new-refresh"));
    }

    #[test]
    fn account_id_decoding_tolerates_padding_and_rejects_garbage() {
        assert_eq!(access_token_account_id("not-a-jwt"), None);
        assert_eq!(access_token_account_id("a.!!!.c"), None);
        let empty = fake_jwt(&serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": "" }
        }));
        assert_eq!(access_token_account_id(&empty), None);
        let ok = jwt_with_account("acct-1");
        assert_eq!(access_token_account_id(&ok).as_deref(), Some("acct-1"));
    }
}

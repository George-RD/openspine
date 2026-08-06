//! Model provider OAuth login, verification, and role binding.
//!
//! The authorization step is split into [`begin`] and [`finish`] so the caller
//! owns the interaction. That ordering matters: the loopback listener has to be
//! bound before the owner opens the authorization URL, and a headless owner
//! needs the URL printed rather than handed to a browser that does not exist.

use crate::cli::readiness::Check;
use crate::config::{Config, ProviderAuth, ProviderConfig, ProviderKind};
use crate::model_gateway::{
    build_prompt, PromptMessage, PromptRole, PromptTemplate, ProviderClient,
};
use crate::oauth::pkce::PkceChallenge;
use crate::oauth::providers::{anthropic, google_antigravity, openai_codex, TokenResponse};
use crate::secret_store::{OAuthIdentityMetadata, SecretStore};
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::workflow::ReasoningTier;
use std::path::Path;

/// The providers this build can log in to and then actually spend.
///
/// Anthropic serves through the Messages API; Codex serves through the
/// ChatGPT backend Responses transport. Antigravity still authorizes fine
/// but needs a provider transport the gateway does not implement, so it is
/// not offered: its spec carries `login_supported: false` and
/// `client_id_for` refuses it.
pub const OAUTH_PROVIDER_IDS: [&str; 2] = ["anthropic", "openai-codex"];

/// An authorization in progress: the URL to visit and the PKCE material the
/// token exchange has to echo back.
#[derive(Debug)]
pub struct Authorization {
    pub provider_id: String,
    pub url: String,
    pub port: u16,
    /// Held so the token exchange presents the same client id the
    /// authorization URL did.
    client_id: String,
    pkce: PkceChallenge,
}

impl Authorization {
    /// The `state` value the loopback callback must return.
    pub fn state(&self) -> &str {
        &self.pkce.state
    }
}

/// What a completed login stored, for reporting. Deliberately carries no token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCredential {
    pub provider_id: String,
    pub account_email: Option<String>,
}

/// The loopback port a provider's registered redirect URI expects.
pub fn default_port(provider_id: &str) -> Result<u16, anyhow::Error> {
    match provider_id {
        "google-antigravity" => Ok(google_antigravity::spec().default_port),
        "openai-codex" => Ok(openai_codex::spec().default_port),
        "anthropic" => Ok(anthropic::spec().default_port),
        other => anyhow::bail!("unsupported provider for OAuth login: {other}"),
    }
}

/// Build the authorization URL for `provider_id`, redirecting to
/// `127.0.0.1:{redirect_port}/callback`.
///
/// Refuses before producing a URL when the provider has no registered OAuth
/// client: an authorization page that rejects the request on arrival is a worse
/// experience than being told what is missing.
pub fn begin(provider_id: &str, redirect_port: u16) -> Result<Authorization, anyhow::Error> {
    let client_id = crate::oauth::providers::client_id_for(provider_id)?;
    begin_with_client_id(provider_id, redirect_port, client_id)
}

/// [`begin`] with the client id supplied rather than read from the environment.
pub fn begin_with_client_id(
    provider_id: &str,
    redirect_port: u16,
    client_id: &str,
) -> Result<Authorization, anyhow::Error> {
    // The Codex client's registered redirect is fixed down to the port. An
    // authorization URL carrying any other port draws a redirect-URI
    // rejection on arrival, so the dead end is refused here, where the fix
    // can be named.
    if provider_id == "openai-codex" {
        let registered = openai_codex::spec().default_port;
        if redirect_port != registered {
            anyhow::bail!(
                "OpenAI's registered redirect is fixed at \
                 http://localhost:{registered}/auth/callback, so the login cannot listen on \
                 port {redirect_port}. Free port {registered} and retry."
            );
        }
    }
    let pkce = PkceChallenge::new();
    let url = match provider_id {
        "google-antigravity" => {
            google_antigravity::build_authorization_url(redirect_port, &pkce, client_id)
        }
        "openai-codex" => openai_codex::build_authorization_url(redirect_port, &pkce, client_id),
        "anthropic" => anthropic::build_authorization_url(redirect_port, &pkce, client_id),
        other => anyhow::bail!("unsupported provider for OAuth login: {other}"),
    };
    Ok(Authorization {
        provider_id: provider_id.to_string(),
        url,
        port: redirect_port,
        client_id: client_id.to_string(),
        pkce,
    })
}

/// Exchange `code` for tokens and store them encrypted in the vault.
pub async fn finish(
    auth: &Authorization,
    code: &str,
    secret_store: &SecretStore,
    client: &reqwest::Client,
    token_url_override: Option<&str>,
) -> Result<StoredCredential, anyhow::Error> {
    let verifier = &auth.pkce.code_verifier;
    let (code, state) = parse_authorization_paste(code, &auth.pkce.state)?;
    let (code, state) = (code.as_str(), state.as_str());
    let tokens = match auth.provider_id.as_str() {
        "google-antigravity" => {
            google_antigravity::exchange_code(
                client,
                &auth.client_id,
                auth.port,
                code,
                verifier,
                token_url_override,
            )
            .await?
        }
        "openai-codex" => {
            openai_codex::exchange_code(
                client,
                &auth.client_id,
                auth.port,
                code,
                verifier,
                token_url_override,
            )
            .await?
        }
        "anthropic" => {
            anthropic::exchange_code(
                client,
                &auth.client_id,
                auth.port,
                code,
                state,
                verifier,
                token_url_override,
            )
            .await?
        }
        other => anyhow::bail!("unsupported provider for OAuth login: {other}"),
    };
    store_tokens(&auth.provider_id, tokens, secret_store)
}

/// Interpret whatever the owner pasted back: a bare code, the
/// `<code>#<state>` pair Anthropic's code page renders, or the full
/// redirected callback URL — the only artifact a Codex login leaves when the
/// redirect cannot reach this machine.
///
/// A pasted state must equal the flow's own. State round-trips verbatim in
/// OAuth, so a mismatch is a corrupted paste or a response minted for a
/// different authorization; refusing here names the problem instead of
/// letting the token endpoint fail with a less honest error.
fn parse_authorization_paste(
    input: &str,
    expected_state: &str,
) -> Result<(String, String), anyhow::Error> {
    let trimmed = input.trim();
    if trimmed.contains("code=") {
        let after_path = trimmed.split_once('?').map(|(_, q)| q).unwrap_or(trimmed);
        let query = after_path.split('#').next().unwrap_or(after_path);
        let params = crate::oauth::callback_server::parse_query(query);
        let code = params
            .get("code")
            .filter(|code| !code.is_empty())
            .ok_or_else(|| anyhow::anyhow!("the pasted input carries no authorization code"))?
            .clone();
        // Presence alone triggers the check: `state=` (empty) is still a
        // carried state, and an empty value can never equal a minted one.
        if let Some(state) = params.get("state") {
            if state != expected_state {
                anyhow::bail!(
                    "OAuth state mismatch in the pasted callback URL; paste the redirect from \
                     this login attempt, not an earlier one"
                );
            }
        }
        return Ok((code, expected_state.to_string()));
    }
    match trimmed.split_once('#') {
        Some((code, state)) if !state.is_empty() => {
            if state != expected_state {
                anyhow::bail!(
                    "OAuth state mismatch in the pasted code; paste the code from this login \
                     attempt, not an earlier one"
                );
            }
            Ok((code.to_string(), state.to_string()))
        }
        _ => Ok((trimmed.to_string(), expected_state.to_string())),
    }
}

fn store_tokens(
    provider_id: &str,
    tokens: TokenResponse,
    secret_store: &SecretStore,
) -> Result<StoredCredential, anyhow::Error> {
    // Never fabricate a refresh token: a placeholder stores a credential that
    // looks fine now and dies at the first renewal, with the background
    // refresher disabling it under a confusing `invalid_grant`.
    //
    // Providers commonly issue a refresh token only on the first authorization,
    // so a re-login that omits one keeps the real token already in the vault.
    // Only a provider that has never issued one is a failure, and the owner is
    // still at the terminal to act on it.
    let reissued = tokens
        .refresh_token
        .clone()
        .filter(|token| !token.is_empty());
    let refresh_token = match reissued {
        Some(token) => token,
        None => secret_store
            .get_oauth_tokens(provider_id)?
            .map(|stored| stored.refresh_token)
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{provider_id} returned no refresh token and none is stored, so OpenSpine \
                     could not renew this credential once it expires. Re-run the login and \
                     grant offline access."
                )
            })?,
    };

    let expires_in = tokens.expires_in.max(300);
    let now_sec = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let expires_at = (now_sec + expires_in).to_string();

    let metadata = OAuthIdentityMetadata {
        account_email: tokens.account_email.clone(),
        account_id: tokens.account_id.clone(),
        identity_key: None,
    };

    secret_store.store_oauth_tokens(
        provider_id,
        &refresh_token,
        &tokens.access_token,
        &expires_at,
        Some(metadata),
    )?;

    Ok(StoredCredential {
        provider_id: provider_id.to_string(),
        account_email: tokens.account_email,
    })
}

/// A lightweight generate through the model gateway, used to prove a provider
/// actually answers before its credential is bound to model roles.
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

/// Bind `selected_provider_id` to OAuth mode in `openspine.yaml` and make it
/// the provider the kernel routes to.
///
/// The promotion is the point. `select_default_provider_id` takes the first
/// configured provider, so appending a freshly authorized provider would leave
/// the previous one serving every turn: the owner completes a login and nothing
/// observable changes.
pub fn update_openspine_yaml_roles(
    config_path: &Path,
    selected_provider_id: &str,
    provider_kind: ProviderKind,
    model: &str,
) -> Result<(), anyhow::Error> {
    let content = std::fs::read_to_string(config_path)?;
    let mut config: Config = serde_yaml::from_str(&content)?;

    let position = config
        .providers
        .iter()
        .position(|provider| provider.id == selected_provider_id);
    let mut selected = match position {
        Some(index) => config.providers.remove(index),
        None => ProviderConfig {
            id: selected_provider_id.to_string(),
            kind: provider_kind,
            base_url: None,
            model: model.to_string(),
            auth: ProviderAuth::Oauth,
        },
    };
    // Binding is a cutover to the canonical transport for this provider id:
    // an entry written by an older build (e.g. `openai-codex` as
    // `openai_compat`) is corrected here, exactly as verification ran. The
    // old kind's base_url goes with it — the new transport's bearer and
    // account header must never be sent to an endpoint configured for a
    // different wire contract.
    if selected.kind != provider_kind {
        selected.kind = provider_kind;
        selected.base_url = None;
    }
    selected.auth = ProviderAuth::Oauth;
    config.providers.insert(0, selected);

    let updated_yaml = serde_yaml::to_string(&config)?;
    std::fs::write(config_path, updated_yaml)?;
    Ok(())
}

/// Verify the provider the kernel will actually route to, through the model
/// gateway, as a readiness check.
///
/// Static checks can all pass on a host with no model server at all: a
/// generated API key satisfies the credential check while nothing is listening.
/// This is the only check that proves the install can produce a reply.
pub async fn verify_default_provider(config: &Config, vault: Option<&SecretStore>) -> Check {
    const ID: &str = "provider.reachable";
    const LABEL: &str = "model endpoint";

    let Some(provider) = config.providers.first() else {
        return Check::fail(
            ID,
            LABEL,
            "no providers configured".to_string(),
            "add a `providers:` entry".to_string(),
        );
    };
    let remedy = format!(
        "check that {} is serving model `{}`",
        provider.base_url.as_deref().unwrap_or("the provider"),
        provider.model
    );
    let (Ok(api_key), Some(vault)) = (crate::config::provider_api_key(provider), vault) else {
        return Check::warn(
            ID,
            LABEL,
            "not probed (provider credentials unresolved)".to_string(),
            "resolve the provider checks above, then re-run".to_string(),
        );
    };

    let client = ProviderClient::from_config(provider, api_key.clone());
    match run_preflight_verification_ping(&client, vault, &provider.id).await {
        Ok(true) => Check::pass(
            ID,
            LABEL,
            format!("{} answered a verification request", provider.id),
        ),
        // A completion the probe caps at 10 tokens can legitimately be empty:
        // a reasoning or vision model may emit nothing that short. The HTTP
        // round trip already proved the endpoint and the model id resolve, so
        // this is worth saying and not worth blocking on.
        Ok(false) => Check::warn(
            ID,
            LABEL,
            format!(
                "{} answered, but produced no text for a short probe",
                provider.id
            ),
            remedy,
        ),
        // A gateway error quotes the provider response body verbatim, and that
        // body can echo the credential that was sent. For an API-key provider
        // that is the resolved key; for an OAuth provider the key is only the
        // `oauth:<id>` sentinel and the real bearer is the vault's access token,
        // so both have to go.
        Err(error) => Check::fail(
            ID,
            LABEL,
            format!(
                "{} did not answer: {}",
                provider.id,
                redact_credentials(&error.to_string(), &api_key, vault, &provider.id)
            ),
            remedy,
        ),
    }
}

/// Replace every credential this call could have transmitted.
///
/// No length floor. A short credential is still a credential: the starter
/// configuration's local API key is the literal `local`, and skipping it would
/// print it verbatim. Redacting a short common word can mangle surrounding
/// prose, which is the correct trade against leaking the key.
fn redact_credentials(text: &str, api_key: &str, vault: &SecretStore, provider_id: &str) -> String {
    let mut secrets = vec![api_key.to_string()];
    if let Ok(Some(tokens)) = vault.get_oauth_tokens(provider_id) {
        secrets.push(tokens.access_token);
        secrets.push(tokens.refresh_token);
    }
    let mut out = text.to_string();
    for secret in secrets {
        if !secret.is_empty() {
            out = out.replace(&secret, "<redacted>");
        }
    }
    out
}

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "setup_paste_tests.rs"]
mod paste_tests;

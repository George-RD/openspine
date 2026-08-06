//! `openspine provider login`: OAuth authorization, verification, and binding.
//!
//! Split from [`super::wizard`] so the interactive setup flow and the login
//! flow each stay readable, and so the file-size gate stays satisfied without
//! an escape hatch.

use super::prompt::prompt;
use super::setup::{self, OAUTH_PROVIDER_IDS};
use super::wizard::open_vault;
use crate::config::{Config, ProviderAuth, ProviderConfig, ProviderKind};
use crate::model_gateway::ProviderClient;
use crate::oauth::callback_server::CallbackServer;
use crate::secret_store::SecretStore;
use anyhow::Context as _;
use std::path::Path;

/// `openspine provider login [id] [--force]`.
pub async fn run_provider_login(
    config_path: &Path,
    provider_id: Option<&str>,
    force: bool,
) -> anyhow::Result<()> {
    let vault = open_vault(config_path)?;
    let Some(store) = vault.store() else {
        anyhow::bail!("{}", vault.write_refusal());
    };
    login_flow(
        config_path,
        Some(store),
        &reqwest::Client::new(),
        provider_id,
        force,
    )
    .await
}

/// Authorize a model provider, verify it, then bind it.
///
/// A headless owner gets the URL printed and pastes the code back; otherwise the
/// loopback listener is bound first, because the provider redirect arrives the
/// moment the browser finishes.
pub(super) async fn login_flow(
    config_path: &Path,
    store: Option<&SecretStore>,
    client: &reqwest::Client,
    provider_id: Option<&str>,
    force: bool,
) -> anyhow::Result<()> {
    let store = store
        .context("the credential vault is not open; resolve the key material checks and re-run")?;
    let provider_id = match provider_id {
        Some(id) => id.to_string(),
        None => choose_provider()?,
    };
    // The rebind shortcut below must not outflank the spendability refusal:
    // a stored credential for a provider this build cannot serve stays
    // refused exactly as a fresh authorization would be.
    crate::oauth::providers::client_id_for(&provider_id)?;
    // Switching between held subscriptions must not cost a browser round
    // trip: a healthy stored credential is re-verified and re-bound
    // directly. `--force` re-runs the authorization (for example to bind a
    // different provider-side account).
    if !force {
        if let Ok(Some(tokens)) = store.get_oauth_tokens(&provider_id) {
            let healthy = !tokens.disabled
                && !tokens.refresh_token.is_empty()
                && ensure_codex_identity(store, &provider_id, &tokens)?;
            if healthy {
                println!(
                    "A stored {provider_id} credential exists; re-verifying and binding it \
                     without a new authorization. Use --force to re-authorize."
                );
                return verify_and_bind(config_path, store, &provider_id).await;
            }
        }
    }
    let port = setup::default_port(&provider_id)?;

    let (auth, code) = if headless() {
        let auth = setup::begin(&provider_id, port)?;
        println!();
        println!("Open this URL in a browser on any machine, then paste the code back here:");
        println!();
        println!("  {}", auth.url);
        println!();
        let code = prompt("Authorization code", None)?
            .context("login needs the authorization code from the browser")?;
        (auth, code)
    } else {
        let callback = CallbackServer::bind(port)
            .await
            .with_context(|| format!("binding the OAuth callback listener on port {port}"))?;
        let auth = setup::begin(&provider_id, callback.port())?;
        println!();
        println!("Opening this URL, waiting for the redirect:");
        println!();
        println!("  {}", auth.url);
        open_in_browser(&auth.url);
        let code = callback
            .wait_for_code(auth.state())
            .await
            .context("waiting for the OAuth redirect")?;
        (auth, code)
    };

    complete_login(config_path, store, client, &auth, &code).await
}

async fn complete_login(
    config_path: &Path,
    store: &SecretStore,
    client: &reqwest::Client,
    auth: &setup::Authorization,
    code: &str,
) -> anyhow::Result<()> {
    let stored = setup::finish(auth, code, store, client, None).await?;
    match &stored.account_email {
        Some(email) => println!("Logged in to {} as {email}.", stored.provider_id),
        None => println!("Logged in to {}.", stored.provider_id),
    }
    verify_and_bind(config_path, store, &stored.provider_id).await
}

/// Verify `provider_id` through the model gateway and, on success, bind it
/// as the provider the kernel routes to. Shared by a fresh login and the
/// stored-credential re-bind path.
async fn verify_and_bind(
    config_path: &Path,
    store: &SecretStore,
    provider_id: &str,
) -> anyhow::Result<()> {
    let (provider, kind) = provider_entry(config_path, provider_id)?;
    println!("Verifying {} through the model gateway...", provider.id);
    // Verify as OAuth even when the configured entry still says `api_key`:
    // logging in is precisely the act of moving it. The configuration is only
    // rewritten once this succeeds, so nothing is persisted on a failure.
    let verifying = ProviderConfig {
        auth: ProviderAuth::Oauth,
        ..provider.clone()
    };
    let verified = setup::run_preflight_verification_ping(
        &ProviderClient::from_config(&verifying, String::new()),
        store,
        &provider.id,
    )
    .await
    .unwrap_or(false);

    if !verified {
        // The spec binds roles only on a successful verification. The stored
        // credential stays, so a retry skips the authorization.
        println!(
            "{} did not answer the verification request. The credential is stored, so \
             `openspine setup` can retry the check without a new login.",
            provider.id
        );
        return Ok(());
    }

    setup::update_openspine_yaml_roles(config_path, &provider.id, kind, &provider.model)?;
    println!(
        "Verified. {} is now bound in {}.",
        provider.id,
        config_path.display()
    );
    Ok(())
}

/// A Codex credential is only spendable with its ChatGPT account id. A store
/// without one (an older build's login) is backfilled from the stored access
/// token when the claim is present; otherwise the credential is incomplete
/// and the caller falls through to a fresh authorization instead of a
/// rebind that could only fail.
fn ensure_codex_identity(
    store: &SecretStore,
    provider_id: &str,
    tokens: &crate::secret_store::OAuthTokens,
) -> anyhow::Result<bool> {
    if provider_id != "openai-codex" {
        return Ok(true);
    }
    if tokens
        .account_id
        .as_deref()
        .is_some_and(|id| !id.is_empty())
    {
        return Ok(true);
    }
    let Some(account_id) =
        crate::oauth::providers::openai_codex::access_token_account_id(&tokens.access_token)
    else {
        return Ok(false);
    };
    store.store_oauth_tokens(
        provider_id,
        &tokens.refresh_token,
        &tokens.access_token,
        &tokens.expires_at,
        Some(crate::secret_store::OAuthIdentityMetadata {
            account_email: tokens.account_email.clone(),
            account_id: Some(account_id),
            identity_key: tokens.identity_key.clone(),
        }),
    )?;
    Ok(true)
}

/// The provider entry to verify against: the configured one when it exists, or
/// an in-memory entry the owner names. Nothing is written until verification
/// succeeds.
pub(super) fn provider_entry(
    config_path: &Path,
    provider_id: &str,
) -> anyhow::Result<(ProviderConfig, ProviderKind)> {
    let kind = match provider_id {
        "anthropic" => ProviderKind::Anthropic,
        "openai-codex" => ProviderKind::OpenaiCodex,
        "google-antigravity" => ProviderKind::GoogleAntigravity,
        other => anyhow::bail!("unsupported provider for OAuth login: {other}"),
    };
    if let Some(mut existing) = Config::load(config_path)
        .ok()
        .and_then(|config| {
            config
                .providers
                .into_iter()
                .find(|provider| provider.id == provider_id)
        })
        .filter(|provider| !provider.model.is_empty())
    {
        // The id namespace is kernel-defined: `openai-codex` is served by
        // the Responses transport whatever kind an older configuration
        // recorded. Verification must run against the transport the binding
        // will actually use — and a base_url written for the OLD transport
        // must not receive the new transport's bearer and account header, so
        // a kind cutover also resets the endpoint to the canonical default.
        if existing.kind != kind {
            existing.kind = kind;
            existing.base_url = None;
        }
        return Ok((existing, kind));
    }
    let model = prompt(&format!("Model id for {provider_id}"), None)?
        .context("verification needs a model id")?;
    Ok((
        ProviderConfig {
            id: provider_id.to_string(),
            kind,
            base_url: None,
            model,
            auth: ProviderAuth::Oauth,
        },
        kind,
    ))
}

fn choose_provider() -> anyhow::Result<String> {
    println!("Providers this build can log in to:");
    for (index, id) in OAUTH_PROVIDER_IDS.iter().enumerate() {
        println!("  {}) {id}", index + 1);
    }
    let default = "1".to_string();
    let choice = prompt("Provider", Some(&default))?.unwrap_or(default);
    match choice.parse::<usize>() {
        Ok(index) if index >= 1 && index <= OAUTH_PROVIDER_IDS.len() => {
            Ok(OAUTH_PROVIDER_IDS[index - 1].to_string())
        }
        _ if OAUTH_PROVIDER_IDS.contains(&choice.as_str()) => Ok(choice),
        _ => anyhow::bail!("'{choice}' is not a provider this build can log in to"),
    }
}

/// No browser to open, so the URL has to be printed and the code pasted back.
fn headless() -> bool {
    if std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some() {
        return true;
    }
    cfg!(target_os = "linux")
        && std::env::var_os("DISPLAY").is_none()
        && std::env::var_os("WAYLAND_DISPLAY").is_none()
}

/// Best effort. The URL is always printed first, so a failure here costs the
/// owner one copy and paste.
fn open_in_browser(url: &str) {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(test)]
#[path = "login_tests.rs"]
mod tests;

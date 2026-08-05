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

/// `openspine provider login [id]`.
pub async fn run_provider_login(
    config_path: &Path,
    provider_id: Option<&str>,
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
) -> anyhow::Result<()> {
    let store = store
        .context("the credential vault is not open; resolve the key material checks and re-run")?;
    let provider_id = match provider_id {
        Some(id) => id.to_string(),
        None => choose_provider()?,
    };
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

    let (provider, kind) = provider_entry(config_path, &stored.provider_id)?;
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

/// The provider entry to verify against: the configured one when it exists, or
/// an in-memory entry the owner names. Nothing is written until verification
/// succeeds.
pub(super) fn provider_entry(
    config_path: &Path,
    provider_id: &str,
) -> anyhow::Result<(ProviderConfig, ProviderKind)> {
    let kind = match provider_id {
        "anthropic" => ProviderKind::Anthropic,
        "openai-codex" => ProviderKind::OpenaiCompat,
        "google-antigravity" => ProviderKind::GoogleAntigravity,
        other => anyhow::bail!("unsupported provider for OAuth login: {other}"),
    };
    if let Some(existing) = Config::load(config_path)
        .ok()
        .and_then(|config| {
            config
                .providers
                .into_iter()
                .find(|provider| provider.id == provider_id)
        })
        .filter(|provider| !provider.model.is_empty())
    {
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
mod tests {
    use super::*;

    /// An SSH session must take the printed-URL path: binding a loopback
    /// listener the owner's browser cannot reach would hang the login.
    #[test]
    fn an_ssh_session_is_treated_as_headless() {
        let restore = std::env::var_os("SSH_CONNECTION");
        std::env::set_var("SSH_CONNECTION", "10.0.0.1 22 10.0.0.2 22");

        let headless = headless();

        match restore {
            Some(value) => std::env::set_var("SSH_CONNECTION", value),
            None => std::env::remove_var("SSH_CONNECTION"),
        }
        assert!(headless);
    }

    #[test]
    fn every_supported_provider_maps_to_a_gateway_kind() {
        let dir = std::env::temp_dir().join(format!("openspine-wizard-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("openspine.yaml");
        std::fs::write(
            &config_path,
            "data_dir: d\nsandbox:\n  driver: process\nowner:\n  telegram_user_id: 1\n  \
             display_name: o\nspend_cap: {}\nproviders:\n  - id: anthropic\n    kind: anthropic\n \
             \x20  model: m\n    auth:\n      mode: oauth\n",
        )
        .unwrap();

        let (provider, kind) = provider_entry(&config_path, "anthropic").unwrap();

        assert_eq!(provider.model, "m");
        assert_eq!(kind, ProviderKind::Anthropic);
    }

    #[test]
    fn an_unsupported_provider_is_refused_before_prompting() {
        let error = provider_entry(Path::new("/nonexistent.yaml"), "not-a-provider").unwrap_err();
        assert!(error.to_string().contains("unsupported provider"));
    }
}

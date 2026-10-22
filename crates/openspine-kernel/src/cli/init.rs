//! `openspine init` — the single-command first-run trust loop.
//!
//! Establishes the three trust-ceremony artifacts in one non-interactive
//! command, so the first-run path is a governed trust loop rather than a
//! repository-setup exercise:
//!
//!   1. **Seed key** — writes the owner-only key file (artifact, grant, and
//!      webhook keys) and a starter configuration when the install has neither.
//!   2. **Approval anchor** — bootstraps the single trusted owner principal into
//!      a fresh kernel store. This is the identity every later approval and
//!      audit row is bound to.
//!   3. **Test** — reports readiness and the honest next steps (authorize a
//!      model, run the first governed task) before the owner talks to Lyra.
//!
//! It never introduces an ungated authority path: the owner principal it writes
//! is exactly the one the kernel would bootstrap on first boot, via the same
//! [`crate::identity::bootstrap_owner_principal`] entry point, and the
//! deterministic pipeline is untouched. The principal is only written on a
//! demonstrably fresh data root, so it never appends a bootstrap audit event
//! against an existing store the kernel has not yet validated (`run()` in
//! `main.rs` validates and reconciles integrity before it bootstraps).

use super::bootstrap;
use super::readiness::{self, Readiness};
use super::starter::{self, StarterConfig};
use crate::config::{self, Config};
use crate::env_file;
use crate::{identity, overlay_export_restore, store};
use anyhow::Context as _;
use std::path::Path;

/// What the owner passed to `openspine init`.
#[derive(Debug, Clone, Copy, Default)]
pub struct InitOptions<'a> {
    /// The owner's Telegram user id — the single trusted-principal channel
    /// binding. Required on a fresh install because the owner principal is
    /// bound once and the kernel fails closed on a later mismatch; requiring
    /// the real id up front avoids binding a placeholder that would trap the
    /// owner.
    pub owner: Option<i64>,
    /// The owner's display name.
    pub name: Option<&'a str>,
    /// The model id to configure. Without it, the endpoint is asked what it
    /// serves and a lone answer is used.
    pub model: Option<&'a str>,
    /// The OpenAI-compatible endpoint of a fresh install.
    pub base_url: Option<&'a str>,
}

/// Run `openspine init`.
///
/// Init succeeds whenever it has established every artifact it owns; a
/// not-yet-authorized model provider is reported as a next step, not failed.
/// A model it cannot determine is different: a configuration with no model
/// answers nothing, so a fresh install is refused rather than written.
pub async fn run_init(config_path: &Path, options: InitOptions<'_>) -> anyhow::Result<()> {
    println!("OpenSpine first-run setup");
    println!("  configuration  {}", config_path.display());
    println!(
        "  key file       {}",
        env_file::path_for(config_path).display()
    );
    println!();

    if config_path.exists() {
        anyhow::ensure!(
            options.base_url.is_none(),
            "--base-url only applies to a fresh install; edit the `providers:` entry in {} instead",
            config_path.display()
        );
        prepare_existing(config_path, options.owner)?;
        settle_model_existing(config_path, options.model).await?;
    } else {
        let owner_id = require_owner(options.owner)?;
        let base_url = options.base_url.unwrap_or(starter::DEFAULT_BASE_URL);
        let discovered = if options.model.is_some_and(|model| !model.trim().is_empty()) {
            Vec::new()
        } else {
            starter::discover_models(&reqwest::Client::new(), base_url).await
        };
        let model = starter::resolve_model(options.model, &discovered, base_url)?;
        write_fresh(config_path, owner_id, options.name, base_url, &model)?;
    }

    // Export the seed keys so readiness and the store open see them, exactly as
    // the kernel's own startup does before anything reads key material.
    env_file::load_adjacent(config_path).context("loading the seed key file")?;

    let config =
        Config::load(config_path).with_context(|| format!("loading {}", config_path.display()))?;

    // Approval anchor. Only bind on a demonstrably fresh data root: `run()`
    // validates and reconciles integrity before it bootstraps, so appending a
    // bootstrap audit against an existing, unvalidated store would break that
    // ordering. An install with any encrypted state defers to the kernel's own
    // validated boot bootstrap.
    if bootstrap::encrypted_state_in(config_path).is_none() {
        bootstrap_owner(&config)?;
        println!(
            "  trusted owner principal established for {} (telegram_user_id {})",
            config.owner.display_name, config.owner.telegram_user_id
        );
    } else {
        println!(
            "  existing install detected — the owner principal is bound and validated on the next `openspine` boot"
        );
    }
    println!();

    let readiness = readiness::assess(config_path, &readiness::process_env, None);
    println!("Readiness");
    print!("{}", readiness.render());
    println!();

    report_trust_ceremony(config_path, &readiness);
    Ok(())
}

fn require_owner(owner: Option<i64>) -> anyhow::Result<i64> {
    owner.ok_or_else(|| {
        anyhow::anyhow!(
            "openspine init needs --owner <telegram_user_id> to bind the trusted owner \
             principal on a fresh install. Message @userinfobot on Telegram to find yours, \
             then re-run: openspine init --owner <id> --model <model>"
        )
    })
}

/// Write a starter configuration and seed key file for an install that has
/// neither, binding the owner's real Telegram id and a real model up front.
fn write_fresh(
    config_path: &Path,
    owner_id: i64,
    name: Option<&str>,
    base_url: &str,
    model: &str,
) -> anyhow::Result<()> {
    let mut starter = StarterConfig::defaults(config_path, owner_id);
    starter.base_url = base_url.to_string();
    starter.model = model.to_string();
    if let Some(name) = name {
        starter.display_name = name.to_string();
    }

    let generated = starter::write(config_path, &starter, &readiness::process_env)
        .context("writing the starter configuration and seed key file")?;
    println!("  wrote {}", config_path.display());
    println!("  model          {model} at {base_url}");
    if !generated.is_empty() {
        let names: Vec<&str> = generated.iter().map(|(name, _)| name.as_str()).collect();
        println!(
            "  wrote {} (mode 0600) with {}",
            env_file::path_for(config_path).display(),
            names.join(", ")
        );
    }
    Ok(())
}

/// Give an existing configuration a model: the one named with `--model`, or
/// else, when none is set, the only one its endpoint serves. An ambiguous or
/// unreachable endpoint is left to the readiness report, which blocks on the
/// empty model and names the fix.
async fn settle_model_existing(config_path: &Path, requested: Option<&str>) -> anyhow::Result<()> {
    let config = Config::load(config_path)?;
    let Some(provider) = config.providers.first() else {
        return Ok(());
    };
    let requested = requested.map(str::trim).filter(|model| !model.is_empty());
    let model = match requested {
        Some(model) if model == provider.model => return Ok(()),
        Some(model) => model.to_string(),
        None if !provider.model.trim().is_empty() => return Ok(()),
        None => {
            let Some(base_url) = provider.base_url.as_deref() else {
                return Ok(());
            };
            let served = starter::discover_models(&reqwest::Client::new(), base_url).await;
            match starter::resolve_model(None, &served, base_url) {
                Ok(model) => model,
                Err(error) => {
                    println!("  model          not set: {error}");
                    return Ok(());
                }
            }
        }
    };
    starter::set_default_model(config_path, &model)?;
    println!("  model          provider {} now uses {model}", provider.id);
    Ok(())
}

/// Repair an install that already has a configuration: fill in any missing seed
/// keys, and refuse an `--owner` that conflicts with the bound owner identity.
fn prepare_existing(config_path: &Path, owner: Option<i64>) -> anyhow::Result<()> {
    // Owner identity is immutable once written. `bootstrap_owner_principal` is
    // fail-closed against a changed id, so silently rewriting the config from a
    // conflicting `--owner` would leave an install that refuses to boot. Reject
    // and name the recovery instead.
    if let (Some(owner_id), Ok(configured)) = (
        owner,
        Config::load(config_path).map(|c| c.owner.telegram_user_id),
    ) {
        if configured != owner_id {
            anyhow::bail!(
                "configuration at {} already names owner telegram_user_id {}, which cannot be \
                 changed to {}: the owner principal is bound to the first id and the kernel fails \
                 closed on a mismatch. Keep the configured id, or start from a fresh data directory.",
                config_path.display(),
                configured,
                owner_id
            );
        }
    }

    let missing = bootstrap::missing_key_names(&readiness::process_env);
    if missing.is_empty() {
        return Ok(());
    }
    bootstrap::guard_orphan_artifact_key(config_path, &missing)?;
    let added = bootstrap::add_missing_key_material(config_path)?;
    if !added.is_empty() {
        println!(
            "  added {} to {} (mode 0600)",
            added.join(", "),
            env_file::path_for(config_path).display()
        );
    }
    Ok(())
}

/// Bind the single trusted owner principal into the kernel store, through the
/// same [`identity::bootstrap_owner_principal`] entry point the kernel uses on
/// first boot. The data-root lifetime lock guarantees no kernel is writing the
/// same state underneath us.
fn bootstrap_owner(config: &Config) -> anyhow::Result<()> {
    let artifact_key = config::artifact_key_bytes().context("reading the artifact key")?;
    let operations = match overlay_export_restore::acquire(&config.data_dir, &artifact_key) {
        Ok(operations) => operations,
        Err(error) if overlay_export_restore::is_already_locked(&error) => {
            anyhow::bail!(
                "another OpenSpine process is holding {}. Stop it before running `openspine init`.",
                config.data_dir.display()
            );
        }
        Err(error) => {
            return Err(anyhow::Error::new(error).context("acquiring the data-root lifetime lock"));
        }
    };
    let store = store::Store::open(&operations.canonical_data_root().join("kernel.db"))
        .context("opening the kernel store")?;
    identity::bootstrap_owner_principal(
        &store,
        config.owner.telegram_user_id,
        &config.owner.display_name,
    )
    .context("bootstrapping the owner principal")?;
    Ok(())
}

/// Print the plain-language trust ceremony and the honest next steps. The
/// governed pipeline is unchanged; this only explains where the boundary is.
fn report_trust_ceremony(config_path: &Path, readiness: &Readiness) {
    let env_path = env_file::path_for(config_path);
    println!("Trust ceremony");
    println!(
        "  seed key   {} holds the artifact, grant, and webhook keys at mode 0600. They \
         encrypt the vault and sign task grants; they never enter a worker.",
        env_path.display()
    );
    println!(
        "  approval   you are the single trusted owner. Lyra runs each task under a \
         short-lived grant; an external effect (for example sending mail) stops for your \
         approval and stays denied until you approve the exact action."
    );
    println!(
        "  test       run `openspine chat` and send a message. `/status` prints readiness; \
         a governed reply is your proof the loop is closed."
    );
    println!();

    if !readiness.is_ready() {
        println!("Before your first task:");
        print!("{}", readiness.render_blocking());
        println!(
            "  resolve the items above (`openspine setup` walks through them; \
             `openspine provider login` authorizes an OAuth model), then start `openspine chat`."
        );
    } else {
        println!("Next: start `openspine chat` and send Lyra a message.");
    }
}

#[cfg(test)]
#[path = "init_tests.rs"]
mod tests;

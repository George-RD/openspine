//! The interactive `openspine setup` flow.
//!
//! Everything here is I/O and sequencing. The decisions it reports come from
//! [`crate::cli::readiness`], the credential work from [`crate::cli::setup`],
//! and provider authorization from [`crate::cli::login`], so the parts worth
//! testing are testable without a terminal.

use super::login::login_flow;
use super::prompt::{confirm, prompt};
use crate::cli::readiness::{self, Readiness};
use crate::cli::setup;
use crate::cli::starter::{self, StarterConfig};
use crate::config::{self, Config};
use crate::env_file;
use crate::overlay_export_restore::{self, OverlayOperations};
use crate::secret_store::SecretStore;
use anyhow::Context as _;
use std::path::{Path, PathBuf};

/// An open credential vault together with the data-root lifetime lock that
/// guarantees no kernel is writing the same state underneath us.
pub(super) struct OpenVault {
    _lock: OverlayOperations,
    store: SecretStore,
}

pub(super) enum Vault {
    /// Boxed: the open variant carries a kilobyte of controller state, and an
    /// unboxed enum would pay for it in every unit variant too.
    Open(Box<OpenVault>),
    /// Key material is absent, so the vault cannot be decrypted. Readiness
    /// already reports this; onboarding continues without credential state.
    Unavailable,
    /// A running kernel holds the data-root lifetime lock.
    ///
    /// Reporting readiness must still work here. `openspine chat` holds this
    /// lock for as long as it runs, and its own first-run notice tells the owner
    /// to run `openspine setup`: refusing outright would make following that
    /// advice from a second terminal fail instead of explaining anything.
    /// Actions that write the vault are refused individually.
    Locked,
}

impl Vault {
    pub(super) fn store(&self) -> Option<&SecretStore> {
        match self {
            Self::Open(open) => Some(&open.store),
            Self::Unavailable | Self::Locked => None,
        }
    }

    /// Whether vault-writing actions can run.
    pub(super) fn writable(&self) -> bool {
        matches!(self, Self::Open(_))
    }

    /// Why a writing action cannot run, for the owner.
    pub(super) fn write_refusal(&self) -> &'static str {
        match self {
            Self::Locked => {
                "another OpenSpine instance is using this data directory, so nothing can be \
                 written to the credential vault. Stop it (an `openspine chat` in another \
                 terminal holds the lock for as long as it runs) and re-run."
            }
            _ => "the credential vault is not open; resolve the key material checks above first.",
        }
    }
}

/// `openspine setup`. Returns whether the install ended up ready.
pub async fn run_setup(config_path: &Path, check_only: bool) -> anyhow::Result<bool> {
    if !check_only {
        println!("OpenSpine setup");
        println!("  configuration  {}", config_path.display());
        println!(
            "  key file       {}",
            env_file::path_for(config_path).display()
        );
        println!();
        if !config_path.exists() {
            bootstrap(config_path).await?;
        } else {
            // A configuration written by a packaging wrapper, or copied from
            // another host, can exist with no key file at all. Without this the
            // "run `openspine setup`" remedy on the key checks would send the
            // owner back to the command they just ran.
            ensure_key_material(config_path)?;
        }
    }

    let mut vault = open_vault(config_path)?;
    if matches!(vault, Vault::Locked) {
        println!("{}", vault.write_refusal());
        println!("Reporting readiness only.");
        println!();
    }
    let mut current = report(config_path, &vault).await;
    if check_only {
        return Ok(current.is_ready());
    }

    let client = reqwest::Client::new();
    loop {
        println!();
        println!("  1) Log in to a model provider with OAuth");
        println!("  2) Re-check readiness");
        println!("  3) Send a verification request to a provider");
        println!("  4) Done");
        let Some(choice) = prompt("choose", Some("4"))? else {
            break;
        };
        match choice.as_str() {
            "1" if !vault.writable() => println!("  {}", vault.write_refusal()),
            "1" => {
                if let Err(error) = login_flow(config_path, vault.store(), &client, None).await {
                    eprintln!("login failed: {error:#}");
                }
                refresh_vault(&mut vault, config_path)?;
                current = report(config_path, &vault).await;
            }
            "2" => {
                refresh_vault(&mut vault, config_path)?;
                current = report(config_path, &vault).await;
            }
            "3" => {
                if let Err(error) = verify_configured_provider(config_path, &vault).await {
                    eprintln!("verification failed: {error:#}");
                }
            }
            "4" | "" => break,
            other => println!("  '{other}' is not one of the options."),
        }
    }

    println!();
    if current.is_ready() {
        println!("This install is ready. Run `openspine` to start talking to Lyra.");
    } else {
        println!("Items above still block a reply. Re-run `openspine setup` after resolving them.");
    }
    Ok(current.is_ready())
}

/// passes on a host with a generated API key and no model server at all, which
/// is exactly the install that then fails on its first turn.
async fn report(config_path: &Path, vault: &Vault) -> Readiness {
    let mut readiness = readiness::assess(config_path, &readiness::process_env, vault.store());
    if readiness.is_ready() {
        if let Ok(config) = Config::load(config_path) {
            readiness
                .checks
                .push(setup::verify_default_provider(&config, vault.store()).await);
        }
    }
    println!("Readiness");
    print!("{}", readiness.render());
    readiness
}

/// Acquire the data-root lifetime lock and open the credential vault under the
/// canonical data root the overlay controller resolves.
pub(super) fn open_vault(config_path: &Path) -> anyhow::Result<Vault> {
    let Ok(config) = Config::load(config_path) else {
        return Ok(Vault::Unavailable);
    };
    let Ok(artifact_key) = config::artifact_key_bytes() else {
        return Ok(Vault::Unavailable);
    };
    let lock = match overlay_export_restore::acquire(&config.data_dir, &artifact_key) {
        Ok(lock) => lock,
        // Degrade instead of failing: readiness has to be reportable while a
        // kernel is running, and readiness already treats a closed vault as
        // unchecked rather than guessing.
        Err(error) if overlay_export_restore::is_already_locked(&error) => {
            return Ok(Vault::Locked)
        }
        Err(error) => {
            return Err(anyhow::Error::new(error).context("acquiring the data-root lifetime lock"))
        }
    };
    let store = SecretStore::open(lock.canonical_data_root().join("credentials"), artifact_key)
        .context("opening the credential vault")?;
    Ok(Vault::Open(Box::new(OpenVault { _lock: lock, store })))
}

/// Reopen the vault only when it is not already open.
///
/// Reopening an already-open vault would try to acquire a data-root lock this
/// process is still holding, which always fails. It is also unnecessary: the
/// store reads each slot from disk on every call, so an open handle already
/// sees a credential written moments ago.
fn refresh_vault(vault: &mut Vault, config_path: &Path) -> anyhow::Result<()> {
    if matches!(vault, Vault::Open(_)) {
        return Ok(());
    }
    *vault = open_vault(config_path)?;
    Ok(())
}

/// Write a starter configuration and key file for an install that has neither.
async fn bootstrap(config_path: &Path) -> anyhow::Result<()> {
    println!("No configuration exists yet.");
    if !confirm("Write a starter configuration there?", true)? {
        anyhow::bail!("setup needs a configuration file to continue");
    }

    let mut starter = StarterConfig::defaults(config_path);
    if let Some(value) = prompt("Your name", Some(&starter.display_name))? {
        starter.display_name = value;
    }
    if let Some(value) = prompt(
        "Model endpoint (OpenAI-compatible)",
        Some(&starter.base_url),
    )? {
        starter.base_url = value;
    }
    starter.model = choose_model(&starter.base_url).await?;

    let generated = starter::write(config_path, &starter, &readiness::process_env)?;
    println!("  wrote {}", config_path.display());
    if !generated.is_empty() {
        let names: Vec<&str> = generated.iter().map(|(name, _)| name.as_str()).collect();
        println!(
            "  wrote {} (mode 0600) with {}",
            env_file::path_for(config_path).display(),
            names.join(", ")
        );
    }
    // Export through the same loader every command uses, so the rest of the
    // wizard sees the new keys and an operator-supplied variable still wins over
    // the file. Exporting the generated pairs directly here would overwrite an
    // existing key and orphan the vault encrypted under it.
    let loaded = env_file::load_adjacent(config_path)?;
    if !loaded.retained.is_empty() {
        println!(
            "  kept the environment's own {} over the file",
            loaded.retained.join(", ")
        );
    }
    println!();
    Ok(())
}

/// Fill in key material an existing configuration is missing.
///
/// Only names the file does not already define are added, so an existing key
/// keeps the value the vault on disk was encrypted under.
fn ensure_key_material(config_path: &Path) -> anyhow::Result<()> {
    let env_path = env_file::path_for(config_path);
    let entries = starter::key_entries(&readiness::process_env);
    let missing: Vec<&str> = entries
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|name| readiness::process_env(name).is_none())
        .collect();
    if missing.is_empty() {
        return Ok(());
    }

    // The artifact key encrypts the credential vault, the artifact store, and
    // the counterparty key ring. Minting a fresh one over an existing data root
    // does not fail loudly: it silently makes every stored credential
    // undecryptable. Refuse, and name the recovery.
    if missing.contains(&"OPENSPINE_ARTIFACT_KEY") {
        if let Some(state) = encrypted_state_in(config_path) {
            anyhow::bail!(
                "{} already holds encrypted state but OPENSPINE_ARTIFACT_KEY is not set. \
                 Restore the original key into {}: generating a new one would make the \
                 existing vault permanently unreadable.",
                state.display(),
                env_path.display()
            );
        }
    }

    println!("This install has no value for {}.", missing.join(", "));
    if !confirm("Generate the missing key material now?", true)? {
        return Ok(());
    }

    let added = env_file::merge_owner_only(&env_path, &entries)?;
    if added.is_empty() {
        println!("  {} already defines them", env_path.display());
    } else {
        println!(
            "  added {} to {} (mode 0600)",
            added.join(", "),
            env_path.display()
        );
    }
    env_file::load_adjacent(config_path)?;
    println!();
    Ok(())
}

/// The first piece of artifact-key-encrypted state in the configured data root,
/// or `None` for a genuinely new one.
///
/// Uses the configured `data_dir` rather than the overlay controller's canonical
/// root so it needs no lock: this runs before the vault is opened.
fn encrypted_state_in(config_path: &Path) -> Option<PathBuf> {
    let data_dir = Config::load(config_path).ok()?.data_dir;
    ["credentials", "artifacts", "kernel.db"]
        .iter()
        .map(|entry| data_dir.join(entry))
        .find(|path| match std::fs::read_dir(path) {
            Ok(mut entries) => entries.next().is_some(),
            // Not a directory: `kernel.db` is a file, so existence is enough.
            Err(_) => path.exists(),
        })
}

/// Offer the model ids the endpoint actually serves. A model string this binary
/// invented would be a guess that fails on the first turn.
async fn choose_model(base_url: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let models = starter::discover_models(&client, base_url).await;
    if models.is_empty() {
        println!("  {base_url} did not answer a model listing.");
        return prompt("Model id", None)?
            .context("setup needs a model id to write a usable configuration");
    }

    println!("  models served by {base_url}:");
    for (index, model) in models.iter().enumerate() {
        println!("    {}) {model}", index + 1);
    }
    let default = "1".to_string();
    let choice = prompt("Model", Some(&default))?.unwrap_or(default);
    match choice.parse::<usize>() {
        Ok(index) if index >= 1 && index <= models.len() => Ok(models[index - 1].clone()),
        // Anything that is not an offered index is taken as a literal model id,
        // so an endpoint with a hidden model is still reachable.
        _ => Ok(choice),
    }
}

/// Probe the default provider on demand.
///
/// Routed through the same check the report uses, so the provider error body
/// that a gateway error quotes verbatim passes through the same credential
/// scrubbing. A second formatting path here would print the bearer that was
/// sent on a malformed request.
async fn verify_configured_provider(config_path: &Path, vault: &Vault) -> anyhow::Result<()> {
    let config = Config::load(config_path).context("loading the configuration")?;
    println!("Verifying the default provider through the model gateway...");
    let check = setup::verify_default_provider(&config, vault.store()).await;
    print!(
        "{}",
        Readiness {
            checks: vec![check]
        }
        .render()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `openspine chat` holds the data-root lock for as long as it runs, and its
    /// own first-run notice tells the owner to run `openspine setup`. Refusing
    /// outright would make following that advice from a second terminal fail
    /// instead of explaining anything.
    #[test]
    fn a_held_lock_degrades_to_a_reportable_vault_rather_than_an_error() {
        let dir = std::env::temp_dir().join(format!("openspine-locked-{}", ulid::Ulid::new()));
        let data_dir = dir.join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let config_path = config_with_data_dir(&dir);
        let key = "aa11bb22cc33dd44ee55ff6600778899aa11bb22cc33dd44ee55ff6600778899";
        std::env::set_var("OPENSPINE_ARTIFACT_KEY", key);
        let held = overlay_export_restore::acquire(&data_dir, &[0xaa; 32]).expect("hold the lock");

        let vault = open_vault(&config_path).expect("must not error");

        assert!(matches!(vault, Vault::Locked));
        assert!(vault.store().is_none());
        assert!(!vault.writable());
        assert!(
            vault.write_refusal().contains("another OpenSpine instance"),
            "{}",
            vault.write_refusal()
        );
        drop(held);
    }

    /// Why [`refresh_vault`] refuses to reopen an open vault: the data-root
    /// lock is exclusive even against the process already holding it, so a
    /// reopen after every login would fail the whole wizard.
    #[test]
    fn the_data_root_lock_cannot_be_acquired_twice() {
        let dir = std::env::temp_dir().join(format!("openspine-lock-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();

        let held = overlay_export_restore::acquire(&dir, &[3u8; 32]).expect("first acquire");
        let second = overlay_export_restore::acquire(&dir, &[3u8; 32]);

        assert!(second.is_err(), "the lock must be exclusive");
        drop(held);
    }

    fn config_with_data_dir(dir: &Path) -> PathBuf {
        let config_path = dir.join("openspine.yaml");
        std::fs::write(
            &config_path,
            format!(
                "data_dir: {}\nsandbox:\n  driver: process\nowner:\n  telegram_user_id: 1\n  \
                 display_name: o\nspend_cap: {{}}\n",
                dir.join("data").display()
            ),
        )
        .unwrap();
        config_path
    }

    /// A fresh data root may be keyed freely.
    #[test]
    fn a_new_data_root_reports_no_encrypted_state() {
        let dir = std::env::temp_dir().join(format!("openspine-fresh-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(dir.join("data")).unwrap();
        let config_path = config_with_data_dir(&dir);

        assert_eq!(encrypted_state_in(&config_path), None);
    }

    /// An empty vault directory holds nothing to orphan, so it must not block
    /// key generation.
    #[test]
    fn an_empty_vault_directory_reports_no_encrypted_state() {
        let dir = std::env::temp_dir().join(format!("openspine-empty-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(dir.join("data").join("credentials")).unwrap();
        let config_path = config_with_data_dir(&dir);

        assert_eq!(encrypted_state_in(&config_path), None);
    }

    /// Minting a fresh artifact key over a populated data root does not fail
    /// loudly: it silently makes every stored credential undecryptable.
    #[test]
    fn a_populated_data_root_is_detected_so_no_new_artifact_key_is_minted() {
        let dir = std::env::temp_dir().join(format!("openspine-keyed-{}", ulid::Ulid::new()));
        let credentials = dir.join("data").join("credentials");
        std::fs::create_dir_all(&credentials).unwrap();
        std::fs::write(credentials.join("provider.anthropic.access_token"), b"x").unwrap();
        let config_path = config_with_data_dir(&dir);

        assert_eq!(encrypted_state_in(&config_path), Some(credentials));
    }

    /// `kernel.db` is a file, not a directory: existence alone is the signal.
    #[test]
    fn an_existing_kernel_database_is_detected() {
        let dir = std::env::temp_dir().join(format!("openspine-db-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::write(dir.join("data").join("kernel.db"), b"sqlite").unwrap();
        let config_path = config_with_data_dir(&dir);

        assert_eq!(
            encrypted_state_in(&config_path),
            Some(dir.join("data").join("kernel.db"))
        );
    }

    #[test]
    fn refresh_leaves_an_unavailable_vault_unavailable_without_a_configuration() {
        let mut vault = Vault::Unavailable;

        refresh_vault(&mut vault, Path::new("/nonexistent/openspine.yaml")).unwrap();

        assert!(vault.store().is_none());
    }
}

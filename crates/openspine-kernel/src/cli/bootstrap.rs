//! Non-interactive first-run bootstrap helpers shared by the interactive
//! `openspine setup` wizard and the single-command `openspine init` flow.
//!
//! The key-material safety rules live here so both surfaces enforce them
//! identically: a fresh install may mint keys freely, but minting a new
//! artifact key over a populated data root silently orphans the credential
//! vault, so that is refused with a named recovery instead.

use crate::cli::readiness;
use crate::cli::starter;
use crate::config::Config;
use crate::env_file;
use std::path::{Path, PathBuf};

/// The names of the key-material variables the environment does not yet define.
///
/// Reads through the injected lookup so callers stay testable without touching
/// the ambient process environment.
pub fn missing_key_names(env: readiness::EnvLookup<'_>) -> Vec<String> {
    starter::key_entries(env)
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| env(name).filter(|value| !value.trim().is_empty()).is_none())
        .collect()
}

/// Refuse to mint a fresh artifact key over a populated data root.
///
/// The artifact key encrypts the credential vault, the artifact store, and the
/// counterparty key ring. Minting a new one over existing state does not fail
/// loudly: it silently makes every stored credential undecryptable. Refuse, and
/// name the recovery.
pub fn guard_orphan_artifact_key(config_path: &Path, missing: &[String]) -> anyhow::Result<()> {
    if !missing.iter().any(|name| name == "OPENSPINE_ARTIFACT_KEY") {
        return Ok(());
    }
    if let Some(state) = encrypted_state_in(config_path) {
        let env_path = env_file::path_for(config_path);
        anyhow::bail!(
            "{} already holds encrypted state but OPENSPINE_ARTIFACT_KEY is not set. \
             Restore the original key into {}: generating a new one would make the \
             existing vault permanently unreadable.",
            state.display(),
            env_path.display()
        );
    }
    Ok(())
}

/// Add every key-material variable the env file does not already define, at
/// mode 0600. Existing entries are kept: the vault on disk is encrypted under
/// the current artifact key, and replacing it would orphan that vault.
///
/// Returns the names that were added.
pub fn add_missing_key_material(config_path: &Path) -> anyhow::Result<Vec<String>> {
    let env_path = env_file::path_for(config_path);
    let entries = starter::key_entries(&readiness::process_env);
    Ok(env_file::merge_owner_only(&env_path, &entries)?)
}

/// The first piece of artifact-key-encrypted state in the configured data root,
/// or `None` for a genuinely new one.
///
/// Uses the configured `data_dir` rather than the overlay controller's canonical
/// root so it needs no lock: this runs before any store or vault is opened.
pub fn encrypted_state_in(config_path: &Path) -> Option<PathBuf> {
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

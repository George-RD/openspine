//! Deterministic readiness assessment for an OpenSpine install.
//!
//! One assessment backs three surfaces: `openspine setup`, `openspine setup
//! --check`, and the notice `openspine chat` prints at first start. Every
//! failing check carries the command, variable, or file that resolves it,
//! because a checklist without remedies only relocates the confusion.
//!
//! The environment is read through an injected closure rather than
//! `std::env::var`, so checks are testable without mutating process-global
//! state and without serializing tests against each other.

use crate::config::{Config, ProviderAuth, ProviderConfig};
use crate::env_file;
use crate::secret_store::SecretStore;
use std::path::{Path, PathBuf};

/// Environment lookup, injected so tests never touch the ambient environment.
pub type EnvLookup<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Reads the real process environment.
pub fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    Pass,
    Warn,
    Fail,
}

impl CheckState {
    fn tag(self) -> &'static str {
        match self {
            Self::Pass => "ok",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Check {
    pub id: String,
    pub label: String,
    pub state: CheckState,
    pub detail: String,
    pub remedy: Option<String>,
}

impl Check {
    fn new(
        id: &str,
        label: &str,
        state: CheckState,
        detail: String,
        remedy: Option<String>,
    ) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            state,
            // Details quote provider error bodies, which arrive with embedded
            // newlines. One check is one line, so the report stays greppable.
            detail: detail.split_whitespace().collect::<Vec<_>>().join(" "),
            remedy,
        }
    }

    pub fn pass(id: &str, label: &str, detail: String) -> Self {
        Self::new(id, label, CheckState::Pass, detail, None)
    }

    pub fn warn(id: &str, label: &str, detail: String, remedy: String) -> Self {
        Self::new(id, label, CheckState::Warn, detail, Some(remedy))
    }

    pub fn fail(id: &str, label: &str, detail: String, remedy: String) -> Self {
        Self::new(id, label, CheckState::Fail, detail, Some(remedy))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Readiness {
    pub checks: Vec<Check>,
}

impl Readiness {
    /// True when nothing blocks a governed turn. Warnings do not block.
    pub fn is_ready(&self) -> bool {
        !self
            .checks
            .iter()
            .any(|check| check.state == CheckState::Fail)
    }

    pub fn blocking(&self) -> impl Iterator<Item = &Check> {
        self.checks
            .iter()
            .filter(|check| check.state == CheckState::Fail)
    }

    /// The full checklist.
    pub fn render(&self) -> String {
        render_checks(self.checks.iter())
    }

    /// Only the checks that block a governed turn.
    pub fn render_blocking(&self) -> String {
        render_checks(self.blocking())
    }
}

/// Renders as `[state] id  label: detail`. The id is shown, not just the human
/// label, so `openspine setup --check` output is greppable from a script.
fn render_checks<'a>(checks: impl Iterator<Item = &'a Check>) -> String {
    let mut out = String::new();
    for check in checks {
        out.push_str(&format!(
            "  [{:<4}] {:<20} {}: {}\n",
            check.state.tag(),
            check.id,
            check.label,
            check.detail
        ));
        if let Some(remedy) = &check.remedy {
            out.push_str(&format!("           remedy: {remedy}\n"));
        }
    }
    out
}

/// Assess an install.
///
/// `vault` is optional: without key material the credential vault cannot be
/// opened, and OAuth credential state is then reported as unchecked rather than
/// guessed.
pub fn assess(config_path: &Path, env: EnvLookup<'_>, vault: Option<&SecretStore>) -> Readiness {
    let mut checks = Vec::new();
    let env_file_path = env_file::path_for(config_path);

    let config = match Config::load(config_path) {
        Ok(config) => {
            checks.push(Check::pass(
                "config",
                "configuration",
                format!("{} parses", config_path.display()),
            ));
            Some(config)
        }
        Err(error) => {
            let missing = !config_path.exists();
            checks.push(Check::fail(
                "config",
                "configuration",
                if missing {
                    format!("{} does not exist", config_path.display())
                } else {
                    format!("{error}")
                },
                if missing {
                    "run `openspine setup` to write a starter configuration".to_string()
                } else {
                    format!("fix the YAML in {}", config_path.display())
                },
            ));
            None
        }
    };

    checks.extend(key_checks(env, &env_file_path));

    let Some(config) = config else {
        return Readiness { checks };
    };

    checks.push(package_check(&config, config_path));
    checks.extend(data_dir_check(&config, config_path));

    if config.providers.is_empty() {
        checks.push(Check::fail(
            "providers",
            "model providers",
            "no providers configured".to_string(),
            format!("add a `providers:` entry to {}", config_path.display()),
        ));
        return Readiness { checks };
    }

    for provider in &config.providers {
        checks.push(provider_check(provider, env, vault, &env_file_path));
        checks.extend(model_check(provider, config_path));
    }
    Readiness { checks }
}

/// The three environment keys startup reads.
///
/// All three block. An absent grant key is not a warning: `pipeline::driver`
/// denies every turn without one, so the install would accept messages and
/// answer none of them.
fn key_checks(env: EnvLookup<'_>, env_file_path: &Path) -> Vec<Check> {
    const KEYS: [(&str, &str, &str); 3] = [
        ("key.artifact", "artifact key", "OPENSPINE_ARTIFACT_KEY"),
        ("key.grant", "grant signing key", "OPENSPINE_GRANT_HMAC_KEY"),
        (
            "key.webhook",
            "webhook signing key",
            "OPENSPINE_WEBHOOK_HMAC_KEY",
        ),
    ];

    KEYS.iter()
        .map(|(id, label, name)| {
            let Some(value) = env(name).filter(|value| !value.trim().is_empty()) else {
                return Check::fail(
                    id,
                    label,
                    format!("{name} is not set"),
                    format!(
                        "run `openspine setup`, or add {name} to {}",
                        env_file_path.display()
                    ),
                );
            };
            if *name == "OPENSPINE_ARTIFACT_KEY" && !is_hex_key(&value) {
                return Check::fail(
                    id,
                    label,
                    format!("{name} is not 64 hexadecimal characters"),
                    format!("set {name} to 64 hexadecimal characters (32 bytes for AES-256-GCM)"),
                );
            }
            Check::pass(id, label, format!("{name} is set"))
        })
        .collect()
}

fn is_hex_key(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The agent package directory. A stale path here is exactly what an upgrade
/// produces when a configuration captured the previous install's location, and
/// a relative path is what a configuration written from another directory
/// produces: it resolves against whatever the current directory happens to be.
fn package_check(config: &Config, config_path: &Path) -> Check {
    let resolved = absolute(&config.lyra_dir);
    if resolved.is_dir() {
        return Check::pass(
            "package",
            "agent package",
            format!("{} is present", resolved.display()),
        );
    }
    let relative = config.lyra_dir.is_relative();
    let detail = if relative {
        format!(
            "`lyra_dir: {}` is relative to the current directory and resolves to {}, which is not a directory",
            config.lyra_dir.display(),
            resolved.display()
        )
    } else {
        format!("{} is not a directory", resolved.display())
    };
    let remedy = match bundled_package_dir() {
        Some(bundled) if bundled != resolved => format!(
            "set `lyra_dir: {}` in {}",
            bundled.display(),
            config_path.display()
        ),
        _ => format!(
            "set `lyra_dir` in {} to the absolute path of the directory that holds the Lyra \
             agent package (this build found no bundled package)",
            config_path.display()
        ),
    };
    Check::fail("package", "agent package", detail, remedy)
}

/// Refuse to start a runtime whose agent package directory is missing.
///
/// The artifact loader treats an absent directory as an empty registry, so
/// without this a bad `lyra_dir` boots cleanly and then denies every message
/// with no hint about why.
pub fn require_package(config: &Config, config_path: &Path) -> anyhow::Result<()> {
    let check = package_check(config, config_path);
    if check.state == CheckState::Fail {
        anyhow::bail!("cannot start without the agent package: {}", check.detail);
    }
    Ok(())
}

/// `path` made absolute against the current directory, without touching the
/// filesystem. Falls back to the input if the current directory is unreadable.
pub fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Warns when `data_dir` is relative: a different working directory then
/// silently selects a different state root, and with it a different vault,
/// owner binding, and audit trail.
fn data_dir_check(config: &Config, config_path: &Path) -> Option<Check> {
    if !config.data_dir.is_relative() {
        return None;
    }
    Some(Check::warn(
        "data_dir",
        "state directory",
        format!(
            "`data_dir: {}` is relative, so it resolves against the current directory ({})",
            config.data_dir.display(),
            absolute(&config.data_dir).display()
        ),
        format!(
            "set `data_dir` in {} to an absolute path so every directory sees the same state",
            config_path.display()
        ),
    ))
}

/// A provider with no model id cannot answer: the endpoint rejects the request
/// (`HTTP 400 model is required`) on the first turn.
fn model_check(provider: &ProviderConfig, config_path: &Path) -> Option<Check> {
    if !provider.model.trim().is_empty() {
        return None;
    }
    Some(Check::fail(
        &format!("model.{}", provider.id),
        &format!("provider {} model", provider.id),
        "no model is configured".to_string(),
        format!(
            "run `openspine setup` and choose a model, or set `model:` for provider {} in {}",
            provider.id,
            config_path.display()
        ),
    ))
}

/// The packaged agent directory shipped with, or built alongside, this binary.
///
/// Always absolute, so a configuration that records it keeps working from any
/// directory. Candidates, in order: the install prefix of the running
/// executable (`<prefix>/share/openspine/lyra`, tracking upgrades),
/// `artifacts/lyra` under the current directory (the checkout the owner is
/// standing in, which is what the relative default always meant), and the
/// checkout this binary was built from.
pub fn bundled_package_dir() -> Option<PathBuf> {
    let packaged = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().and_then(Path::parent).map(Path::to_path_buf))
        .map(|prefix| prefix.join("share").join("openspine").join("lyra"));
    let built_from =
        std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra"))
            .ok();
    [
        packaged,
        Some(absolute(Path::new("artifacts/lyra"))),
        built_from,
    ]
    .into_iter()
    .flatten()
    .find(|dir| dir.is_dir())
}

/// [`bundled_package_dir`], or the absolute path the package is expected at
/// when none exists yet.
pub fn default_package_dir() -> PathBuf {
    bundled_package_dir().unwrap_or_else(|| absolute(Path::new("artifacts/lyra")))
}

/// Whether a provider can authenticate. Reports presence only: no token, key,
/// or secret value reaches the rendered output.
fn provider_check(
    provider: &ProviderConfig,
    env: EnvLookup<'_>,
    vault: Option<&SecretStore>,
    env_file_path: &Path,
) -> Check {
    let id = format!("provider.{}", provider.id);
    let label = format!("provider {}", provider.id);
    match &provider.auth {
        ProviderAuth::ApiKey { env: name } => {
            match env(name).filter(|value| !value.trim().is_empty()) {
                Some(_) => Check::pass(&id, &label, format!("{name} is set")),
                None => Check::fail(
                    &id,
                    &label,
                    format!("{name} is not set"),
                    format!("add {name} to {}", env_file_path.display()),
                ),
            }
        }
        ProviderAuth::Oauth => oauth_provider_check(&id, &label, &provider.id, vault),
    }
}

fn oauth_provider_check(
    id: &str,
    label: &str,
    provider_id: &str,
    vault: Option<&SecretStore>,
) -> Check {
    let login = format!("run `openspine provider login {provider_id}`");
    let Some(vault) = vault else {
        return Check::warn(
            id,
            label,
            "OAuth credential state not checked (no key material)".to_string(),
            "resolve the key material checks above, then re-run".to_string(),
        );
    };
    match vault.get_oauth_tokens(provider_id) {
        Ok(Some(tokens)) if tokens.disabled => Check::fail(
            id,
            label,
            "stored OAuth credential is disabled".to_string(),
            login,
        ),
        Ok(Some(tokens)) => Check::pass(
            id,
            label,
            match tokens.account_email {
                Some(email) => format!("OAuth credential stored for {email}"),
                None => "OAuth credential stored".to_string(),
            },
        ),
        Ok(None) => Check::fail(id, label, "no stored OAuth credential".to_string(), login),
        Err(error) => Check::fail(
            id,
            label,
            format!("credential vault unreadable: {error}"),
            login,
        ),
    }
}

#[cfg(test)]
#[path = "readiness_tests.rs"]
mod tests;

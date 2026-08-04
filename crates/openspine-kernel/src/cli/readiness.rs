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
/// produces when a configuration captured the previous install's location.
fn package_check(config: &Config, config_path: &Path) -> Check {
    if config.lyra_dir.is_dir() {
        return Check::pass(
            "package",
            "agent package",
            format!("{} is present", config.lyra_dir.display()),
        );
    }
    Check::fail(
        "package",
        "agent package",
        format!("{} is not a directory", config.lyra_dir.display()),
        format!(
            "set `lyra_dir: {}` in {}",
            default_package_dir().display(),
            config_path.display()
        ),
    )
}

/// The packaged agent directory for the running executable.
///
/// Resolved from `current_exe` so it tracks the running binary. A configuration
/// that captured an absolute install path at first run points at the previous
/// generation after an upgrade. A development build with no packaged directory
/// falls back to the in-repo fixtures.
pub fn default_package_dir() -> PathBuf {
    let packaged = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().and_then(Path::parent).map(Path::to_path_buf))
        .map(|prefix| prefix.join("share").join("openspine").join("lyra"));
    match packaged {
        Some(dir) if dir.is_dir() => dir,
        _ => PathBuf::from("artifacts/lyra"),
    }
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
        ProviderAuth::Oauth => oauth_provider_check(&id, &label, &provider.id, env, vault),
    }
}

/// The env var naming a provider's registered OAuth client id.
fn client_id_env_for(provider_id: &str) -> Option<&'static str> {
    crate::oauth::providers::get_provider_spec(provider_id).map(|spec| spec.client_id_env)
}

fn oauth_provider_check(
    id: &str,
    label: &str,
    provider_id: &str,
    env: EnvLookup<'_>,
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
        // A stored credential whose client id is gone cannot be renewed: the
        // background refresher needs the same client id the authorization used,
        // so this credential dies at its next expiry.
        Ok(Some(_))
            if client_id_env_for(provider_id)
                .and_then(|name| env(name).filter(|value| !value.trim().is_empty()))
                .is_none() =>
        {
            let name = client_id_env_for(provider_id).unwrap_or("the client id variable");
            Check::fail(
                id,
                label,
                format!("OAuth credential stored, but {name} is not set"),
                format!("set {name} so the stored credential can be renewed"),
            )
        }
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

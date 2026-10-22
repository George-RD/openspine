//! Starter configuration and key material for an install that has neither.
//!
//! The template is embedded rather than copied from the repository: `flake.nix`
//! installs only `artifacts/lyra` into `share/openspine/lyra`, so a packaged
//! binary has no repo-root file to read.

use crate::cli::readiness::{self, EnvLookup};
use crate::config::{
    Config, KernelBindConfig, OwnerConfig, ProviderAuth, ProviderConfig, ProviderKind,
    SandboxConfig, SandboxDriverKind, SpendCapConfig,
};
use crate::env_file;
use rand::Rng as _;
use std::net::TcpListener;
use std::path::{Path, PathBuf};

/// The local OpenAI-compatible endpoint most self-hosted installs start from.
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434";
/// Bind port the kernel uses when nothing else is configured.
pub const DEFAULT_BIND_PORT: u16 = 7777;
/// Env var the starter provider reads its (often ignored) API key from.
pub const DEFAULT_API_KEY_ENV: &str = "OPENSPINE_LOCAL_API_KEY";

/// Everything the owner chose during bootstrap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterConfig {
    pub data_dir: PathBuf,
    pub display_name: String,
    /// The owner's Telegram user id — the single trusted-principal channel
    /// binding written to `owner.telegram_user_id`. The kernel binds it as the
    /// authoritative owner principal on the *first* boot
    /// (`bootstrap_owner_principal`), fail-closed: once bound, changing the id
    /// makes the kernel refuse to start. So it is always the owner's real id,
    /// asked for up front, never a placeholder.
    pub telegram_user_id: i64,
    pub provider_id: String,
    pub base_url: String,
    /// Must be non-empty by the time [`write`] runs: an empty model id makes
    /// the endpoint reject every request.
    pub model: String,
    pub bind_addr: String,
    pub package_dir: PathBuf,
}

impl StarterConfig {
    /// Defaults for a configuration at `config_path`, before the owner adjusts
    /// them. `model` is deliberately empty: model ids move independently of
    /// this binary, so the owner names one (or picks one the endpoint serves).
    ///
    /// Every path is absolute. A relative `data_dir` or package directory
    /// resolves against the current directory, so the same file would select a
    /// different state root, or no package at all, from anywhere else.
    pub fn defaults(config_path: &Path, telegram_user_id: i64) -> Self {
        let dir = readiness::absolute(config_path)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"));
        Self {
            data_dir: dir.join("data"),
            telegram_user_id,
            display_name: std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "owner".to_string()),
            provider_id: "local".to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
            model: String::new(),
            bind_addr: format!("127.0.0.1:{}", free_port_from(DEFAULT_BIND_PORT)),
            package_dir: readiness::default_package_dir(),
        }
    }

    /// The [`Config`] this starter describes.
    ///
    /// Building the real struct and letting `serde_yaml` emit it is what keeps
    /// prompt-supplied values safe: a display name containing a quote, a colon,
    /// or a newline would corrupt or inject into hand-spliced YAML, and the
    /// serializer quotes and escapes every scalar for us. It also guarantees
    /// the output parses back through `Config::load`, since that is the exact
    /// type being written.
    pub fn to_config(&self) -> Config {
        Config {
            data_dir: self.data_dir.clone(),
            sandbox: SandboxConfig {
                driver: SandboxDriverKind::Process,
                docker_image: None,
                docker_network: None,
            },
            owner: OwnerConfig {
                telegram_user_id: self.telegram_user_id,
                display_name: self.display_name.clone(),
            },
            providers: vec![ProviderConfig {
                id: self.provider_id.clone(),
                kind: ProviderKind::OpenaiCompat,
                base_url: Some(self.base_url.clone()),
                model: self.model.clone(),
                auth: ProviderAuth::ApiKey {
                    env: DEFAULT_API_KEY_ENV.to_string(),
                },
            }],
            unsafe_allow_uncontained_private_data: false,
            spend_cap: SpendCapConfig {
                model_calls_per_day: 100,
                connector_calls_per_day: 100,
            },
            kernel: KernelBindConfig {
                bind_addr: self.bind_addr.clone(),
                advertise_endpoint: None,
            },
            lyra_dir: self.package_dir.clone(),
            gmail: None,
            model_tiers: Default::default(),
            reflection_miner_interval_seconds: 300,
        }
    }

    pub fn render(&self) -> Result<String, serde_yaml::Error> {
        Ok(format!(
            "# Written by `openspine init`.\n\
             # Secrets stay in {} beside this file, at mode 0600.\n\
             # `sandbox.driver: process` runs task workers with your own\n\
             # privileges; use the Docker deployment for contained workers.\n\
             {}",
            env_file::ENV_FILE_NAME,
            serde_yaml::to_string(&self.to_config())?
        ))
    }
}

/// Key material for a new install: whatever the environment already supplies,
/// and fresh values for the rest.
///
/// Carrying an already-set value into the file is what keeps the install
/// self-consistent. Generating a competing random value instead would work for
/// the current process, where the environment wins, and then silently take over
/// on the next invocation, decrypting an existing vault with the wrong key.
///
/// The local provider's key is included because an OpenAI-compatible server such
/// as Ollama ignores the bearer token, but the gateway still needs the variable
/// to resolve.
pub fn key_entries(env: EnvLookup<'_>) -> Vec<(String, String)> {
    [
        ("OPENSPINE_ARTIFACT_KEY", None),
        ("OPENSPINE_GRANT_HMAC_KEY", None),
        ("OPENSPINE_WEBHOOK_HMAC_KEY", None),
        (DEFAULT_API_KEY_ENV, Some("local")),
    ]
    .into_iter()
    .map(|(name, fixed)| {
        let value = env(name)
            .filter(|value| !value.trim().is_empty())
            .or_else(|| fixed.map(str::to_string))
            .unwrap_or_else(hex_key);
        (name.to_string(), value)
    })
    .collect()
}

fn hex_key() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The first free loopback port at or above `start`, falling back to `start`
/// when the whole probed range is taken.
pub fn free_port_from(start: u16) -> u16 {
    (start..start.saturating_add(16))
        .find(|port| TcpListener::bind(("127.0.0.1", *port)).is_ok())
        .unwrap_or(start)
}

/// Model ids an OpenAI-compatible endpoint reports, so the owner picks a real
/// one instead of a model string this binary guessed.
pub async fn discover_models(client: &reqwest::Client, base_url: &str) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct Entry {
        id: String,
    }
    #[derive(serde::Deserialize)]
    struct Listing {
        data: Vec<Entry>,
    }

    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let Ok(response) = client
        .get(url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    else {
        return Vec::new();
    };
    match response.json::<Listing>().await {
        Ok(listing) => listing.data.into_iter().map(|entry| entry.id).collect(),
        Err(_) => Vec::new(),
    }
}

/// The model to configure: the one the owner named, or the only one the
/// endpoint serves. Several served models are a choice only the owner can make,
/// and none means the endpoint is unreachable or serves nothing, so either way
/// the answer is an error naming the next step rather than a guess.
pub fn resolve_model(
    requested: Option<&str>,
    discovered: &[String],
    base_url: &str,
) -> anyhow::Result<String> {
    if let Some(model) = requested.map(str::trim).filter(|model| !model.is_empty()) {
        return Ok(model.to_string());
    }
    match discovered {
        [only] => Ok(only.clone()),
        [] => anyhow::bail!(
            "no model was named and {base_url} did not list any. Pass `--model <id>`, or start \
             the endpoint and re-run so its models can be discovered."
        ),
        several => anyhow::bail!(
            "{base_url} serves several models ({}). Pass `--model <id>` to choose one.",
            several.join(", ")
        ),
    }
}

/// Set the model of the provider the kernel routes to (the first one) in an
/// existing configuration.
///
/// Edits the parsed configuration and rewrites it, the same way provider
/// binding does, so the new value is serialised and cannot inject YAML.
pub fn set_default_model(config_path: &Path, model: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!model.trim().is_empty(), "the model id must not be empty");
    let mut config = Config::load(config_path)?;
    let provider = config
        .providers
        .first_mut()
        .ok_or_else(|| anyhow::anyhow!("{} configures no providers", config_path.display()))?;
    provider.model = model.trim().to_string();
    std::fs::write(config_path, serde_yaml::to_string(&config)?)?;
    Ok(())
}

/// Write the starter configuration and, when absent, an owner-only key file.
///
/// Returns the generated entries so the caller can export them into the running
/// process. An existing key file is never regenerated: the credential vault is
/// encrypted under the current artifact key, and a new one would orphan it.
pub fn write(
    config_path: &Path,
    starter: &StarterConfig,
    env: EnvLookup<'_>,
) -> Result<Vec<(String, String)>, anyhow::Error> {
    anyhow::ensure!(
        !starter.model.trim().is_empty(),
        "a model id is required: an empty `model:` makes every request fail with HTTP 400"
    );
    if let Some(parent) = config_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(&starter.data_dir)?;
    std::fs::write(config_path, starter.render()?)?;

    let env_path = env_file::path_for(config_path);
    if env_path.exists() {
        return Ok(Vec::new());
    }
    let entries = key_entries(env);
    env_file::write_owner_only(&env_path, &env_file::render(&entries))?;
    Ok(entries)
}

#[cfg(test)]
#[path = "starter_tests.rs"]
mod tests;

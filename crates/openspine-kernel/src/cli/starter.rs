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
    pub provider_id: String,
    pub base_url: String,
    pub model: String,
    pub bind_addr: String,
    pub package_dir: PathBuf,
}

impl StarterConfig {
    /// Defaults for a configuration at `config_path`, before the owner adjusts
    /// them. `model` is deliberately empty: model ids move independently of
    /// this binary, so the owner names one.
    pub fn defaults(config_path: &Path) -> Self {
        let dir = config_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            data_dir: dir.join("data"),
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
                telegram_user_id: 1,
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
            reflection_miner_interval_seconds: 300,
        }
    }

    pub fn render(&self) -> Result<String, serde_yaml::Error> {
        Ok(format!(
            "# Written by `openspine setup`.\n\
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
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("openspine-starter-{tag}-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn starter(dir: &Path) -> StarterConfig {
        let mut starter = StarterConfig::defaults(&dir.join("openspine.yaml"));
        starter.model = "test-model:latest".to_string();
        starter
    }

    /// The template is only useful if the kernel's own loader accepts it.
    #[test]
    fn the_starter_configuration_parses_through_the_real_loader() {
        let dir = temp_dir("parse");
        let config_path = dir.join("openspine.yaml");
        let starter = starter(&dir);

        write(&config_path, &starter, &no_env).unwrap();
        let config = Config::load(&config_path).expect("starter config must load");

        assert_eq!(config.data_dir, starter.data_dir);
        assert_eq!(config.lyra_dir, starter.package_dir);
        assert_eq!(config.providers.len(), 1);
        assert_eq!(config.providers[0].model, "test-model:latest");
        assert_eq!(
            config.providers[0].auth,
            ProviderAuth::ApiKey {
                env: DEFAULT_API_KEY_ENV.to_string()
            }
        );
    }

    /// Every value here comes from a terminal prompt. Hand-spliced YAML would
    /// let a quote, a colon, or a newline corrupt the file or inject a key.
    #[test]
    fn prompt_supplied_values_cannot_corrupt_or_inject_yaml() {
        let dir = temp_dir("injection");
        let config_path = dir.join("openspine.yaml");
        let mut starter = starter(&dir);
        starter.display_name =
            "quote\" colon: newline\nunsafe_allow_uncontained_private_data: true".to_string();
        starter.model = "model: with: colons".to_string();
        starter.provider_id = "#comment".to_string();

        write(&config_path, &starter, &no_env).unwrap();
        let config = Config::load(&config_path).expect("must still parse");

        assert_eq!(config.owner.display_name, starter.display_name);
        assert_eq!(config.providers[0].model, "model: with: colons");
        assert_eq!(config.providers[0].id, "#comment");
        assert!(
            !config.unsafe_allow_uncontained_private_data,
            "a newline in a prompt answer must not set another field"
        );
    }

    /// A configuration that captured an install path at first run points at the
    /// previous generation after an upgrade, so the default tracks the running
    /// executable instead.
    #[test]
    fn the_package_directory_is_resolved_from_the_running_executable() {
        let dir = temp_dir("package");
        let resolved = StarterConfig::defaults(&dir.join("openspine.yaml")).package_dir;

        let expected = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().and_then(Path::parent).map(Path::to_path_buf))
            .map(|prefix| prefix.join("share").join("openspine").join("lyra"));
        match expected {
            Some(packaged) if packaged.is_dir() => assert_eq!(resolved, packaged),
            _ => assert_eq!(resolved, PathBuf::from("artifacts/lyra")),
        }
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn artifact_key_of(entries: &[(String, String)]) -> String {
        entries
            .iter()
            .find(|(name, _)| name == "OPENSPINE_ARTIFACT_KEY")
            .map(|(_, value)| value.clone())
            .expect("artifact key")
    }

    #[test]
    fn generated_keys_satisfy_the_artifact_key_shape() {
        let artifact = artifact_key_of(&key_entries(&no_env));

        assert_eq!(artifact.len(), 64);
        assert!(artifact.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(
            artifact,
            artifact_key_of(&key_entries(&no_env)),
            "keys must differ"
        );
    }

    /// Generating a competing random key would work for the current process,
    /// where the environment wins, and then take over on the next invocation and
    /// decrypt an existing vault with the wrong key.
    #[test]
    fn an_environment_supplied_key_is_carried_into_the_file_unchanged() {
        let supplied = "c".repeat(64);
        let env = |name: &str| (name == "OPENSPINE_ARTIFACT_KEY").then(|| supplied.clone());

        let entries = key_entries(&env);

        assert_eq!(artifact_key_of(&entries), supplied);
        assert_ne!(
            entries
                .iter()
                .find(|(name, _)| name == "OPENSPINE_GRANT_HMAC_KEY")
                .map(|(_, value)| value.as_str()),
            Some(supplied.as_str()),
            "an unset key is still generated"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_generated_key_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = temp_dir("keyfile");
        let config_path = dir.join("openspine.yaml");

        let entries = write(&config_path, &starter(&dir), &no_env).unwrap();

        assert_eq!(entries.len(), 4);
        let mode = std::fs::metadata(env_file::path_for(&config_path))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "mode was {mode:04o}");
    }

    /// Rewriting the configuration must never mint new key material: the
    /// existing vault is encrypted under the current artifact key.
    #[test]
    fn an_existing_key_file_is_left_alone() {
        let dir = temp_dir("keep-keys");
        let config_path = dir.join("openspine.yaml");
        let env_path = env_file::path_for(&config_path);
        env_file::write_owner_only(&env_path, "OPENSPINE_ARTIFACT_KEY=existing\n").unwrap();

        let entries = write(&config_path, &starter(&dir), &no_env).unwrap();

        assert!(entries.is_empty());
        assert_eq!(
            std::fs::read_to_string(&env_path).unwrap(),
            "OPENSPINE_ARTIFACT_KEY=existing\n"
        );
    }

    #[test]
    fn the_bind_port_probe_skips_a_port_already_in_use() {
        let held = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let taken = held.local_addr().unwrap().port();

        assert_ne!(free_port_from(taken), taken);
    }
}

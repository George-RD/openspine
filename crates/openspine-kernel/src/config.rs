//! `openspine.yaml` + environment configuration (build plan Step 4a).
//!
//! Secrets never live in `openspine.yaml` itself: the bot token, the
//! artifact encryption key, and provider API keys are all environment
//! variables (design.md "Secret intake" — this slice defers a richer
//! secret-intake flow and documents the shortcut explicitly, see
//! `docs/telegram-setup.md`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Which containment driver spawns the per-task shell (D-025/O-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxDriverKind {
    Process,
    Docker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxConfig {
    pub driver: SandboxDriverKind,
    /// Only meaningful for `driver: docker`.
    #[serde(default)]
    pub docker_image: Option<String>,
    #[serde(default)]
    pub docker_network: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerConfig {
    pub telegram_user_id: i64,
    pub display_name: String,
}

/// `providers.yaml`'s `auth` clause: either a plain API key sourced from an
/// env var, or a future OAuth mode (Step 4c wires only `api_key` for
/// Anthropic/OpenAI-compat; `oauth` is accepted here so config parsing
/// doesn't need to change again once `provider login` lands).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ProviderAuth {
    ApiKey { env: String },
    Oauth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Anthropic,
    OpenaiCompat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub id: String,
    pub kind: ProviderKind,
    #[serde(default)]
    pub base_url: Option<String>,
    /// The exact model identifier to send the provider (e.g. a specific
    /// dated model string). Deliberately not defaulted or hardcoded in
    /// code — model ids change independently of this binary's release
    /// cycle, so the operator names one explicitly in `openspine.yaml`.
    pub model: String,
    pub auth: ProviderAuth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KernelBindConfig {
    pub bind_addr: String,
    /// What the kernel tells the shell to connect to (`KERNEL_ENDPOINT`),
    /// distinct from `bind_addr` (what the kernel itself listens on).
    /// D-032/D-035: under `DockerDriver` the kernel must bind a wildcard
    /// address (e.g. `0.0.0.0:7777`) to be reachable from the shell's
    /// container on the compose-internal network, but `0.0.0.0` is not a
    /// connectable destination — the shell needs the compose service DNS
    /// name instead (e.g. `http://kernel:7777`). `None` (the `Process`
    /// driver default) derives `http://<bind_addr>`, correct for the
    /// loopback-only dev case where kernel and shell share one host.
    #[serde(default)]
    pub advertise_endpoint: Option<String>,
}

fn default_kernel_bind() -> KernelBindConfig {
    KernelBindConfig {
        bind_addr: "127.0.0.1:7777".to_string(),
        advertise_endpoint: None,
    }
}

fn default_lyra_dir() -> PathBuf {
    PathBuf::from("artifacts/lyra")
}

/// `openspine.yaml` (build plan 4a).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub data_dir: PathBuf,
    pub sandbox: SandboxConfig,
    pub owner: OwnerConfig,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    /// PRD §16 last paragraph (D-025): the kernel refuses to route
    /// `external_communication` events when the active driver is `process`
    /// unless this is explicitly set. Defaults to `false` — the safe state.
    #[serde(default)]
    pub unsafe_allow_uncontained_private_data: bool,
    #[serde(default = "default_kernel_bind")]
    pub kernel: KernelBindConfig,
    /// Where to load the `routes/agents/workflows/packs/policies/templates`
    /// artifact registry from (`artifact_loader::load_registry`). Relative
    /// paths resolve against the process's working directory. Defaults to
    /// the in-repo dev fixtures; a real deploy sets this explicitly.
    #[serde(default = "default_lyra_dir")]
    pub lyra_dir: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("missing required environment variable {0}")]
    MissingEnv(String),
    #[error(
        "{0} must be 64 lowercase hex characters (32 bytes for AES-256-GCM), got {1} characters"
    )]
    InvalidArtifactKey(&'static str, usize),
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        serde_yaml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }
}

/// The required `OPENSPINE_TELEGRAM_BOT_TOKEN` env var.
pub fn telegram_bot_token() -> Result<String, ConfigError> {
    std::env::var("OPENSPINE_TELEGRAM_BOT_TOKEN")
        .map_err(|_| ConfigError::MissingEnv("OPENSPINE_TELEGRAM_BOT_TOKEN".to_string()))
}

/// The required `OPENSPINE_ARTIFACT_KEY` env var: 64 lowercase hex chars
/// (32 raw bytes) for AES-256-GCM.
pub fn artifact_key_bytes() -> Result<[u8; 32], ConfigError> {
    let hex = std::env::var("OPENSPINE_ARTIFACT_KEY")
        .map_err(|_| ConfigError::MissingEnv("OPENSPINE_ARTIFACT_KEY".to_string()))?;
    parse_hex_key(&hex)
}

fn parse_hex_key(hex: &str) -> Result<[u8; 32], ConfigError> {
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ConfigError::InvalidArtifactKey(
            "OPENSPINE_ARTIFACT_KEY",
            hex.len(),
        ));
    }
    let mut bytes = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let s = std::str::from_utf8(chunk).unwrap();
        bytes[i] = u8::from_str_radix(s, 16).unwrap();
    }
    Ok(bytes)
}

/// Resolve one provider's API key from its configured env var.
pub fn provider_api_key(provider: &ProviderConfig) -> Result<String, ConfigError> {
    match &provider.auth {
        ProviderAuth::ApiKey { env } => {
            std::env::var(env).map_err(|_| ConfigError::MissingEnv(env.clone()))
        }
        ProviderAuth::Oauth => Err(ConfigError::MissingEnv(format!(
            "{}: oauth provider login not yet implemented (Step 4c defers `provider login`)",
            provider.id
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_yaml() -> &'static str {
        r#"
data_dir: data
sandbox:
  driver: process
owner:
  telegram_user_id: 123456789
  display_name: George
providers:
  - id: anthropic
    kind: anthropic
    model: placeholder-model-id
    auth:
      mode: api_key
      env: OPENSPINE_ANTHROPIC_API_KEY
unsafe_allow_uncontained_private_data: false
"#
    }

    #[test]
    fn parses_minimal_config() {
        let cfg: Config = serde_yaml::from_str(sample_yaml()).unwrap();
        assert_eq!(cfg.owner.telegram_user_id, 123456789);
        assert_eq!(cfg.sandbox.driver, SandboxDriverKind::Process);
        assert!(!cfg.unsafe_allow_uncontained_private_data);
        assert_eq!(cfg.kernel.bind_addr, "127.0.0.1:7777");
        assert_eq!(cfg.providers.len(), 1);
    }

    #[test]
    fn rejects_unknown_top_level_fields() {
        let mut value: serde_yaml::Value = serde_yaml::from_str(sample_yaml()).unwrap();
        value
            .as_mapping_mut()
            .unwrap()
            .insert("sneaky".into(), "field".into());
        let text = serde_yaml::to_string(&value).unwrap();
        assert!(serde_yaml::from_str::<Config>(&text).is_err());
    }

    #[test]
    fn artifact_key_requires_exactly_64_hex_chars() {
        assert!(parse_hex_key(&"a".repeat(64)).is_ok());
        assert!(parse_hex_key(&"a".repeat(63)).is_err());
        assert!(parse_hex_key(&"z".repeat(64)).is_err());
    }

    #[test]
    fn artifact_key_round_trips_bytes() {
        let hex = "00112233445566778899aabbccddeeff102132435465768798a9bacbdcedfeee";
        let bytes = parse_hex_key(hex).unwrap();
        assert_eq!(bytes[0], 0x00);
        assert_eq!(bytes[1], 0x11);
        assert_eq!(bytes[31], 0xee);
    }

    /// Guards `openspine.example.yaml`/`openspine.docker.example.yaml`
    /// against drifting out of sync with what `Config` actually parses
    /// (`deny_unknown_fields` means a stale example fails loudly here
    /// instead of silently confusing a new operator).
    #[test]
    fn example_configs_parse_against_the_real_schema() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for name in ["openspine.example.yaml", "openspine.docker.example.yaml"] {
            let cfg = Config::load(&repo_root.join(name))
                .unwrap_or_else(|err| panic!("{name} must parse against Config: {err}"));
            assert_eq!(cfg.owner.telegram_user_id, 123456789);
            assert_eq!(cfg.providers.len(), 1);
        }
    }
}

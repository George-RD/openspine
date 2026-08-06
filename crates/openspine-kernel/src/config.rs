//! `openspine.yaml` + environment configuration (build plan Step 4a).
//!
//! Secrets never live in `openspine.yaml` itself: the bot token, the
//! artifact encryption key, and provider API keys are all environment
//! variables (design.md "Secret intake" — this slice defers a richer
//! secret-intake flow and documents the shortcut explicitly, see
//! `docs/telegram-setup.md`).

use std::path::{Path, PathBuf};

use openspine_schemas::digest::{digest_of, Digest};
use serde::{Deserialize, Serialize};
use serde_json::json;

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

/// `openspine.yaml`'s `gmail` block (build plan Step 5 / D-037). `None`
/// when unset — the kernel then refuses `/draft` commands (no connector to
/// serve them) rather than failing to start, since Phase 1's Telegram-only
/// slice must keep working with no Gmail configured at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GmailConfig {
    pub client_id: String,
    /// Env var naming the OAuth client secret (D-014-style secret intake:
    /// never the literal value in `openspine.yaml`).
    pub client_secret_env: String,
    /// Env var naming the long-lived refresh token obtained once via
    /// Google's OAuth consent screen (D-037) — see `docs/gmail-setup.md`.
    pub refresh_token_env: String,
    /// The owner's own Gmail address (D-042) — used to find the correct
    /// reply recipient by skipping the owner's own messages when walking
    /// a thread newest-first. Static and operator-supplied rather than
    /// queried from Gmail on every preview (see D-042's trade-off).
    pub mailbox_address: String,
}
/// AD-143: required global per-day spend cap across all model and connector
/// calls, sitting above per-task grant budgets. Operators choose explicit
/// finite limits; a zero value for either counter is a hard cap of zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpendCapConfig {
    #[serde(default)]
    pub model_calls_per_day: u64,
    #[serde(default)]
    pub connector_calls_per_day: u64,
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
    /// OpenAI Codex subscription OAuth, served by the ChatGPT backend
    /// Responses transport — a different wire contract from
    /// [`ProviderKind::OpenaiCompat`]'s chat-completions client, which is why
    /// the ambiguous state (a Codex credential on a chat-completions client)
    /// is unrepresentable.
    OpenaiCodex,
    Onyx,
    GoogleAntigravity,
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

/// Digest of the non-secret provider configuration a model-swap evidence
/// run evaluated. The auth mode/env name is intentionally excluded: key
/// material never participates in approval identity, while provider id,
/// kind, normalized endpoint, and model do.
pub fn provider_config_digest(provider: &ProviderConfig) -> Digest {
    let default_base = match provider.kind {
        ProviderKind::Anthropic => "https://api.anthropic.com",
        ProviderKind::OpenaiCompat => "https://api.openai.com",
        ProviderKind::OpenaiCodex => "https://chatgpt.com/backend-api",
        ProviderKind::Onyx => "http://127.0.0.1:8080",
        ProviderKind::GoogleAntigravity => "https://generativelanguage.googleapis.com",
    };
    // An OAuth provider receives a first-party client fingerprint the owner
    // never wrote: extra headers and, for Anthropic, a leading system block.
    // Binding its digest here keeps the approval honest, since a swap approval
    // would otherwise cover one request shape while the provider received
    // another. Each OAuth client surface carries its own fingerprint.
    let oauth_fingerprint = provider.is_oauth().then(|| match provider.kind {
        ProviderKind::OpenaiCodex => {
            crate::codex_fingerprint::oauth_fingerprint_digest().to_string()
        }
        _ => crate::anthropic_fingerprint::oauth_fingerprint_digest().to_string(),
    });
    digest_of(&json!({
        "provider_id": provider.id,
        "kind": provider.kind,
        "base_url": provider.base_url.as_deref().unwrap_or(default_base),
        "model": provider.model,
        "oauth_client_fingerprint": oauth_fingerprint,
    }))
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
    /// AD-143: global per-day spend cap across all model and connector calls,
    /// sitting above per-task grant budgets. Required (no implicit "unlimited"
    /// default): a wallet-draining kill-switch must be opted INTO with a real
    /// number, not silently absent. Field defaults keep `spend_cap: {}` valid.
    pub spend_cap: SpendCapConfig,
    #[serde(default = "default_kernel_bind")]
    pub kernel: KernelBindConfig,
    /// Where to load the `routes/agents/workflows/packs/policies/templates`
    /// artifact registry from (`artifact_loader::load_registry`). Relative
    /// paths resolve against the process's working directory. Defaults to
    /// the in-repo dev fixtures; a real deploy sets this explicitly.
    #[serde(default = "default_lyra_dir")]
    pub lyra_dir: PathBuf,
    /// `None` disables the `/draft <thread_id>` selection command entirely
    /// (Step 5 / D-036/D-037) — Phase 1's Telegram-only slice keeps working
    /// with no Gmail connector configured.
    #[serde(default)]
    pub gmail: Option<GmailConfig>,
    /// AD-050/135: interval (seconds) between scheduled reflection-miner
    /// passes. The driver is fail-closed and self-healing (D-047 sweep reaps
    /// expired scheduled grants), so a conservative default is safe.
    #[serde(default = "default_reflection_miner_interval_secs")]
    pub reflection_miner_interval_seconds: u64,
    /// Optional per-reasoning-tier provider routing consumed by the gateway
    /// tier map (AD-046/AD-122's static tier map). Owner-authored
    /// configuration — the same trust root as the provider list itself.
    /// Tiers without a route fall back to the active provider, so an absent
    /// section preserves single-provider behavior. Runtime swaps of the
    /// ACTIVE provider stay behind the AD-152 model-swap ceremony; this only
    /// pins which configured provider serves a declared tier.
    #[serde(default)]
    pub model_tiers: ModelTiersConfig,
}

/// Default interval between scheduled reflection-miner passes: 5 minutes.
fn default_reflection_miner_interval_secs() -> u64 {
    300
}

/// `model_tiers`: reasoning tier -> provider id. Every named provider must
/// exist in `providers`; validation is fail-closed at load.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelTiersConfig {
    #[serde(default)]
    pub low: Option<String>,
    #[serde(default)]
    pub standard: Option<String>,
    #[serde(default)]
    pub high: Option<String>,
}

impl ModelTiersConfig {
    /// The declared routes, named for error reporting.
    pub fn routes(&self) -> [(&'static str, Option<&str>); 3] {
        [
            ("low", self.low.as_deref()),
            ("standard", self.standard.as_deref()),
            ("high", self.high.as_deref()),
        ]
    }
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
    #[error("reflection_miner_interval_seconds must be greater than zero")]
    InvalidReflectionMinerInterval,
    #[error(
        "model_tiers.{tier} routes to unknown provider id '{provider_id}'; every tier route \
         must name an entry in `providers`"
    )]
    UnknownTierProvider {
        tier: &'static str,
        provider_id: String,
    },
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let config: Self = serde_yaml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        config.validate()
    }

    fn validate(self) -> Result<Self, ConfigError> {
        if self.reflection_miner_interval_seconds == 0 {
            return Err(ConfigError::InvalidReflectionMinerInterval);
        }
        // A tier route naming a provider that does not exist would otherwise
        // surface as a silent fallback to the active provider on the first
        // call — the owner asked for a split and quietly did not get one.
        for (tier, route) in self.model_tiers.routes() {
            if let Some(provider_id) = route {
                if !self.providers.iter().any(|p| p.id == provider_id) {
                    return Err(ConfigError::UnknownTierProvider {
                        tier,
                        provider_id: provider_id.to_string(),
                    });
                }
            }
        }
        Ok(self)
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
/// The required `OPENSPINE_WEBHOOK_HMAC_KEY` env var: the shared HMAC-SHA256
/// secret the kernel uses to verify inbound webhook signatures (AD-134/AD-141).
/// Absent in development/test, where a fixed test key is wired directly.
pub fn webhook_hmac_secret() -> Result<Vec<u8>, ConfigError> {
    let secret = std::env::var("OPENSPINE_WEBHOOK_HMAC_KEY")
        .map_err(|_| ConfigError::MissingEnv("OPENSPINE_WEBHOOK_HMAC_KEY".to_string()))?;
    if secret.trim().is_empty() {
        return Err(ConfigError::MissingEnv(
            "OPENSPINE_WEBHOOK_HMAC_KEY".to_string(),
        ));
    }
    Ok(secret.into_bytes())
}

/// Resolve a [`GmailConfig`]'s OAuth client secret from its configured env var.
pub fn gmail_client_secret(cfg: &GmailConfig) -> Result<String, ConfigError> {
    std::env::var(&cfg.client_secret_env)
        .map_err(|_| ConfigError::MissingEnv(cfg.client_secret_env.clone()))
}

/// Resolve a [`GmailConfig`]'s long-lived refresh token from its configured env var.
pub fn gmail_refresh_token(cfg: &GmailConfig) -> Result<String, ConfigError> {
    std::env::var(&cfg.refresh_token_env)
        .map_err(|_| ConfigError::MissingEnv(cfg.refresh_token_env.clone()))
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
        ProviderAuth::Oauth => Ok(format!("oauth:{}", provider.id)),
    }
}

impl ProviderConfig {
    pub fn is_oauth(&self) -> bool {
        matches!(self.auth, ProviderAuth::Oauth)
    }
}

#[cfg(test)]
mod tests;

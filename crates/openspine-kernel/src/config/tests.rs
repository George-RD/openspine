//! Configuration schema, digest, and validation tests. Split from
//! `config.rs` for the 500-line module gate.

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
spend_cap:
  model_calls_per_day: 100
  connector_calls_per_day: 500
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
fn rejects_zero_reflection_miner_interval() {
    let mut cfg: Config = serde_yaml::from_str(sample_yaml()).unwrap();
    cfg.reflection_miner_interval_seconds = 0;
    assert!(matches!(
        cfg.validate(),
        Err(ConfigError::InvalidReflectionMinerInterval)
    ));
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
#[test]
fn terminal_example_config_uses_onyx_lfm_models() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cfg = Config::load(&repo_root.join("openspine.terminal.example.yaml"))
        .expect("terminal example must parse against Config");
    assert_eq!(cfg.owner.telegram_user_id, 1);
    assert_eq!(cfg.providers.len(), 2);
    assert_eq!(cfg.providers[0].id, "onyx-lfm-1-2b");
    assert_eq!(cfg.providers[0].kind, ProviderKind::Onyx);
    assert_eq!(cfg.providers[0].model, "LiquidAI/LFM2.5-1.2B-Instruct");
    assert_eq!(cfg.providers[1].id, "onyx-lfm-350m");
    assert_eq!(cfg.providers[1].kind, ProviderKind::Onyx);
    assert_eq!(cfg.providers[1].model, "LiquidAI/LFM2.5-350M");
}

#[test]
fn config_accepts_provider_auth_oauth_variant() {
    let yaml = r#"
id: google-antigravity
kind: google_antigravity
model: gemini-2.5-flash
auth:
  mode: oauth
"#;
    let provider: ProviderConfig = serde_yaml::from_str(yaml).expect("parse provider oauth config");
    assert!(provider.is_oauth());
    assert_eq!(provider.id, "google-antigravity");
    assert_eq!(provider.kind, ProviderKind::GoogleAntigravity);
    assert_eq!(
        provider_api_key(&provider).unwrap(),
        "oauth:google-antigravity"
    );
}

/// A model-swap approval binds this digest. An OAuth provider receives a
/// first-party client fingerprint the owner never wrote, so the digest has
/// to move with it; otherwise the ceremony approves one request shape while
/// the provider receives another.
#[test]
fn the_oauth_client_fingerprint_participates_in_the_provider_digest() {
    let api_key = ProviderConfig {
        id: "anthropic".to_string(),
        kind: ProviderKind::Anthropic,
        base_url: None,
        model: "claude-sonnet-4-6".to_string(),
        auth: ProviderAuth::ApiKey {
            env: "ANTHROPIC_API_KEY".to_string(),
        },
    };
    let oauth = ProviderConfig {
        auth: ProviderAuth::Oauth,
        ..api_key.clone()
    };

    assert_ne!(
        provider_config_digest(&api_key),
        provider_config_digest(&oauth),
        "the OAuth client surface must be visible to swap approval"
    );
}

const TIERED_CONFIG: &str = r#"
data_dir: d
sandbox:
  driver: process
owner:
  telegram_user_id: 1
  display_name: o
spend_cap: {}
providers:
  - id: anthropic
    kind: anthropic
    model: claude-sonnet-4-6
    auth:
      mode: oauth
  - id: openai-codex
    kind: openai_codex
    model: gpt-5-codex
    auth:
      mode: oauth
model_tiers:
  high: anthropic
  low: openai-codex
"#;

#[test]
fn model_tiers_parse_and_validate_against_the_provider_list() {
    let config: Config = serde_yaml::from_str(TIERED_CONFIG).expect("parse");
    let config = config.validate().expect("tier routes name real providers");
    assert_eq!(config.model_tiers.high.as_deref(), Some("anthropic"));
    assert_eq!(config.model_tiers.low.as_deref(), Some("openai-codex"));
    assert_eq!(config.model_tiers.standard, None);
}

#[test]
fn a_tier_route_to_an_unknown_provider_fails_config_validation() {
    let yaml = TIERED_CONFIG.replace("high: anthropic", "high: no-such-provider");
    let config: Config = serde_yaml::from_str(&yaml).expect("parse");
    let error = config
        .validate()
        .expect_err("a dangling tier route must refuse startup");
    let rendered = error.to_string();
    assert!(rendered.contains("no-such-provider"), "{rendered}");
    assert!(rendered.contains("model_tiers.high"), "{rendered}");
}
#[test]
fn provider_config_digest_handles_oauth_providers() {
    let provider = ProviderConfig {
        id: "google-antigravity".to_string(),
        kind: ProviderKind::GoogleAntigravity,
        base_url: None,
        model: "gemini-2.5-flash".to_string(),
        auth: ProviderAuth::Oauth,
    };
    let digest = provider_config_digest(&provider);
    assert!(!digest.to_string().is_empty());
}

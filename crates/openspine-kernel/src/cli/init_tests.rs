//! Tests for `cli::init`, in a separate file so `init.rs` stays under the
//! 500-line budget.
//!
//! These exercise the pieces that do not mutate the ambient process
//! environment (config/key writing and the owner-immutability guard). The full
//! seed-key -> owner-principal -> readiness loop is covered end-to-end by the
//! `first_run_smoke` integration test, which spawns the binary in an isolated
//! process so its `env_file::load_adjacent` cannot race other tests.

use super::*;
use crate::config::Config;

const URL: &str = "http://127.0.0.1:1";

/// A fresh init writes a configuration bound to the owner's real Telegram id
/// and model, and an owner-only key file carrying the three seed keys.
#[test]
fn fresh_write_captures_owner_model_and_seed_keys() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("openspine.yaml");

    write_fresh(
        &config_path,
        123_456_789,
        Some("Tester"),
        "http://127.0.0.1:9",
        "some-model:q4",
    )
    .unwrap();

    let config = Config::load(&config_path).expect("written config parses");
    assert_eq!(config.owner.telegram_user_id, 123_456_789);
    assert_eq!(config.owner.display_name, "Tester");
    assert_eq!(config.providers[0].model, "some-model:q4");
    assert_eq!(
        config.providers[0].base_url.as_deref(),
        Some("http://127.0.0.1:9")
    );

    let env_text = std::fs::read_to_string(env_file::path_for(&config_path)).unwrap();
    for key in [
        "OPENSPINE_ARTIFACT_KEY",
        "OPENSPINE_GRANT_HMAC_KEY",
        "OPENSPINE_WEBHOOK_HMAC_KEY",
    ] {
        assert!(env_text.contains(key), "seed key file is missing {key}");
    }
}

/// The configuration has to work from any directory, so every path init writes
/// is absolute. (The bare relative `--config` case is covered by the
/// `first_run_smoke` integration test and the starter tests.)
#[test]
fn fresh_write_records_only_absolute_paths() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("openspine.yaml");

    write_fresh(&config_path, 7, None, URL, "m").unwrap();

    let config = Config::load(&config_path).unwrap();
    assert!(
        config.data_dir.is_absolute(),
        "{}",
        config.data_dir.display()
    );
    assert!(
        config.lyra_dir.is_absolute(),
        "{}",
        config.lyra_dir.display()
    );
}

/// A fresh install refuses to proceed without a real owner id: binding a
/// placeholder would trap the owner behind the fail-closed bootstrap.
#[tokio::test]
async fn fresh_init_requires_owner() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("openspine.yaml");

    let options = InitOptions {
        model: Some("m"),
        ..InitOptions::default()
    };
    let error = run_init(&config_path, options).await.unwrap_err();

    assert!(error.to_string().contains("--owner"));
    assert!(
        !config_path.exists(),
        "no configuration is written on refusal"
    );
}

/// A configuration with no model is the install that printed green and then
/// failed its first turn. With no `--model` and an endpoint that lists
/// nothing, a fresh init refuses and writes nothing.
#[tokio::test]
async fn fresh_init_without_a_discoverable_model_is_refused_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("openspine.yaml");

    let options = InitOptions {
        owner: Some(5),
        base_url: Some(URL),
        ..InitOptions::default()
    };
    let error = run_init(&config_path, options).await.unwrap_err();

    let message = error.to_string();
    assert!(message.contains("--model"), "{message}");
    assert!(message.contains(URL), "{message}");
    assert!(!config_path.exists(), "nothing is written on refusal");
    assert!(!env_file::path_for(&config_path).exists());
}

/// `--base-url` belongs to the fresh-install path; silently ignoring it on an
/// existing configuration would leave the owner believing it took effect.
#[tokio::test]
async fn base_url_on_an_existing_configuration_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("openspine.yaml");
    write_fresh(&config_path, 111, None, URL, "m").unwrap();

    let options = InitOptions {
        base_url: Some("http://127.0.0.1:2"),
        ..InitOptions::default()
    };
    let error = run_init(&config_path, options).await.unwrap_err();

    assert!(error.to_string().contains("--base-url"), "{error}");
}

/// `--model` on an existing configuration changes the model, so a config that
/// was written empty (or points at a retired model) is repaired by re-running
/// init rather than by hand-editing YAML.
#[tokio::test]
async fn model_flag_repairs_an_existing_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("openspine.yaml");
    write_fresh(&config_path, 111, None, URL, "old").unwrap();

    settle_model_existing(&config_path, Some("new-model"))
        .await
        .unwrap();

    let config = Config::load(&config_path).unwrap();
    assert_eq!(config.providers[0].model, "new-model");
    assert_eq!(config.owner.telegram_user_id, 111);
}

/// Without `--model`, a configured model is left alone.
#[tokio::test]
async fn an_existing_model_is_kept_when_none_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("openspine.yaml");
    write_fresh(&config_path, 111, None, URL, "keep-me").unwrap();

    settle_model_existing(&config_path, None).await.unwrap();

    let config = Config::load(&config_path).unwrap();
    assert_eq!(config.providers[0].model, "keep-me");
}

/// The owner identity is immutable once written: a conflicting `--owner` is
/// rejected rather than silently rewriting the config into an unbootable state.
#[test]
fn existing_config_rejects_conflicting_owner() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("openspine.yaml");
    write_fresh(&config_path, 111, Some("First"), URL, "m").unwrap();

    let error = prepare_existing(&config_path, Some(222)).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("111"), "names the configured id");
    assert!(message.contains("222"), "names the rejected id");
}

/// Re-running init against an existing config with the same owner is fine: the
/// owner check passes and missing keys are topped up without complaint.
#[test]
fn existing_config_accepts_matching_owner() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("openspine.yaml");
    write_fresh(&config_path, 111, Some("First"), URL, "m").unwrap();

    prepare_existing(&config_path, Some(111)).expect("matching owner is accepted");
}

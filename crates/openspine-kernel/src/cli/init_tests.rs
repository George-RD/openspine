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

/// A fresh init writes a configuration bound to the owner's real Telegram id
/// and an owner-only key file carrying the three seed keys.
#[test]
fn fresh_write_captures_owner_and_seed_keys() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("openspine.yaml");

    write_fresh(&config_path, Some(123_456_789), Some("Tester")).unwrap();

    let config = Config::load(&config_path).expect("written config parses");
    assert_eq!(config.owner.telegram_user_id, 123_456_789);
    assert_eq!(config.owner.display_name, "Tester");

    let env_text = std::fs::read_to_string(env_file::path_for(&config_path)).unwrap();
    for key in [
        "OPENSPINE_ARTIFACT_KEY",
        "OPENSPINE_GRANT_HMAC_KEY",
        "OPENSPINE_WEBHOOK_HMAC_KEY",
    ] {
        assert!(env_text.contains(key), "seed key file is missing {key}");
    }
}

/// A fresh install refuses to proceed without a real owner id: binding a
/// placeholder would trap the owner behind the fail-closed bootstrap.
#[test]
fn fresh_write_requires_owner() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("openspine.yaml");

    let error = write_fresh(&config_path, None, None).unwrap_err();
    assert!(error.to_string().contains("--owner"));
    assert!(
        !config_path.exists(),
        "no configuration is written on refusal"
    );
}

/// The owner identity is immutable once written: a conflicting `--owner` is
/// rejected rather than silently rewriting the config into an unbootable state.
#[test]
fn existing_config_rejects_conflicting_owner() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("openspine.yaml");
    write_fresh(&config_path, Some(111), Some("First")).unwrap();

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
    write_fresh(&config_path, Some(111), Some("First")).unwrap();

    prepare_existing(&config_path, Some(111)).expect("matching owner is accepted");
}

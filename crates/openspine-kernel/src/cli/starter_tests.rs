//! Tests for `cli::starter`, in a separate file so `starter.rs` stays under the
//! 500-line budget.

use super::*;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("openspine-starter-{tag}-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn starter(dir: &Path) -> StarterConfig {
    let mut starter = StarterConfig::defaults(&dir.join("openspine.yaml"), 42);
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
    assert_eq!(config.owner.telegram_user_id, 42);
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
/// executable instead. Whatever it resolves to is absolute.
#[test]
fn the_package_directory_is_absolute_and_tracks_the_running_executable() {
    let dir = temp_dir("package");
    let resolved = StarterConfig::defaults(&dir.join("openspine.yaml"), 42).package_dir;

    assert!(resolved.is_absolute(), "{}", resolved.display());
    let packaged = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().and_then(Path::parent).map(Path::to_path_buf))
        .map(|prefix| prefix.join("share").join("openspine").join("lyra"));
    if let Some(packaged) = packaged.filter(|dir| dir.is_dir()) {
        assert_eq!(resolved, packaged);
    }
}

/// A bare `--config openspine.yaml` has an empty parent. The state directory
/// must still come out absolute, or the same file selects a different state
/// root from every directory it is run in.
#[test]
fn a_bare_relative_config_path_still_yields_an_absolute_data_dir() {
    let starter = StarterConfig::defaults(Path::new("openspine.yaml"), 42);

    assert!(
        starter.data_dir.is_absolute(),
        "{}",
        starter.data_dir.display()
    );
    assert!(starter.package_dir.is_absolute());
    assert_eq!(
        starter.data_dir,
        std::env::current_dir().unwrap().join("data")
    );
}

/// An empty model is the install that prints green and then fails its first
/// turn with `HTTP 400 model is required`, so nothing may write one.
#[test]
fn writing_a_configuration_with_no_model_is_refused() {
    let dir = temp_dir("no-model");
    let config_path = dir.join("openspine.yaml");
    let mut starter = starter(&dir);
    starter.model = "  ".to_string();

    let error = write(&config_path, &starter, &no_env).unwrap_err();

    assert!(error.to_string().contains("model"), "{error}");
    assert!(!config_path.exists(), "nothing is written on refusal");
}

#[test]
fn a_named_model_wins_over_discovery() {
    let served = vec!["a".to_string(), "b".to_string()];
    assert_eq!(
        resolve_model(Some(" b "), &served, "http://x").unwrap(),
        "b"
    );
}

#[test]
fn the_only_served_model_is_chosen_when_none_is_named() {
    let served = vec!["only".to_string()];
    assert_eq!(resolve_model(None, &served, "http://x").unwrap(), "only");
}

/// Choosing among several is the owner's call, and none served means there is
/// nothing to guess from: both name `--model` as the way forward.
#[test]
fn an_ambiguous_or_empty_discovery_asks_for_an_explicit_model() {
    let several = vec!["a".to_string(), "b".to_string()];
    let ambiguous = resolve_model(None, &several, "http://x").unwrap_err();
    assert!(ambiguous.to_string().contains("--model"), "{ambiguous}");
    assert!(ambiguous.to_string().contains("a, b"), "{ambiguous}");

    let none = resolve_model(None, &[], "http://x").unwrap_err();
    assert!(none.to_string().contains("--model"), "{none}");
    assert!(none.to_string().contains("http://x"), "{none}");
}

#[test]
fn setting_the_default_model_rewrites_only_the_first_provider_model() {
    let dir = temp_dir("set-model");
    let config_path = dir.join("openspine.yaml");
    let mut empty = starter(&dir);
    empty.model = "old".to_string();
    write(&config_path, &empty, &no_env).unwrap();

    set_default_model(&config_path, " model: with: colons ").unwrap();

    let config = Config::load(&config_path).unwrap();
    assert_eq!(config.providers[0].model, "model: with: colons");
    assert_eq!(config.owner.telegram_user_id, 42);
    assert!(set_default_model(&config_path, "").is_err());
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

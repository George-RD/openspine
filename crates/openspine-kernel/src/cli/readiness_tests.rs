//! Readiness assessment tests.

use super::*;
use crate::secret_store::{OAuthIdentityMetadata, SecretStore};
use std::collections::HashMap;

const ARTIFACT_KEY: &str = "aa11bb22cc33dd44ee55ff6600778899aa11bb22cc33dd44ee55ff6600778899";

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("openspine-ready-{tag}-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A configuration whose only variable is the provider block, so each test
/// changes exactly the thing it is about.
fn write_config(dir: &Path, providers: &str) -> PathBuf {
    let package = dir.join("package");
    std::fs::create_dir_all(&package).unwrap();
    let path = dir.join("openspine.yaml");
    std::fs::write(
        &path,
        format!(
            "data_dir: {data}\n\
             sandbox:\n  driver: process\n\
             owner:\n  telegram_user_id: 1\n  display_name: owner\n\
             spend_cap:\n  model_calls_per_day: 10\n  connector_calls_per_day: 10\n\
             lyra_dir: {package}\n\
             {providers}",
            data = dir.join("data").display(),
            package = package.display(),
        ),
    )
    .unwrap();
    path
}

fn api_key_provider(env_name: &str) -> String {
    format!(
        "providers:\n  - id: local\n    kind: openai_compat\n    model: m\n    \
         auth:\n      mode: api_key\n      env: {env_name}\n"
    )
}

fn oauth_provider() -> String {
    "providers:\n  - id: anthropic\n    kind: anthropic\n    model: m\n    \
     auth:\n      mode: oauth\n"
        .to_string()
}

/// An environment with every key material variable satisfied, so provider
/// checks are the only thing left that can fail.
fn full_env() -> HashMap<String, String> {
    HashMap::from([
        (
            "OPENSPINE_ARTIFACT_KEY".to_string(),
            ARTIFACT_KEY.to_string(),
        ),
        ("OPENSPINE_GRANT_HMAC_KEY".to_string(), "grant".to_string()),
        ("OPENSPINE_WEBHOOK_HMAC_KEY".to_string(), "hook".to_string()),
    ])
}

fn lookup(map: &HashMap<String, String>) -> impl Fn(&str) -> Option<String> + '_ {
    move |name: &str| map.get(name).cloned()
}

/// Locate one check by its stable id.
fn find<'a>(readiness: &'a Readiness, id: &str) -> Option<&'a Check> {
    readiness.checks.iter().find(|check| check.id == id)
}

fn vault(dir: &Path) -> SecretStore {
    SecretStore::open(dir.join("credentials"), [7u8; 32]).unwrap()
}

#[test]
fn missing_configuration_blocks_and_points_at_setup() {
    let dir = temp_dir("no-config");
    let env = full_env();

    let readiness = assess(&dir.join("openspine.yaml"), &lookup(&env), None);

    assert!(!readiness.is_ready());
    let check = find(&readiness, "config").unwrap();
    assert_eq!(check.state, CheckState::Fail);
    assert!(
        check.remedy.as_deref().unwrap().contains("openspine setup"),
        "{:?}",
        check.remedy
    );
}

#[test]
fn unparseable_configuration_blocks_without_claiming_it_is_absent() {
    let dir = temp_dir("bad-config");
    let path = dir.join("openspine.yaml");
    std::fs::write(&path, "data_dir: [not a path\n").unwrap();
    let env = full_env();

    let readiness = assess(&path, &lookup(&env), None);

    let check = find(&readiness, "config").unwrap();
    assert_eq!(check.state, CheckState::Fail);
    assert!(!check.detail.contains("does not exist"), "{}", check.detail);
    assert!(check.remedy.as_deref().unwrap().contains("fix the YAML"));
}

#[test]
fn each_absent_key_blocks_and_names_the_env_file() {
    let dir = temp_dir("keys");
    let path = write_config(&dir, &api_key_provider("LOCAL_KEY"));
    let env = HashMap::new();

    let readiness = assess(&path, &lookup(&env), None);

    for id in ["key.artifact", "key.grant", "key.webhook"] {
        let check = find(&readiness, id).unwrap();
        assert_eq!(check.state, CheckState::Fail, "{id}");
        assert!(
            check.remedy.as_deref().unwrap().contains("openspine.env"),
            "{id}: {:?}",
            check.remedy
        );
    }
}

/// The grant key gates `pipeline::driver`, which denies every turn without it.
/// Reporting it as a warning would describe a mute assistant as ready.
#[test]
fn an_absent_grant_key_blocks_rather_than_warns() {
    let dir = temp_dir("grant");
    let path = write_config(&dir, &api_key_provider("LOCAL_KEY"));
    let mut env = full_env();
    env.remove("OPENSPINE_GRANT_HMAC_KEY");
    env.insert("LOCAL_KEY".to_string(), "k".to_string());

    let readiness = assess(&path, &lookup(&env), None);

    assert_eq!(
        find(&readiness, "key.grant").unwrap().state,
        CheckState::Fail
    );
    assert!(!readiness.is_ready());
}

#[test]
fn a_malformed_artifact_key_blocks_with_the_expected_shape() {
    let dir = temp_dir("hexkey");
    let path = write_config(&dir, &api_key_provider("LOCAL_KEY"));
    let mut env = full_env();
    env.insert(
        "OPENSPINE_ARTIFACT_KEY".to_string(),
        "too-short".to_string(),
    );
    env.insert("LOCAL_KEY".to_string(), "k".to_string());

    let readiness = assess(&path, &lookup(&env), None);

    let check = find(&readiness, "key.artifact").unwrap();
    assert_eq!(check.state, CheckState::Fail);
    assert!(check.detail.contains("64 hexadecimal"), "{}", check.detail);
}

#[test]
fn an_unset_provider_key_blocks_and_names_the_variable() {
    let dir = temp_dir("apikey");
    let path = write_config(&dir, &api_key_provider("SOME_KEY"));
    let env = full_env();

    let readiness = assess(&path, &lookup(&env), None);

    let check = find(&readiness, "provider.local").unwrap();
    assert_eq!(check.state, CheckState::Fail);
    assert!(check.remedy.as_deref().unwrap().contains("SOME_KEY"));
}

#[test]
fn a_set_provider_key_passes() {
    let dir = temp_dir("apikey-ok");
    let path = write_config(&dir, &api_key_provider("SOME_KEY"));
    let mut env = full_env();
    env.insert("SOME_KEY".to_string(), "value".to_string());

    let readiness = assess(&path, &lookup(&env), None);

    assert!(readiness.is_ready(), "{}", readiness.render());
}

#[test]
fn a_stale_package_directory_blocks_and_names_a_replacement() {
    let dir = temp_dir("package");
    let path = write_config(&dir, &api_key_provider("SOME_KEY"));
    std::fs::remove_dir_all(dir.join("package")).unwrap();
    let mut env = full_env();
    env.insert("SOME_KEY".to_string(), "value".to_string());

    let readiness = assess(&path, &lookup(&env), None);

    let check = find(&readiness, "package").unwrap();
    assert_eq!(check.state, CheckState::Fail);
    assert!(check.remedy.as_deref().unwrap().contains("lyra_dir"));
}

#[test]
fn an_oauth_provider_without_a_credential_blocks_and_names_the_login() {
    let dir = temp_dir("oauth-missing");
    let path = write_config(&dir, &oauth_provider());
    let env = full_env();
    let store = vault(&dir);

    let readiness = assess(&path, &lookup(&env), Some(&store));

    let check = find(&readiness, "provider.anthropic").unwrap();
    assert_eq!(check.state, CheckState::Fail);
    assert_eq!(
        check.remedy.as_deref(),
        Some("run `openspine provider login anthropic`")
    );
}

#[test]
fn a_disabled_oauth_credential_blocks() {
    let dir = temp_dir("oauth-disabled");
    let path = write_config(&dir, &oauth_provider());
    let env = full_env();
    let store = vault(&dir);
    store
        .store_oauth_tokens("anthropic", "refresh", "access", "9999999999", None)
        .unwrap();
    store
        .disable_oauth_credential("anthropic", "revoked")
        .unwrap();

    let readiness = assess(&path, &lookup(&env), Some(&store));

    let check = find(&readiness, "provider.anthropic").unwrap();
    assert_eq!(check.state, CheckState::Fail);
    assert!(check.detail.contains("disabled"), "{}", check.detail);
}

#[test]
fn a_stored_oauth_credential_passes_and_names_the_account() {
    let dir = temp_dir("oauth-ok");
    let path = write_config(&dir, &oauth_provider());
    let env = full_env();
    let store = vault(&dir);
    store
        .store_oauth_tokens(
            "anthropic",
            "refresh",
            "access",
            "9999999999",
            Some(OAuthIdentityMetadata {
                account_email: Some("owner@example.com".to_string()),
                ..OAuthIdentityMetadata::default()
            }),
        )
        .unwrap();

    let readiness = assess(&path, &lookup(&env), Some(&store));

    assert!(readiness.is_ready(), "{}", readiness.render());
    assert!(find(&readiness, "provider.anthropic")
        .unwrap()
        .detail
        .contains("owner@example.com"));
}

/// The report is printed to a terminal and pasted into issues. Nothing that
/// authenticates the owner may travel with it.
#[test]
fn the_rendered_report_never_contains_token_material() {
    let dir = temp_dir("secrets");
    let path = write_config(&dir, &oauth_provider());
    let env = full_env();
    let store = vault(&dir);
    store
        .store_oauth_tokens(
            "anthropic",
            "refresh-token-value",
            "access-token-value",
            "9999999999",
            None,
        )
        .unwrap();

    let rendered = assess(&path, &lookup(&env), Some(&store)).render();

    assert!(!rendered.contains("access-token-value"), "{rendered}");
    assert!(!rendered.contains("refresh-token-value"), "{rendered}");
    assert!(!rendered.contains(ARTIFACT_KEY), "{rendered}");
}

#[test]
fn oauth_state_is_reported_unchecked_when_the_vault_cannot_be_opened() {
    let dir = temp_dir("oauth-novault");
    let path = write_config(&dir, &oauth_provider());
    let env = full_env();

    let readiness = assess(&path, &lookup(&env), None);

    let check = find(&readiness, "provider.anthropic").unwrap();
    assert_eq!(check.state, CheckState::Warn);
    assert!(readiness.is_ready(), "a warning must not block");
}

#[test]
fn render_blocking_shows_only_failures() {
    let dir = temp_dir("render");
    let path = write_config(&dir, &api_key_provider("SOME_KEY"));
    let env = full_env();

    let readiness = assess(&path, &lookup(&env), None);
    let blocking = readiness.render_blocking();

    assert!(blocking.contains("SOME_KEY"), "{blocking}");
    assert!(!blocking.contains("[ok"), "{blocking}");
}

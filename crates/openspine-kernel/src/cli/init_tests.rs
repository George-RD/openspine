//! Tests for `cli::init`, in a separate file so `init.rs` stays under the
//! 500-line budget.
//!
//! Config/key writing and owner-immutability checks do not mutate the ambient
//! process environment. Command-level seed acceptance runs in isolated child
//! processes so `env_file::load_adjacent` cannot race other tests. The
//! `first_run_smoke` integration test also exercises the complete first-run loop.

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

/// Run the real command in an isolated process: loading adjacent keys must not
/// change the environment of concurrently running unit tests.
#[test]
fn init_accepts_shipped_seeds_on_fresh_and_existing_installs() {
    for case in ["fresh", "existing", "invalid-store", "edited-and-deleted"] {
        let dir = tempfile::tempdir().unwrap();
        let test_name = format!(
            "{}::init_seed_acceptance_child",
            module_path!().split_once("::").unwrap().1
        );
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &test_name, "--nocapture"])
            .env("OPENSPINE_INIT_SEED_CASE", case)
            .env("OPENSPINE_INIT_SEED_DIR", dir.path())
            .env_remove("OPENSPINE_ARTIFACT_KEY")
            .env_remove("OPENSPINE_GRANT_HMAC_KEY")
            .env_remove("OPENSPINE_WEBHOOK_HMAC_KEY")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{case}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
}

#[tokio::test]
async fn init_seed_acceptance_child() {
    let Ok(case) = std::env::var("OPENSPINE_INIT_SEED_CASE") else {
        return;
    };
    let dir = std::path::PathBuf::from(std::env::var_os("OPENSPINE_INIT_SEED_DIR").unwrap());
    let config_path = dir.join("openspine.yaml");
    if case != "fresh" {
        write_fresh(&config_path, 111, None, URL, "m").unwrap();
        let config = Config::load(&config_path).unwrap();
        std::fs::create_dir_all(&config.data_dir).unwrap();
        if case == "invalid-store" {
            let db = config.data_dir.join("kernel.db");
            std::fs::write(&db, b"not a kernel store").unwrap();
            assert!(run_init(&config_path, InitOptions::default())
                .await
                .is_err());
            assert_eq!(std::fs::read(db).unwrap(), b"not a kernel store");
            return;
        }
        // Existing encrypted state used to skip the owner/seed ceremony.
        crate::store::Store::open(&config.data_dir.join("kernel.db")).unwrap();
    }
    run_init(
        &config_path,
        InitOptions {
            owner: Some(111),
            model: Some("m"),
            base_url: (case == "fresh").then_some(URL),
            ..InitOptions::default()
        },
    )
    .await
    .unwrap();
    let config = Config::load(&config_path).unwrap();
    let store = crate::store::Store::open(&config.data_dir.join("kernel.db")).unwrap();
    let rows = store.list_learned_artifacts().unwrap();
    assert_eq!(rows.len(), 4);
    for manifest in crate::seed_workflows::parsed().unwrap() {
        let row = rows
            .iter()
            .find(|row| row.artifact_id == manifest.id.as_str())
            .expect("every shipped seed has accepted provenance");
        let crate::store::learned_artifacts::Provenance::ProducedBy {
            source_event_id, ..
        } = &row.provenance
        else {
            panic!("acceptance has genuine producing provenance");
        };
        let event = store
            .validated_audit_event_by_id(*source_event_id)
            .unwrap()
            .unwrap();
        assert_eq!(event.kind.as_str(), "seed_workflows.owner_accepted");
        assert_eq!(
            event.actor,
            Some(
                crate::identity::bootstrap_owner_principal(&store, 111, "Test")
                    .unwrap()
                    .principal_id
            )
        );
        assert!(store
            .is_active_proposal("workflow", manifest.id.as_str(), manifest.version)
            .unwrap());
    }
    let workflows = config.data_dir.join("artifacts.d").join("workflows");
    let manifests = crate::seed_workflows::parsed().unwrap();
    let deleted = workflows.join(crate::artifact_loader::overlay_filename(
        &manifests[0].id,
        manifests[0].version,
    ));
    let edited = workflows.join(crate::artifact_loader::overlay_filename(
        &manifests[1].id,
        manifests[1].version,
    ));
    let edited_bytes = format!("{}\n# owner edit\n", crate::seed_workflows::all()[1].1);
    if case == "edited-and-deleted" {
        std::fs::remove_file(&deleted).unwrap();
        std::fs::write(&edited, &edited_bytes).unwrap();
    }
    drop(store);
    run_init(&config_path, InitOptions::default())
        .await
        .unwrap();
    if case == "edited-and-deleted" {
        assert!(
            !deleted.exists(),
            "rerunning init must not resurrect a deleted seed"
        );
        assert_eq!(std::fs::read_to_string(edited).unwrap(), edited_bytes);
    }
    let store = crate::store::Store::open(&config.data_dir.join("kernel.db")).unwrap();
    assert_eq!(store.list_learned_artifacts().unwrap().len(), 4);
    assert!(store.verify_audit_chain().unwrap());
}

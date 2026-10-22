//! End-to-end smoke test for the single-command first-run trust loop (#118).
//!
//! Spawns the real `openspine` binary in a clean temp directory with no
//! pre-seeded key material and asserts the loop completes: the command writes
//! the configuration, mints the owner-only seed key file, binds the trusted
//! owner principal (a kernel store appears), and reports the trust ceremony.
//!
//! It runs in a separate process so its `env_file::load_adjacent` can never
//! race the in-process unit tests over the ambient environment.

use std::path::Path;
use std::process::Command;

const OWNER_ID: &str = "987654321";

#[test]
fn init_bootstraps_the_first_run_trust_loop_in_a_clean_dir() {
    let dir = tempfile::tempdir().expect("temp dir");
    let config_path = dir.path().join("openspine.yaml");

    let mut command = Command::new(env!("CARGO_BIN_EXE_openspine"));
    command
        .arg("--config")
        .arg(&config_path)
        .arg("init")
        .arg("--owner")
        .arg(OWNER_ID)
        .arg("--name")
        .arg("Tester")
        .arg("--model")
        .arg("smoke-model")
        // A genuinely clean HOME/dir: no inherited key material, so init has to
        // mint its own seed keys.
        .env("HOME", dir.path())
        .env_remove("OPENSPINE_ARTIFACT_KEY")
        .env_remove("OPENSPINE_GRANT_HMAC_KEY")
        .env_remove("OPENSPINE_WEBHOOK_HMAC_KEY")
        .env_remove("OPENSPINE_LOCAL_API_KEY");

    let output = command.output().expect("run openspine init");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "init exited non-zero.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // 1. Seed key: configuration + owner-only key file with the three keys.
    let config_text = std::fs::read_to_string(&config_path).expect("configuration written");
    assert!(
        config_text.contains(&format!("telegram_user_id: {OWNER_ID}")),
        "configuration binds the owner id.\n{config_text}"
    );
    assert!(
        config_text.contains("display_name: Tester"),
        "configuration carries the display name.\n{config_text}"
    );

    let env_text =
        std::fs::read_to_string(dir.path().join("openspine.env")).expect("seed key file written");
    for key in [
        "OPENSPINE_ARTIFACT_KEY",
        "OPENSPINE_GRANT_HMAC_KEY",
        "OPENSPINE_WEBHOOK_HMAC_KEY",
    ] {
        assert!(env_text.contains(key), "seed key file is missing {key}");
    }

    // 2. Approval anchor: the trusted owner principal was bound into a store.
    assert!(
        stdout.contains("trusted owner principal established"),
        "init reports the approval anchor.\nstdout:\n{stdout}"
    );
    assert!(
        Path::new(&dir.path().join("data").join("kernel.db")).exists(),
        "a kernel store exists after bootstrap"
    );

    // 3. Test step: the trust ceremony is reported to the owner.
    assert!(
        stdout.contains("Trust ceremony"),
        "init prints the trust ceremony.\nstdout:\n{stdout}"
    );
}

/// Runs `openspine` in `cwd` with no inherited key material.
fn run(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_openspine"))
        .current_dir(cwd)
        .args(args)
        .env("HOME", cwd)
        .env_remove("OPENSPINE_ARTIFACT_KEY")
        .env_remove("OPENSPINE_GRANT_HMAC_KEY")
        .env_remove("OPENSPINE_WEBHOOK_HMAC_KEY")
        .env_remove("OPENSPINE_LOCAL_API_KEY")
        .env_remove("OPENSPINE_TELEGRAM_BOT_TOKEN")
        .output()
        .expect("run openspine")
}

/// A bare `--config openspine.yaml` is what the quickstart types. The file it
/// writes must not depend on the directory it was written from: absolute state
/// and package paths, and the model the owner named, so readiness is green.
#[test]
fn init_from_a_relative_config_path_writes_location_independent_paths_and_a_model() {
    let dir = tempfile::tempdir().expect("temp dir");

    let output = run(
        dir.path(),
        &[
            "--config",
            "openspine.yaml",
            "init",
            "--owner",
            OWNER_ID,
            "--model",
            "smoke-model:q4",
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "init failed.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let config_text = std::fs::read_to_string(dir.path().join("openspine.yaml")).unwrap();
    let config: serde_yaml::Value = serde_yaml::from_str(&config_text).unwrap();
    for key in ["data_dir", "lyra_dir"] {
        let path = config[key].as_str().unwrap_or_default();
        assert!(
            Path::new(path).is_absolute(),
            "{key} must be absolute, got {path:?}.\n{config_text}"
        );
    }
    assert_eq!(
        config["providers"][0]["model"].as_str(),
        Some("smoke-model:q4")
    );
    assert!(
        !stdout.contains("[fail]"),
        "a fresh init must not report a blocking failure.\n{stdout}"
    );
    assert!(
        stdout.contains("start `openspine chat`"),
        "init names the chat command.\n{stdout}"
    );
}

/// Without a model init cannot produce a working install, so it refuses and
/// writes nothing rather than printing green over `model: ''`.
#[test]
fn init_without_a_model_it_can_discover_refuses_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");

    let output = run(
        dir.path(),
        &[
            "--config",
            "openspine.yaml",
            "init",
            "--owner",
            OWNER_ID,
            "--base-url",
            "http://127.0.0.1:1",
        ],
    );

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--model"), "{stderr}");
    assert!(!dir.path().join("openspine.yaml").exists());
}

/// A stale or wrong `lyra_dir` used to boot an empty registry and then deny
/// every message with `terminal message was denied before a task grant was
/// created`. It must now fail up front with the real cause.
#[test]
fn chat_with_a_missing_package_directory_fails_non_zero_and_names_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let init = run(
        dir.path(),
        &[
            "--config",
            "openspine.yaml",
            "init",
            "--owner",
            OWNER_ID,
            "--model",
            "m",
        ],
    );
    assert!(init.status.success());
    let config_path = dir.path().join("openspine.yaml");
    let text = std::fs::read_to_string(&config_path).unwrap();
    let missing = dir.path().join("no-such-package");
    let rewritten: String = text
        .lines()
        .map(|line| {
            if line.starts_with("lyra_dir:") {
                format!("lyra_dir: {}", missing.display())
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&config_path, rewritten).unwrap();

    let output = run(
        dir.path(),
        &["--config", "openspine.yaml", "chat", "--once", "hello"],
    );

    assert!(!output.status.success(), "chat must exit non-zero");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("agent package"), "{stderr}");
    assert!(stderr.contains("no-such-package"), "{stderr}");
    assert!(stderr.contains("remedy:"), "{stderr}");
    assert!(
        !stderr.contains("denied before a task grant"),
        "the real cause replaces the opaque denial: {stderr}"
    );
}

/// Bare `openspine` is the Telegram server. Its missing-token error must name
/// the token and the token-free alternative, not point at `setup`, which never
/// asks for one.
#[test]
fn bare_openspine_without_a_bot_token_names_the_token_and_chat() {
    let dir = tempfile::tempdir().expect("temp dir");
    let init = run(
        dir.path(),
        &[
            "--config",
            "openspine.yaml",
            "init",
            "--owner",
            OWNER_ID,
            "--model",
            "m",
        ],
    );
    assert!(init.status.success());

    let output = run(dir.path(), &["--config", "openspine.yaml"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("OPENSPINE_TELEGRAM_BOT_TOKEN"), "{stderr}");
    assert!(stderr.contains("openspine chat"), "{stderr}");
    assert!(stderr.contains("@BotFather"), "{stderr}");
}

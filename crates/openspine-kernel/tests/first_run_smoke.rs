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

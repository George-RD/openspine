//! Native read-only package transition review contracts for #285.
#![cfg(any(target_os = "linux", target_os = "macos"))]

use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

#[path = "package_review/guardrails.rs"]
mod guardrails;
#[path = "package_review/support.rs"]
mod support;
use support::{assert_success, database_rows, files, Fixture};

#[test]
fn review_rejects_noncanonical_selector_before_configuration_without_echoing_it() {
    let root = tempfile::tempdir().unwrap();
    for invalid in [
        "latest",
        "01arz3ndektsv4rrffq69g5fav",
        "--FORGED-\u{1b}[31m",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_openspine"))
            .env_clear()
            .current_dir(root.path())
            .arg("--config")
            .arg(root.path().join("missing/openspine.yaml"))
            .args(["package", "review", invalid, "--json"])
            .output()
            .unwrap();
        assert_error(&output, "installation-id-invalid");
        assert!(output.stdout.len() < 1024);
        assert!(!output.stdout.contains(&0x1b));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("FORGED"));
        assert!(output.stderr.is_empty());
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn review_uses_retained_candidate_and_actual_legacy_base_without_runtime() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    fs::remove_dir_all(fixture.root.path().join("candidate")).unwrap();
    fs::write(
        fixture.root.path().join("artifacts/lyra/README.md"),
        "configured-source-sentinel\n",
    )
    .unwrap();
    let inspection = fixture.run(&["package", "inspect", "artifacts/lyra", "--json"]);
    assert_success(&inspection);
    let configured: Value = serde_json::from_slice(&inspection.stdout).unwrap();
    let output = fixture
        .review_command(&installed)
        // Tokio rejects zero threads if a runtime is constructed.
        .env("TOKIO_WORKER_THREADS", "0")
        .output()
        .unwrap();
    assert_success(&output);
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["approval"], "not-performed");
    assert_eq!(report["activation_supported"], false);
    assert_eq!(report["source"]["mode"], "legacy-configured");
    assert_eq!(
        report["source"]["identity"]["content_digest"],
        configured["content_digest"]
    );
    assert_ne!(
        report["source"]["identity"]["content_digest"],
        installed["receipt"]["content_digest"]
    );
    assert_eq!(report["candidate"], installed["receipt"]);
    assert!(report["plan_digest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert_eq!(fixture.review(&installed), report);
}

#[test]
fn review_preserves_logical_rows_config_keys_and_all_package_and_overlay_bytes() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    let rows_before = database_rows(&fixture);
    let files_before = files(fixture.root.path());
    fixture.review(&installed);
    assert_eq!(database_rows(&fixture), rows_before);
    assert_eq!(files(fixture.root.path()), files_before);
    assert!(!fixture.root.path().join("data/artifacts.d").exists());
}

#[test]
fn review_does_not_recover_erased_keys_or_remove_interrupted_key_writes() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    let keys = fixture.root.path().join("data/keys");
    fs::create_dir_all(&keys).unwrap();
    let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    fs::write(keys.join(id), b"retained encrypted-key evidence").unwrap();
    fs::write(keys.join(format!("{id}.erased")), b"").unwrap();
    fs::write(
        keys.join(format!("{id}.tmp.interrupted")),
        b"interrupted-key evidence",
    )
    .unwrap();
    let before = files(&keys);
    fixture.review(&installed);
    assert_eq!(files(&keys), before);
    assert!(!fixture.root.path().join("data/artifacts").exists());
}

fn assert_error(output: &Output, expected: &str) -> Value {
    assert!(!output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("expected bounded JSON error, got {output:?}"));
    assert_eq!(report["error"]["code"], expected, "{report}");
    assert_eq!(report["approval"], "not-performed");
    assert_eq!(report["activation_supported"], false);
    report
}

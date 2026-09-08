//! Real-CLI installation contract for #273/#275. No alternate DB/config path.
#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            root: tempfile::tempdir().unwrap(),
        };
        let output = fixture.run(&["init", "--owner", "987654321", "--name", "Tester"]);
        assert_success(&output);
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra"),
            &fixture.root.path().join("candidate"),
        );
        copy_tree(
            &fixture.root.path().join("candidate"),
            &fixture.root.path().join("artifacts/lyra"),
        );
        fixture
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_openspine"));
        command
            .current_dir(self.root.path())
            .arg("--config")
            .arg(self.root.path().join("openspine.yaml"))
            .args(args)
            .env("HOME", self.root.path())
            .env_remove("OPENSPINE_ARTIFACT_KEY")
            .env_remove("OPENSPINE_GRANT_HMAC_KEY")
            .env_remove("OPENSPINE_WEBHOOK_HMAC_KEY")
            .env_remove("OPENSPINE_LOCAL_API_KEY")
            .env_remove("OPENSPINE_TEST_PACKAGE_CRASH")
            .env_remove("OPENSPINE_TEST_PACKAGE_PAUSE")
            .env_remove("OPENSPINE_TEST_PACKAGE_BARRIER");
        command
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.run(args);
        assert_success(&output);
        serde_json::from_slice(&output.stdout).expect("one JSON response")
    }

    fn install(&self) -> Value {
        self.json(&["install", "--from", "candidate", "--json"])
    }

    fn list(&self) -> Value {
        self.json(&["package", "list", "--json"])
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn install_lists_exact_snapshot_as_inactive_after_process_restart() {
    let fixture = Fixture::new();
    let config_before = fs::read(fixture.root.path().join("openspine.yaml")).unwrap();
    let keys_before = fs::read(fixture.root.path().join("openspine.env")).unwrap();
    let inspected = fixture.json(&["package", "inspect", "candidate", "--json"]);
    let installed = fixture.install();
    assert_eq!(installed["status"], "installed-inactive");
    assert_eq!(installed["selected"], false);
    assert_eq!(installed["active"], false);
    assert_eq!(installed["activation_supported"], false);
    assert_eq!(installed["idempotent_retry"], false);
    let receipt = &installed["receipt"];
    assert_eq!(receipt["package_id"], inspected["package_id"]);
    assert_eq!(receipt["revision"], inspected["revision"]);
    assert_eq!(receipt["content_digest"], inspected["content_digest"]);
    assert_eq!(receipt["inventory_format_version"], 1);
    assert_eq!(receipt["provenance"], "local-unverified");
    assert!(receipt["installation_id"].as_str().is_some());
    assert!(receipt["audit_seq"].as_u64().is_some_and(|seq| seq > 0));
    let listed = fixture.list();
    let packages = listed["packages"].as_array().unwrap();
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0]["receipt"], *receipt);
    assert_eq!(packages[0]["availability"], "available");
    assert_eq!(packages[0]["selected"], false);
    assert_eq!(packages[0]["active"], false);
    assert_eq!(
        fs::read(fixture.root.path().join("openspine.yaml")).unwrap(),
        config_before
    );
    assert_eq!(
        fs::read(fixture.root.path().join("openspine.env")).unwrap(),
        keys_before
    );
    assert!(!fixture.root.path().join("data/artifacts.d").exists());
}

#[test]
fn exact_retry_returns_same_durable_installation_receipt() {
    let fixture = Fixture::new();
    let first = fixture.install();
    let second = fixture.install();
    assert_eq!(second["idempotent_retry"], true);
    assert_eq!(first["receipt"], second["receipt"]);
    assert_eq!(fixture.list()["packages"].as_array().unwrap().len(), 1);
}

#[test]
fn changed_bytes_under_same_revision_conflict_without_overwrite() {
    let fixture = Fixture::new();
    let first = fixture.install();
    fs::write(
        fixture.root.path().join("candidate/README.md"),
        "Different bytes, same declared revision.\n",
    )
    .unwrap();
    let output = fixture.run(&["install", "--from", "candidate", "--json"]);
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["error"]["code"], "revision-conflict");
    let listed = fixture.list();
    assert_eq!(listed["packages"].as_array().unwrap().len(), 1);
    assert_eq!(listed["packages"][0]["receipt"], first["receipt"]);
}

#[test]
fn bundled_candidate_installs_without_selecting_or_connecting_accounts() {
    let fixture = Fixture::new();
    let installed = fixture.json(&["install", "lyra", "--json"]);
    assert_eq!(installed["receipt"]["provenance"], "runtime-bundled");
    assert_eq!(installed["status"], "installed-inactive");
    assert_eq!(installed["selected"], false);
    assert_eq!(installed["active"], false);
}

#[test]
fn invalid_candidate_never_appears_in_installed_index() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.path().join("candidate/package.yaml"),
        "schema_version: 999\n",
    )
    .unwrap();
    let output = fixture.run(&["install", "--from", "candidate", "--json"]);
    assert!(!output.status.success());
    assert!(fixture.list()["packages"].as_array().unwrap().is_empty());
}

#[test]
fn installed_package_verification_does_not_require_temporary_storage() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    let not_a_directory = fixture.root.path().join("temp-file");
    fs::write(&not_a_directory, b"not a directory").unwrap();
    for temporary in [fixture.root.path().join("missing-temp"), not_a_directory] {
        // Prove the child observes the unusable TMPDIR. New candidates still
        // require full loader validation; listing a retained identity does not.
        let inspection = fixture
            .command(&["package", "inspect", "candidate", "--json"])
            .env("TMPDIR", &temporary)
            .output()
            .unwrap();
        assert!(!inspection.status.success());
        let error: Value = serde_json::from_slice(&inspection.stdout).unwrap();
        assert_eq!(error["error"]["code"], "staging-unavailable");
        let output = fixture
            .command(&["package", "list", "--json"])
            .env("TMPDIR", &temporary)
            .output()
            .unwrap();
        assert_success(&output);
        let listed: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(listed["packages"].as_array().unwrap().len(), 1);
        assert_eq!(listed["packages"][0]["receipt"], installed["receipt"]);
        assert_eq!(listed["packages"][0]["availability"], "available");
        assert_eq!(listed["packages"][0]["selected"], false);
        assert_eq!(listed["packages"][0]["active"], false);
    }
}

#[path = "package_install/concurrency.rs"]
mod concurrency;
#[path = "package_install/faults.rs"]
mod faults;
#[path = "package_install/guardrails.rs"]
mod guardrails;

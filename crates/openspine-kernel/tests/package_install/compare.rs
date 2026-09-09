//! Installed comparison exercises the real non-serving CLI, never live sources.
use super::{assert_success, Fixture};
use openspine_schemas::digest::digest_of_bytes;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn id(installation: &Value) -> &str {
    installation["receipt"]["installation_id"].as_str().unwrap()
}

fn next_revision(fixture: &Fixture) {
    let path = fixture.root.path().join("candidate/package.yaml");
    let mut declaration: serde_yaml::Value =
        serde_yaml::from_slice(&fs::read(&path).unwrap()).unwrap();
    let revision = declaration["version"].as_u64().unwrap();
    declaration["version"] = (revision + 1).into();
    fs::write(path, serde_yaml::to_string(&declaration).unwrap()).unwrap();
}

fn object(fixture: &Fixture, installation: &Value) -> PathBuf {
    fixture.root.path().join("data/packages/objects").join(
        installation["receipt"]["content_digest"]
            .as_str()
            .unwrap()
            .strip_prefix("sha256:")
            .unwrap(),
    )
}

fn compare(fixture: &Fixture, from: &Value, to: &Value) -> Value {
    fixture.json(&["package", "compare", id(from), id(to), "--json"])
}

fn assert_error(output: &std::process::Output, code: &str) {
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "expected bounded JSON error; stdout: {} stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(report["error"]["code"], code);
    assert!(report.get("changes").is_none());
    assert!(report.get("from").is_none());
    assert!(report.get("to").is_none());
}

#[test]
fn comparison_reports_deterministic_exact_changes_after_source_removal() {
    let fixture = Fixture::new();
    let source = fixture.root.path().join("candidate");
    fs::write(source.join("old-note.txt"), "old documentation\n").unwrap();
    let before_readme = fs::read(source.join("README.md")).unwrap();
    let inspection = fixture.json(&["package", "inspect", "candidate", "--json"]);
    let original_files = inspection["inventory"].as_array().unwrap().len();
    let first = fixture.install();
    fs::remove_file(source.join("old-note.txt")).unwrap();
    fs::write(source.join("new-note.txt"), "private-new-document\n").unwrap();
    fs::write(source.join("README.md"), "private-replacement-document\n").unwrap();
    next_revision(&fixture);
    let second = fixture.install();
    fs::remove_dir_all(source).unwrap();

    let report = compare(&fixture, &first, &second);
    assert_eq!(report, compare(&fixture, &first, &second));
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["status"], "comparison-complete");
    assert_eq!(report["basis"], "retained-byte-integrity");
    assert_eq!(report["from"], first["receipt"]);
    assert_eq!(report["to"], second["receipt"]);
    assert_eq!(report["identical"], false);
    assert_eq!(report["runtime_inputs_changed"], false);
    assert_eq!(report["package_declaration_changed"], true);
    assert_eq!(report["counts"]["added"], 1);
    assert_eq!(report["counts"]["removed"], 1);
    assert_eq!(report["counts"]["modified"], 2);
    assert_eq!(report["counts"]["unchanged"], original_files - 3);
    let changes = report["changes"].as_array().unwrap();
    let paths: Vec<_> = changes
        .iter()
        .map(|change| change["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        ["README.md", "new-note.txt", "old-note.txt", "package.yaml"]
    );
    assert_eq!(changes[0]["change"], "modified");
    assert_eq!(changes[0]["category"], "documentation");
    assert_eq!(changes[0]["before"]["bytes"], before_readme.len());
    assert_eq!(
        changes[0]["before"]["digest"],
        digest_of_bytes(&before_readme).as_str()
    );
    assert_eq!(
        changes[0]["after"]["bytes"],
        b"private-replacement-document\n".len()
    );
    assert_eq!(changes[1]["change"], "added");
    assert!(changes[1]["before"].is_null());
    assert_eq!(changes[2]["change"], "removed");
    assert!(changes[2]["after"].is_null());
    assert_eq!(changes[3]["category"], "package-declaration");
    assert_eq!(report["runtime_compatibility"], "not-evaluated");
    assert_eq!(report["authority_review"], "not-performed");
    assert_eq!(report["publisher_verification"], "not-performed");
    assert_eq!(report["activation_supported"], false);
    assert_eq!(report["active_package_identified"], false);
    assert!(!report.to_string().contains("private-replacement-document"));
    assert!(!report.to_string().contains("private-new-document"));
}

#[test]
fn self_comparison_verifies_integrity_without_temporary_staging_or_runtime() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    let files = fixture.json(&["package", "inspect", "candidate", "--json"])["inventory"]
        .as_array()
        .unwrap()
        .len();
    let output = fixture
        .command(&[
            "package",
            "compare",
            id(&installed),
            id(&installed),
            "--json",
        ])
        .env("TMPDIR", fixture.root.path().join("missing-temp"))
        .env("TOKIO_WORKER_THREADS", "0")
        .output()
        .unwrap();
    assert_success(&output);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["identical"], true);
    assert!(report["changes"].as_array().unwrap().is_empty());
    assert_eq!(
        report["counts"],
        serde_json::json!({"added": 0, "removed": 0, "modified": 0, "unchanged": files})
    );
    assert_eq!(report["runtime_inputs_changed"], false);
    assert_eq!(report["package_declaration_changed"], false);
    assert_eq!(report["authority_review"], "not-performed");
}

#[test]
fn reverse_comparison_reverses_changes_and_preserves_original_provenance() {
    let fixture = Fixture::new();
    let first = fixture.json(&["install", "lyra", "--json"]);
    next_revision(&fixture);
    fs::write(
        fixture.root.path().join("candidate/extra.txt"),
        "not echoed",
    )
    .unwrap();
    let second = fixture.install();
    let forward = compare(&fixture, &first, &second);
    let reverse = compare(&fixture, &second, &first);
    assert_eq!(forward["from"]["provenance"], "runtime-bundled");
    assert_eq!(forward["to"]["provenance"], "local-unverified");
    assert_eq!(reverse["from"], forward["to"]);
    assert_eq!(reverse["to"], forward["from"]);
    assert_eq!(forward["counts"]["added"], reverse["counts"]["removed"]);
    assert_eq!(forward["counts"]["removed"], reverse["counts"]["added"]);
    for (left, right) in forward["changes"]
        .as_array()
        .unwrap()
        .iter()
        .zip(reverse["changes"].as_array().unwrap())
    {
        assert_eq!(left["path"], right["path"]);
        assert_eq!(left["before"], right["after"]);
        assert_eq!(left["after"], right["before"]);
    }
}

#[test]
fn runtime_comment_edits_are_flagged_without_claiming_semantic_authority_review() {
    let fixture = Fixture::new();
    let first = fixture.install();
    let file = fs::read_dir(fixture.root.path().join("candidate/packs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
        })
        .unwrap();
    let before = fs::read(&file).unwrap();
    let mut after = before.clone();
    after.extend_from_slice(b"\n# package comparison runtime sentinel\n");
    fs::write(&file, &after).unwrap();
    next_revision(&fixture);
    let second = fixture.install();
    let report = compare(&fixture, &first, &second);
    assert_eq!(report["runtime_inputs_changed"], true);
    let change = report["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|change| change["category"] == "runtime-input")
        .unwrap();
    assert_eq!(change["change"], "modified");
    assert_eq!(
        change["before"]["digest"],
        digest_of_bytes(&before).as_str()
    );
    assert_eq!(change["after"]["digest"], digest_of_bytes(&after).as_str());
    assert_eq!(report["authority_review"], "not-performed");
    assert!(!report.to_string().contains("runtime sentinel"));
}

#[test]
fn selectors_are_exact_committed_ids_and_diagnostics_never_echo_input() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    for invalid in [
        "lyra",
        "latest",
        "candidate",
        "sha256:deadbeef",
        "\u{1b}[31mFORGED-APPROVAL\n",
    ] {
        for (from, to) in [(invalid, id(&installed)), (id(&installed), invalid)] {
            let output = fixture.run(&["package", "compare", from, to, "--json"]);
            assert_error(&output, "installation-id-invalid");
            assert!(!output.stdout.contains(&0x1b));
            assert!(!String::from_utf8_lossy(&output.stdout).contains("FORGED-APPROVAL"));
            assert!(output.stderr.is_empty());
        }
    }
    let missing = ulid::Ulid::new().to_string();
    for (from, to) in [
        (missing.as_str(), id(&installed)),
        (id(&installed), missing.as_str()),
    ] {
        assert_error(
            &fixture.run(&["package", "compare", from, to, "--json"]),
            "installation-not-found",
        );
    }
}

#[test]
fn both_endpoints_fail_closed_for_missing_changed_appended_or_linked_objects() {
    for corrupt_from in [false, true] {
        for fault in ["missing", "modified", "appended", "linked"] {
            let fixture = Fixture::new();
            let first = fixture.install();
            next_revision(&fixture);
            let second = fixture.install();
            let retained = object(&fixture, if corrupt_from { &first } else { &second });
            match fault {
                "missing" => fs::remove_dir_all(&retained).unwrap(),
                "modified" => fs::write(retained.join("README.md"), "corruption sentinel").unwrap(),
                "appended" => {
                    fs::write(retained.join("unexpected.md"), "corruption sentinel").unwrap()
                }
                "linked" => {
                    fs::remove_file(retained.join("README.md")).unwrap();
                    std::os::unix::fs::symlink(
                        fixture.root.path().join("candidate/README.md"),
                        retained.join("README.md"),
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
            let output = fixture.run(&["package", "compare", id(&first), id(&second), "--json"]);
            assert_error(&output, "object-corrupt");
            assert!(!String::from_utf8_lossy(&output.stdout).contains("corruption sentinel"));
            if fault == "missing" {
                assert!(!retained.exists());
            }
        }
    }
}

#[test]
fn corrupt_self_comparison_is_not_short_circuited_to_equal() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    fs::write(object(&fixture, &installed).join("README.md"), "changed").unwrap();
    assert_error(
        &fixture.run(&[
            "package",
            "compare",
            id(&installed),
            id(&installed),
            "--json",
        ]),
        "object-corrupt",
    );
}

#[test]
fn comparison_does_not_change_configuration_keys_receipts_or_active_files() {
    let fixture = Fixture::new();
    let first = fixture.install();
    next_revision(&fixture);
    let second = fixture.install();
    let config = fs::read(fixture.root.path().join("openspine.yaml")).unwrap();
    let keys = fs::read(fixture.root.path().join("openspine.env")).unwrap();
    let active = fixture.json(&["package", "inspect", "artifacts/lyra", "--json"]);
    let receipts = fixture.json(&["package", "receipts", "--json"]);
    let installed = fixture.list();
    compare(&fixture, &first, &second);
    assert_eq!(
        fs::read(fixture.root.path().join("openspine.yaml")).unwrap(),
        config
    );
    assert_eq!(
        fs::read(fixture.root.path().join("openspine.env")).unwrap(),
        keys
    );
    assert_eq!(
        fixture.json(&["package", "inspect", "artifacts/lyra", "--json"]),
        active
    );
    assert_eq!(fixture.json(&["package", "receipts", "--json"]), receipts);
    assert_eq!(fixture.list(), installed);
    assert!(!fixture.root.path().join("data/artifacts.d").exists());
}

#[test]
fn comparison_rejects_a_tampered_ledger_projection_before_producing_a_report() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    let db = rusqlite::Connection::open(fixture.root.path().join("data/kernel.db")).unwrap();
    db.execute(
        "UPDATE audit_log SET event_json = '{}' WHERE kind = 'package.install_committed'",
        [],
    )
    .unwrap();
    drop(db);
    assert_error(
        &fixture.run(&[
            "package",
            "compare",
            id(&installed),
            id(&installed),
            "--json",
        ]),
        "ledger-unavailable",
    );
}

#[cfg(debug_assertions)]
#[test]
fn comparison_does_not_recover_an_unrelated_interrupted_installation() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    next_revision(&fixture);
    let interrupted = fixture
        .command(&["install", "--from", "candidate", "--json"])
        .env("OPENSPINE_TEST_PACKAGE_CRASH", "after-prepared")
        .output()
        .unwrap();
    assert_eq!(interrupted.status.code(), Some(75));
    let snapshot = || {
        let db = rusqlite::Connection::open(fixture.root.path().join("data/kernel.db")).unwrap();
        let audits: i64 = db
            .query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0))
            .unwrap();
        let prepared: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM package_install_attempts WHERE state = 'prepared'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        (audits, prepared)
    };
    let before = snapshot();
    assert_eq!(before.1, 1);
    compare(&fixture, &installed, &installed);
    assert_eq!(snapshot(), before);
}

#[path = "compare_guardrails.rs"]
mod guardrails;

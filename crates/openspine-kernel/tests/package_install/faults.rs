//! Crash and tamper proofs through separate invocations of the real binary.
use super::{assert_success, Fixture};
use serde_json::Value;
use std::fs;
use std::os::unix::fs::symlink;

fn committed(fixture: &Fixture) -> usize {
    fixture.json(&["package", "receipts", "--json"])["receipts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["event"] == "package.install_committed")
        .count()
}

#[test]
fn every_crash_boundary_recovers_without_a_false_or_duplicate_install() {
    for point in [
        "after-prepared",
        "during-staging",
        "before-publish",
        "after-rename-before-sync",
        "after-publish",
        "after-audit-before-index",
        "before-index-commit",
        "after-commit",
    ] {
        let fixture = Fixture::new();
        let output = fixture
            .command(&["install", "--from", "candidate", "--json"])
            .env("OPENSPINE_TEST_PACKAGE_CRASH", point)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(75), "{point}: {:?}", output);
        let expected = usize::from(point == "after-commit");
        assert_eq!(
            fixture.list()["packages"].as_array().unwrap().len(),
            expected,
            "{point}"
        );
        assert_eq!(committed(&fixture), expected, "{point}");
        let first = fixture.install();
        assert_eq!(first["idempotent_retry"], expected == 1, "{point}");
        let retry = fixture.install();
        assert_eq!(first["receipt"], retry["receipt"], "{point}");
        assert_eq!(committed(&fixture), 1, "{point}");
        assert_eq!(
            fs::read_dir(fixture.root.path().join("data/packages/staging"))
                .unwrap()
                .count(),
            0,
            "{point}"
        );
    }
}

#[test]
fn audit_insert_failure_rolls_back_index_and_retry_reuses_the_orphan() {
    let fixture = Fixture::new();
    let database = fixture.root.path().join("data/kernel.db");
    let conn = rusqlite::Connection::open(&database).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_package_commit BEFORE INSERT ON audit_log
        WHEN NEW.kind = 'package.install_committed' BEGIN SELECT RAISE(ABORT, 'injected append failure'); END;").unwrap();
    let output = fixture.run(&["install", "--from", "candidate", "--json"]);
    assert!(!output.status.success());
    assert_eq!(fixture.list()["packages"].as_array().unwrap().len(), 0);
    assert_eq!(committed(&fixture), 0);
    assert_eq!(
        fs::read_dir(fixture.root.path().join("data/packages/objects"))
            .unwrap()
            .count(),
        1
    );
    conn.execute_batch("DROP TRIGGER reject_package_commit;")
        .unwrap();
    fixture.install();
    assert_eq!(committed(&fixture), 1);
    assert_eq!(
        fs::read_dir(fixture.root.path().join("data/packages/objects"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn missing_mutated_and_appended_objects_are_never_silently_repaired() {
    for damage in ["missing", "mutated", "appended"] {
        let fixture = Fixture::new();
        let first = fixture.install();
        let digest = first["receipt"]["content_digest"]
            .as_str()
            .unwrap()
            .strip_prefix("sha256:")
            .unwrap();
        let object = fixture
            .root
            .path()
            .join("data/packages/objects")
            .join(digest);
        match damage {
            "missing" => fs::remove_dir_all(&object).unwrap(),
            "mutated" => fs::write(object.join("README.md"), "tampered").unwrap(),
            _ => fs::write(object.join("added.md"), "appended").unwrap(),
        }
        let listed = fixture.list();
        assert_eq!(listed["packages"][0]["receipt"], first["receipt"]);
        assert_eq!(
            listed["packages"][0]["availability"],
            "unavailable-or-corrupt"
        );
        let output = fixture.run(&["install", "--from", "candidate", "--json"]);
        assert!(!output.status.success());
        let error: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(error["error"]["code"], "object-corrupt");
        assert_eq!(committed(&fixture), 1);
        if damage == "missing" {
            assert!(!object.exists());
        }
    }
}

#[test]
fn destination_symlink_is_refused_without_writing_outside_data_root() {
    let fixture = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), fixture.root.path().join("data/packages")).unwrap();
    let output = fixture.run(&["install", "--from", "candidate", "--json"]);
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["error"]["code"], "destination-invalid");
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn concurrent_exact_installers_share_one_identity_and_success_receipt() {
    let fixture = Fixture::new();
    let command = ["install", "--from", "candidate", "--json"];
    let first = fixture
        .command(&command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let second = fixture
        .command(&command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();
    assert_success(&first);
    assert_success(&second);
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    let second: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(first["receipt"], second["receipt"]);
    assert_ne!(first["idempotent_retry"], second["idempotent_retry"]);
    assert_eq!(committed(&fixture), 1);
}

#[test]
fn deleted_index_row_is_ledger_corruption_not_permission_to_install_again() {
    let fixture = Fixture::new();
    fixture.install();
    let conn = rusqlite::Connection::open(fixture.root.path().join("data/kernel.db")).unwrap();
    conn.execute("DELETE FROM installed_packages", []).unwrap();
    for args in [
        &["install", "--from", "candidate", "--json"][..],
        &["package", "list", "--json"][..],
    ] {
        let output = fixture.run(args);
        assert!(!output.status.success());
        let error: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(error["error"]["code"], "ledger-unavailable");
    }
}

#[test]
fn missing_database_does_not_create_an_alternate_ledger() {
    let fixture = Fixture::new();
    let database = fixture.root.path().join("data/kernel.db");
    fs::remove_file(&database).unwrap();
    let output = fixture.run(&["install", "--from", "candidate", "--json"]);
    assert!(!output.status.success());
    assert!(!database.exists());
    assert!(!fixture.root.path().join("data/packages").exists());
}

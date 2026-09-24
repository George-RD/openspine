use super::*;
use std::os::fd::AsRawFd;

#[test]
fn staging_failure_does_not_reclassify_intact_candidate_as_corrupt() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    let output = fixture
        .review_command(&installed)
        .env("TMPDIR", fixture.root.path().join("missing-temp"))
        .output()
        .unwrap();
    let error = assert_error(&output, "candidate-staging-unavailable");
    assert_eq!(error["candidate_integrity"], "verified");
    assert_eq!(error["candidate_compatibility"], "not-evaluated");
    let id = installed["receipt"]["installation_id"].as_str().unwrap();
    let comparison = fixture
        .command(&["package", "compare", id, id, "--json"])
        .env("TMPDIR", fixture.root.path().join("missing-temp"))
        .output()
        .unwrap();
    assert_success(&comparison);
    assert_eq!(
        serde_json::from_slice::<Value>(&comparison.stdout).unwrap()["identical"],
        true
    );
}

#[test]
fn review_refuses_missing_modified_appended_or_linked_retained_bytes_without_repair() {
    for fault in ["missing", "modified", "appended", "linked"] {
        let fixture = Fixture::new();
        let installed = fixture.install();
        let object = fixture.object(&installed);
        match fault {
            "missing" => fs::remove_dir_all(&object).unwrap(),
            "modified" => {
                fs::write(object.join("README.md"), "private-corruption-sentinel").unwrap()
            }
            "appended" => {
                fs::write(object.join("appended.md"), "private-corruption-sentinel").unwrap()
            }
            "linked" => {
                fs::remove_file(object.join("README.md")).unwrap();
                std::os::unix::fs::symlink(
                    fixture.root.path().join("candidate/README.md"),
                    object.join("README.md"),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let before = database_rows(&fixture);
        let output = fixture.review_command(&installed).output().unwrap();
        let error = assert_error(&output, "object-corrupt");
        assert_eq!(error["candidate_integrity"], "unavailable-or-corrupt");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("private-corruption-sentinel"));
        assert!(!String::from_utf8_lossy(&output.stdout)
            .contains(fixture.root.path().to_str().unwrap()));
        assert_eq!(database_rows(&fixture), before);
        if fault == "missing" {
            assert!(!object.exists());
        }
    }
}

#[test]
fn review_does_not_invent_a_current_manifest_or_migrate_a_different_product() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    fs::remove_file(fixture.root.path().join("artifacts/lyra/package.yaml")).unwrap();
    assert_error(
        &fixture.review_command(&installed).output().unwrap(),
        "current-base-unavailable",
    );
    assert!(!fixture
        .root
        .path()
        .join("artifacts/lyra/package.yaml")
        .exists());

    let fixture = Fixture::new();
    let manifest = fixture.root.path().join("candidate/package.yaml");
    let mut declaration: serde_yaml::Value =
        serde_yaml::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    declaration["id"] = "different-product".into();
    fs::write(manifest, serde_yaml::to_string(&declaration).unwrap()).unwrap();
    let installed = fixture.install();
    assert_error(
        &fixture.review_command(&installed).output().unwrap(),
        "package-id-mismatch",
    );
}

#[test]
fn review_counts_live_work_without_printing_payload_or_consuming_state() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    let db = rusqlite::Connection::open(fixture.root.path().join("data/kernel.db")).unwrap();
    db.execute(
        "INSERT INTO kv_state (key,value) VALUES ('secret.intake.pending', ?1)",
        ["private-intake-payload-sentinel"],
    )
    .unwrap();
    drop(db);
    let before = database_rows(&fixture);
    let report = fixture.review(&installed);
    assert_eq!(report["outstanding_work"]["quiescent"], false);
    assert_eq!(
        report["outstanding_work"]["sources"]["kv_state.secret.intake.pending"]["outstanding"],
        1
    );
    assert!(report["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| blocker == "outstanding-work"));
    assert!(!report
        .to_string()
        .contains("private-intake-payload-sentinel"));
    assert_eq!(database_rows(&fixture), before);
}

#[test]
fn review_respects_runtime_lock_and_does_not_bootstrap_a_missing_ledger() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    let lock_path = fs::read_dir(fixture.root.path())
        .unwrap()
        .map(|entry| entry.unwrap().path().join("lifetime.lock"))
        .find(|path| path.is_file())
        .unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path)
        .unwrap();
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    assert_error(
        &fixture.review_command(&installed).output().unwrap(),
        "data-directory-locked",
    );
    drop(lock);
    let database = fixture.root.path().join("data/kernel.db");
    fs::remove_file(&database).unwrap();
    assert_error(
        &fixture.review_command(&installed).output().unwrap(),
        "ledger-unavailable",
    );
    assert!(!database.exists());
}

#[cfg(debug_assertions)]
#[test]
fn review_does_not_recover_an_unrelated_interrupted_installation() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    let manifest = fixture.root.path().join("candidate/package.yaml");
    let mut declaration: serde_yaml::Value =
        serde_yaml::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    declaration["version"] = (declaration["version"].as_u64().unwrap() + 1).into();
    fs::write(manifest, serde_yaml::to_string(&declaration).unwrap()).unwrap();
    let crashed = fixture
        .command(&["install", "--from", "candidate", "--json"])
        .env("OPENSPINE_TEST_PACKAGE_CRASH", "after-prepared")
        .output()
        .unwrap();
    assert_eq!(crashed.status.code(), Some(75));
    let rows = database_rows(&fixture);
    let payloads = files(fixture.root.path());
    fixture.review(&installed);
    assert_eq!(database_rows(&fixture), rows);
    assert_eq!(files(fixture.root.path()), payloads);
}

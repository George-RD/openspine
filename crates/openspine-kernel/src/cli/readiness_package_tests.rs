use super::*;
use crate::package::install_types::{PackageIdentity, PackageProvenance};
use crate::package::managed_source::tests::{variant, Fixture};
use crate::store::package_install::{CommitResult, PrepareResult};
use std::fs;

fn config(fixture: &Fixture) -> Config {
    serde_yaml::from_str(&format!(
        "data_dir: {}\nsandbox:\n  driver: process\nowner:\n  telegram_user_id: 1\n  display_name: owner\nspend_cap:\n  model_calls_per_day: 10\n  connector_calls_per_day: 10\nlyra_dir: /missing/conflicting-package\nproviders: []\n", fixture.data.display())).unwrap()
}

#[test]
fn readiness_snapshot_has_no_database_write_or_journal_side_effects() {
    let fixture = Fixture::new();
    let receipt = fixture.install(&variant(fixture.root.path(), 3));
    fixture.select(&receipt);
    let path = fixture.data.join("kernel.db");
    let before = fs::read(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    assert!(!path.with_extension("db-wal").exists());
    assert!(!path.with_extension("db-shm").exists());
    let checks = checks(&config(&fixture), Path::new("openspine.yaml"));
    assert_eq!(
        checks[0].state,
        super::super::CheckState::Pass,
        "{checks:?}"
    );
    assert!(checks[0]
        .detail
        .contains(&receipt.installation_id.to_string()));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    assert!(!path.with_extension("db-wal").exists());
    assert!(!path.with_extension("db-shm").exists());
}

#[test]
fn readiness_cannot_migrate_an_older_schema_or_initialize_a_missing_database() {
    let fixture = Fixture::new();
    let path = fixture.data.join("kernel.db");
    rusqlite::Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", 0)
        .unwrap();
    let before = fs::read(&path).unwrap();
    let report = checks(&config(&fixture), Path::new("openspine.yaml"));
    assert_eq!(report[0].state, super::super::CheckState::Fail);
    assert!(report[0].detail.contains("ledger-schema-unavailable"));
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::remove_file(&path).unwrap();
    assert_eq!(
        checks(&config(&fixture), Path::new("openspine.yaml"))[0].state,
        super::super::CheckState::Fail
    );
    assert!(!path.exists());
}

#[test]
fn current_runtime_drift_is_unavailable_with_exact_selected_identity() {
    let fixture = Fixture::new();
    let source = variant(fixture.root.path(), 3);
    let snapshot = crate::package::inspect(&source).unwrap();
    let mut files: std::collections::BTreeMap<_, _> = snapshot
        .files()
        .map(|(p, b)| (p.to_owned(), b.to_vec()))
        .collect();
    files.insert(
        "agents/email_reply_drafter.yaml".into(),
        b"not an artifact".to_vec(),
    );
    let identity = PackageIdentity {
        package_id: "lyra".into(),
        revision: 3,
        inventory_format_version: 1,
        content_digest: openspine_schemas::digest::digest_of(&serde_json::json!({
            "inventory_format_version": 1,
            "files": files.iter().map(|(path, bytes)| serde_json::json!({
                "path": path, "bytes": bytes.len(),
                "digest": openspine_schemas::digest::digest_of_bytes(bytes),
            })).collect::<Vec<_>>(),
        })),
        manifest_digest: openspine_schemas::digest::digest_of_bytes(&files["package.yaml"]),
    };
    let object = fixture.data.join("packages/objects").join(
        identity
            .content_digest
            .as_str()
            .strip_prefix("sha256:")
            .unwrap(),
    );
    for (path, bytes) in files {
        let path = object.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let PrepareResult::Prepared(metadata) = fixture
        .store
        .prepare_package_install(identity, PackageProvenance::LocalUnverified)
        .unwrap()
    else {
        panic!("fresh installation")
    };
    let CommitResult::Committed(receipt) = fixture.store.commit_package_install(&metadata).unwrap()
    else {
        panic!("commit")
    };
    fixture.select(&receipt);
    let report = checks(&config(&fixture), Path::new("openspine.yaml"));
    assert_eq!(report[0].state, super::super::CheckState::Fail);
    assert!(
        report[0].detail.contains("selected-package-incompatible"),
        "{report:?}"
    );
    assert!(report[0]
        .detail
        .contains(&receipt.installation_id.to_string()));
    assert!(!report[0].detail.contains("not an artifact"));
}

#[test]
fn readiness_verifies_the_audit_chain_and_refuses_a_linked_database() {
    let fixture = Fixture::new();
    let receipt = fixture.install(&variant(fixture.root.path(), 3));
    fixture.select(&receipt);
    let path = fixture.data.join("kernel.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("UPDATE audit_log SET hash = 'corrupt' WHERE seq = 1", [])
        .unwrap();
    assert!(checks(&config(&fixture), Path::new("openspine.yaml"))[0]
        .detail
        .contains("selection-ledger-invalid"));
    drop(db);
    let moved = fixture.data.join("real.db");
    fs::rename(&path, &moved).unwrap();
    std::os::unix::fs::symlink(moved, &path).unwrap();
    assert!(Store::open_for_readiness(&path).is_err());
}

#[test]
fn a_writable_outer_transaction_is_never_silently_reused_for_a_read() {
    let store = Store::open_in_memory().unwrap();
    store.with_conn_for_test(|conn| conn.execute_batch("BEGIN DEFERRED").unwrap());
    assert!(store.with_deferred_read(|_| Ok(())).is_err());
    store.with_conn_for_test(|conn| conn.execute_batch("ROLLBACK").unwrap());
}

#[test]
fn readiness_keeps_one_wal_snapshot_while_the_runtime_can_append() {
    let fixture = Fixture::new();
    let first = fixture.install(&variant(fixture.root.path(), 3));
    fixture.select(&first);
    let path = fixture.data.join("kernel.db");
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    let reader = Store::open_for_readiness(&path).unwrap();
    let before = fs::read(&path).unwrap();
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();
    let second = fixture.install(&variant(fixture.root.path(), 4));
    fixture.select(&second);
    assert_eq!(reader.resolve(&fixture.data).unwrap().unwrap().receipt.installation_id, first.installation_id);
    assert_eq!(reader.installations().unwrap().len(), 1);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), mtime);
    assert_eq!(Store::open_for_readiness(&path).unwrap().resolve(&fixture.data).unwrap().unwrap().receipt.installation_id, second.installation_id);
}

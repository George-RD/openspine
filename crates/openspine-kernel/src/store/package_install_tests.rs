//! Maintenance opening must not run the authority-changing runtime sweep.
use super::{standing_rules_staleness_tests::rule_manifest, Store};
use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::standing_rule::DarkWindowDefault;

#[test]
fn package_maintenance_does_not_sweep_authority_but_runtime_open_still_does() {
    let root = tempfile::tempdir().unwrap();
    // Match the CLI's locked canonical data root. macOS temp paths can contain
    // parent aliases, which SQLite's NOFOLLOW correctly rejects.
    let path = root.path().canonicalize().unwrap().join("kernel.db");
    let now = Timestamp::now();
    let action = ActionId::new("email.send");
    let store = Store::open(&path).unwrap();
    super::standing_rules_tests::install_legacy_allow_dark_window_rule(
        &store,
        &rule_manifest("package-no-sweep", "email.send", DarkWindowDefault::Allow),
        now,
    );
    assert!(store
        .active_standing_rule_for_action(&action, now)
        .unwrap()
        .is_some());
    drop(store);
    let maintenance = Store::open_for_package_management(&path).unwrap();
    assert!(maintenance
        .active_standing_rule_for_action(&action, now)
        .unwrap()
        .is_some());
    assert_eq!(
        maintenance
            .count_audit_events_of_kind("standing_rule.ineligible_allow_retired")
            .unwrap(),
        0
    );
    drop(maintenance);
    let runtime = Store::open(&path).unwrap();
    assert!(runtime
        .active_standing_rule_for_action(&action, now)
        .unwrap()
        .is_none());
    assert_eq!(
        runtime
            .count_audit_events_of_kind("standing_rule.ineligible_allow_retired")
            .unwrap(),
        1
    );
}

#[cfg(unix)]
#[test]
fn package_maintenance_rejects_database_and_parent_symlinks_without_mutation() {
    let root = tempfile::tempdir().unwrap();
    let canonical = root.path().canonicalize().unwrap();
    let path = canonical.join("kernel.db");
    drop(Store::open(&path).unwrap());
    let before = std::fs::read(&path).unwrap();
    let database_alias = canonical.join("alias.db");
    std::os::unix::fs::symlink(&path, &database_alias).unwrap();
    let parent_alias = canonical.join("alias-dir");
    std::os::unix::fs::symlink(&canonical, &parent_alias).unwrap();

    for alias in [database_alias, parent_alias.join("kernel.db")] {
        let result = Store::open_for_package_management(&alias);
        assert!(
            matches!(
                result,
                Err(super::StoreError::Sqlite(rusqlite::Error::SqliteFailure(code, _)))
                    if code.extended_code == rusqlite::ffi::SQLITE_CANTOPEN_SYMLINK
            ),
            "maintenance must refuse a symlink before opening the ledger"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}

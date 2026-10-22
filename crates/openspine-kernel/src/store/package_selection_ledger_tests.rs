//! Ledger replay, sticky managed mode and unrelated authority state (ANO-822).
use super::package_selection_fixtures::{decision, install, select};
use super::{Store, StoreError};
use jiff::Timestamp;

#[test]
fn ledger_replay_rejects_every_projection_without_its_audit_receipt() {
    let tamper = |sql: &str| {
        let store = Store::open_in_memory().unwrap();
        let (a, b) = (install(&store, 1), install(&store, 2));
        let now = Timestamp::now();
        select(&store, &a, now);
        select(&store, &b, now);
        store.validate_package_selection().unwrap();
        // Replay must not depend on SQLite foreign keys: disable them so the
        // tampered projection is actually written, then require detection.
        store
            .conn
            .lock()
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .unwrap();
        store
            .with_immediate_tx(|tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
        let mismatch = |result: Result<(), StoreError>| {
            matches!(result, Err(StoreError::BadLedgerMeta(message))
                if message == "package selection ledger mismatch")
        };
        mismatch(store.validate_package_selection()) && mismatch(store.validate_package_ledger())
    };
    for sql in [
        // A history row with no matching receipt (and the projection to match).
        "INSERT INTO package_selection_history VALUES (3, (SELECT installation_id FROM installed_packages LIMIT 1), 0, 'forged', 1);",
        "DELETE FROM package_selection_history WHERE generation = 2;",
        "DELETE FROM package_selection;",
        // The receipts remain but every projection is wiped: not legacy mode.
        "DELETE FROM package_selection; DELETE FROM package_selection_history; DELETE FROM package_selection_decisions;",
        "UPDATE package_selection SET installation_id = (SELECT installation_id FROM installed_packages ORDER BY revision LIMIT 1);",
        "UPDATE package_selection SET generation = 1;",
        "UPDATE package_selection SET invalidation_generation = 1;",
        "UPDATE package_selection_history SET installation_id = (SELECT installation_id FROM installed_packages ORDER BY revision LIMIT 1) WHERE generation = 2;",
        "UPDATE package_selection_history SET invalidation_generation = 1 WHERE generation = 2;",
        "UPDATE package_selection_decisions SET state = 'presented';",
        "DELETE FROM package_selection_decisions;",
        "UPDATE package_selection_decisions SET payload_json = replace(payload_json, 'owner-principal', 'someone');",
    ] {
        assert!(tamper(sql), "replay accepted tampering: {sql}");
    }
}

#[test]
fn managed_mode_is_sticky_when_the_selection_row_is_lost() {
    let store = Store::open_in_memory().unwrap();
    let (a, b) = (install(&store, 1), install(&store, 2));
    let now = Timestamp::now();
    select(&store, &a, now);
    let presented = decision(&store, &b, now);
    store
        .record_package_selection_decision(&presented, now)
        .unwrap();
    store
        .conn
        .lock()
        .execute_batch("PRAGMA foreign_keys = OFF;")
        .unwrap();
    store
        .with_immediate_tx(|tx| {
            tx.execute_batch("DELETE FROM package_selection;")?;
            Ok(())
        })
        .unwrap();
    // The projection is gone but history remains: never silently legacy.
    assert!(store.validate_package_selection().is_err());
    // Nor can a legacy-shaped decision be presented or a presented one
    // consumed to recreate it without the sticky history.
    let legacy_shaped = decision(&store, &b, now);
    assert!(matches!(
        store.record_package_selection_decision(&legacy_shaped, now),
        Err(StoreError::BadLedgerMeta(_))
    ));
    assert!(matches!(
        store.commit_package_selection(&presented, now),
        Err(StoreError::BadLedgerMeta(_))
    ));
}

#[test]
fn selection_changes_no_other_authority_state() {
    let store = Store::open_in_memory().unwrap();
    let (a, b) = (install(&store, 1), install(&store, 2));
    let now = Timestamp::now();
    let dump = |store: &Store| -> String {
        store
            .with_deferred_read(|tx| {
                let mut out = String::new();
                for table in [
                    "standing_rules",
                    "standing_rule_usage",
                    "action_requests",
                    "task_grants",
                    "grant_counters",
                    "erased_counterparties",
                    "eval_verdicts",
                ] {
                    let mut statement = tx.prepare(&format!("SELECT * FROM {table}"))?;
                    let columns = statement.column_count();
                    let mut rows = statement.query([])?;
                    while let Some(row) = rows.next()? {
                        for index in 0..columns {
                            out.push_str(&format!("{:?}|", row.get_ref(index)?));
                        }
                        out.push('\n');
                    }
                }
                Ok(out)
            })
            .unwrap()
    };
    let before = dump(&store);
    select(&store, &a, now);
    select(&store, &b, now);
    select(&store, &a, now);
    assert_eq!(dump(&store), before);
}

//! Standing-rule scope-binding schema migration tests (#128), split from
//! `migration_tests.rs` to keep both files under the 500-line gate.

use super::Store;
use tempfile::tempdir;

/// v5 adds the two nullable standing-rule scope-binding digest columns via the
/// additive ad-hoc lane (idempotent across every open) and stamps the version.
/// A legacy DB converges additively without any row rewrite, and the versioned
/// down path drops the columns for the documented downgrade.
#[test]
fn v5_standing_rule_scope_binding_columns_added_and_dropped() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    let store = Store::open(&path).unwrap();
    {
        let conn = store.conn.lock();
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, 10);

        // Both columns exist after a fresh open.
        let cols: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('standing_rules') \
                 WHERE name IN ('reviewed_scope_digest', 'compatibility_digest')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cols, 2, "scope-binding digest columns must exist after v5");
    }
    drop(store);

    // Downgrade path: drop the columns and roll the stamp back.
    let store = Store::open(&path).unwrap();
    {
        let mut conn = store.conn.lock();
        super::migrations::revert_versioned_migrations_for_test(&mut conn, 4).unwrap();
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, 4);
        let cols: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('standing_rules') \
                 WHERE name IN ('reviewed_scope_digest', 'compatibility_digest')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cols, 0, "columns must be dropped on the v5 downgrade");
    }
    drop(store);

    // Re-open re-adds them additively.
    let store = Store::open(&path).unwrap();
    {
        let conn = store.conn.lock();
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, 10);
        let cols: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('standing_rules') \
                 WHERE name IN ('reviewed_scope_digest', 'compatibility_digest')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cols, 2, "columns must be re-added on re-open");
    }
}

/// v6 adds the reviewed outstanding-exception allowance column and the two
/// pending-row context-binding columns via the same additive ad-hoc lane, and
/// stamps the version. The down path drops all three for the documented
/// downgrade; a re-open re-adds them without touching any row.
#[test]
fn v6_dark_window_exception_columns_added_and_dropped() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    let store = Store::open(&path).unwrap();
    let count_columns = |conn: &rusqlite::Connection| -> i64 {
        let rules: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('standing_rules') \
                 WHERE name = 'dark_window_max_pending'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('standing_rule_pending_actions') \
                 WHERE name IN ('reviewed_scope_digest', 'compatibility_digest')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        rules + pending
    };
    {
        let conn = store.conn.lock();
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, 10);
        assert_eq!(count_columns(&conn), 3, "all three columns exist after v6");
    }
    drop(store);

    let store = Store::open(&path).unwrap();
    {
        let mut conn = store.conn.lock();
        super::migrations::revert_versioned_migrations_for_test(&mut conn, 5).unwrap();
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, 5);
        assert_eq!(
            count_columns(&conn),
            0,
            "columns are dropped on the v6 downgrade"
        );
    }
    drop(store);

    let store = Store::open(&path).unwrap();
    {
        let conn = store.conn.lock();
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, 10);
        assert_eq!(count_columns(&conn), 3, "columns are re-added on re-open");
    }
}

use super::Store;
use rusqlite::Connection;
use tempfile::tempdir;

#[test]
fn legacy_digest_summary_is_sanitized_idempotently() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    let secret = "LEGACY_DIGEST_SECRET_MUST_NOT_SURVIVE";
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE digest_items (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                id TEXT NOT NULL UNIQUE,
                ts TEXT NOT NULL,
                class TEXT NOT NULL,
                summary TEXT NOT NULL,
                resolved INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO digest_items (id, ts, class, summary, resolved) VALUES (?1, ?2, ?3, ?4, 0)",
            rusqlite::params![
                "01J00000000000000000000000",
                "2026-01-01T00:00:00Z",
                "connector",
                secret,
            ],
        )
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    let first = store.owner_digest_items().unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(
        first[0].summary,
        "[connector] legacy failure detail unavailable"
    );
    assert!(first[0].text_ref.is_none());
    assert!(!first[0].summary.contains(secret));
    drop(store);

    let reopened = Store::open(&path).unwrap();
    let second = reopened.owner_digest_items().unwrap();
    assert_eq!(second[0].summary, first[0].summary);
    assert!(!second[0].summary.contains(secret));
}

#[test]
fn versioned_migrations_up_down_up() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");

    // 1. Initial open runs SCHEMA_SQL + ad-hoc + versioned up to latest (2)
    let store = Store::open(&path).unwrap();

    {
        let conn = store.conn.lock();
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, 2);

        let table_exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='boot_meta'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_exists, 1);
    }
    drop(store);

    // 2. Re-open and revert to BASELINE (1)
    let store = Store::open(&path).unwrap();
    {
        let mut conn = store.conn.lock();
        super::migrations::revert_versioned_migrations_for_test(&mut conn, 1).unwrap();

        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, 1);

        let table_exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='boot_meta'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_exists, 0);
    }
    drop(store);

    // 3. Re-open again (triggers upgrade back to 2)
    let store = Store::open(&path).unwrap();
    {
        let conn = store.conn.lock();
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, 2);

        let table_exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='boot_meta'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_exists, 1);
    }
}

#[test]
fn legacy_user_version_0_stamps_and_migrates() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");

    // Create a pre-migration DB: user_version=0, ad-hoc tables missing
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE audit_log (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                id TEXT NOT NULL,
                ts TEXT NOT NULL,
                kind TEXT NOT NULL,
                prev_hash TEXT NOT NULL,
                hash TEXT NOT NULL,
                meta_json TEXT NOT NULL,
                event_json TEXT NOT NULL
            );
            CREATE TABLE digest_items (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                id TEXT NOT NULL UNIQUE,
                ts TEXT NOT NULL,
                class TEXT NOT NULL,
                summary TEXT NOT NULL,
                resolved INTEGER NOT NULL DEFAULT 0
            );
            PRAGMA user_version = 0;",
        )
        .unwrap();

        // Insert a legacy audit row
        conn.execute(
            "INSERT INTO audit_log (id, ts, kind, prev_hash, hash, meta_json, event_json)
             VALUES ('01J00000000000000000000000', '2026-01-01T00:00:00Z', 'kernel.started',
                     'sha256:0000000000000000000000000000000000000000000000000000000000000000',
                     'sha256:0000000000000000000000000000000000000000000000000000000000000000',
                     '{}', '{}')",
            [],
        )
        .unwrap();
    }

    // Current Store::open runs apply_versioned_migrations
    let store = Store::open(&path).unwrap();

    {
        let conn = store.conn.lock();

        // Verify user_version is 2
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, 2);

        // Verify legacy row survived (aggregate_id default is 'system')
        let agg_id: String = conn
            .query_row(
                "SELECT aggregate_id FROM audit_log WHERE seq = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(agg_id, "system");

        // Verify boot_meta table was created
        let table_exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='boot_meta'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_exists, 1);
    }
}

#[test]
fn versioned_migrations_atomicity_rollback() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");

    // 1. Initial open to setup DB at version 2
    let store = Store::open(&path).unwrap();
    {
        let conn = store.conn.lock();
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, 2);
    }
    drop(store);

    // 2. Re-open and try to run a failing versioned migration (v99)
    let mut conn = Connection::open(&path).unwrap();
    let up_sql = "CREATE TABLE test_rollback (id INT); INSERT INTO nonexistent_table VALUES (1);";

    // Call the test ctor we added in migrations.rs
    let res = super::migrations::apply_single_migration_for_test(&mut conn, 99, up_sql);
    assert!(res.is_err(), "migration must fail");

    // 3. Assert atomicity:
    // - user_version must STILL be 2 (rolled back)
    // - test_rollback table must NOT exist (rolled back)
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(user_version, 2);

    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='test_rollback'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(table_exists, 0, "DDL must have rolled back");
}

#[test]
fn versioned_migrations_future_rejected() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");

    // 1. Manually create an empty database file and stamp user_version to 99
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA user_version = 99;").unwrap();
    }

    // 2. Try to open it via Store::open. It must fail with UnsupportedVersion
    let res = Store::open(&path);
    assert!(res.is_err(), "must fail");
    let err = res.err().unwrap();
    assert!(
        matches!(
            err,
            crate::store::StoreError::UnsupportedVersion {
                current: 99,
                latest: 2
            }
        ),
        "expected UnsupportedVersion, got {err:?}"
    );

    // 3. Verify that zero mutation occurred: the database remains empty (no tables)
    {
        let conn = Connection::open(&path).unwrap();
        let table_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 0, "no schema tables must be created");
    }
}

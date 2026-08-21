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
        assert_eq!(user_version, 10);

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
        assert_eq!(user_version, 10);

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
        assert_eq!(user_version, 10);

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
        assert_eq!(user_version, 10);
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
    assert_eq!(user_version, 10);

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
                latest: 10
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

/// AD-040/AD-041 v3 migration regression: a skills table created by the
/// pre-v3 ad-hoc lane (no schema_version column) must converge to the
/// current shape after Store::open, and a legacy row must hydrate with
/// schema_version=1 (the DEFAULT backfill) without fabrication.
#[test]
fn v2_to_v3_skills_schema_migration_backfills_legacy_rows() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");

    // 1. Create a pre-v3 DB: skills table WITHOUT schema_version column.
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS skills (
                id TEXT NOT NULL,
                version INTEGER NOT NULL,
                provenance TEXT NOT NULL,
                state TEXT NOT NULL,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                task_shape_json TEXT NOT NULL,
                visibility_json TEXT NOT NULL,
                content_digest TEXT NOT NULL,
                installed_at INTEGER NOT NULL,
                PRIMARY KEY(id, version)
            );
            PRAGMA user_version = 2;",
        )
        .unwrap();
        // Insert a legacy row.
        conn.execute(
            "INSERT INTO skills (id, version, provenance, state, title, body, \
             task_shape_json, visibility_json, content_digest, installed_at) \
             VALUES ('legacy_skill', 1, '\"shipped_seed\"', '\"installed\"', 'legacy', 'body', \
                     '[]', '{\"agents\":[],\"packs\":[]}', \
                     'sha256:0000000000000000000000000000000000000000000000000000000000000000', 0)",
            [],
        )
        .unwrap();
    }

    // 2. Open via Store::open — v3 migration adds schema_version DEFAULT 1.
    let store = Store::open(&path).unwrap();
    {
        let conn = store.conn.lock();
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, 10, "v3 through v10 migrations must have run");

        // Verify the column exists.
        let col_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('skills') WHERE name = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            col_count, 1,
            "schema_version column must exist after v3 migration"
        );
    }

    // 3. The legacy row must hydrate with schema_version=1 (DEFAULT backfill).
    let skill = crate::store::skill_store::get_skill(&store, "legacy_skill", 1)
        .unwrap()
        .expect("legacy skill must be readable after migration");
    assert_eq!(
        skill.schema_version, 1,
        "legacy row must hydrate with schema_version=1 (DEFAULT backfill)"
    );
    assert_eq!(skill.id, "legacy_skill");
    assert_eq!(skill.version, 1);
}

/// v4 rewrites the order-compared `TEXT` timestamp columns to the fixed
/// nine-digit form. A legacy whole-second `next_attempt_at` is the value that
/// used to sort *after* a later sub-second `now`, hiding a due dead letter.
#[test]
fn v4_normalizes_legacy_timestamp_renderings_idempotently() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    let store = Store::open(&path).unwrap();
    {
        let mut conn = store.conn.lock();
        // Re-introduce the three legacy renderings jiff's Display can emit.
        conn.execute_batch(
            "INSERT INTO notify_dead_letters \
               (id, enqueued_at, owner_surface_json, text_ref, task_grant_id, digest_item_ids, attempts, \
                next_attempt_at, state) \
             VALUES \
               ('whole', '2026-01-01T00:00:00Z', NULL, 'r', '', '', 0, '2026-01-01T00:00:00Z', 'pending'), \
               ('short', '2026-01-01T00:00:00.5Z', NULL, 'r', '', '', 0, '2026-01-01T00:00:00.5Z', 'pending'), \
               ('full',  '2026-01-01T00:00:00.123456789Z', NULL, 'r', '', '', 0, '2026-01-01T00:00:00.123456789Z', 'pending');",
        )
        .unwrap();
        // Roll the stamp back so the next open re-applies v4 over these rows.
        // Use the documented down path (not a raw PRAGMA) so the v10 rebuild's
        // `chat_id` source column is restored and the subsequent re-up runs.
        super::migrations::revert_versioned_migrations_for_test(&mut conn, 3).unwrap();
    }
    drop(store);

    let store = Store::open(&path).unwrap();
    let rendered: Vec<String> = {
        let conn = store.conn.lock();
        let mut stmt = conn
            .prepare("SELECT next_attempt_at FROM notify_dead_letters ORDER BY id")
            .unwrap();
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows
    };
    assert_eq!(
        rendered,
        vec![
            "2026-01-01T00:00:00.123456789Z".to_string(),
            "2026-01-01T00:00:00.500000000Z".to_string(),
            "2026-01-01T00:00:00.000000000Z".to_string(),
        ],
        "every legacy rendering must become the fixed nine-digit form"
    );
    assert!(
        rendered
            .iter()
            .all(|value| value.len() == rendered[0].len()),
        "fixed width is what makes SQLite's byte comparison a valid time order"
    );
    // The whole-second row is the regression: it must now sort before a later
    // sub-second instant, which its legacy rendering did not.
    let whole = &rendered[2];
    let later = crate::store::sql_timestamp("2026-01-01T00:00:00.000000001Z".parse().unwrap());
    assert!(whole.as_str() < later.as_str());
    drop(store);

    // Re-running the migration over already-canonical rows changes nothing.
    let store = Store::open(&path).unwrap();
    {
        let mut conn = store.conn.lock();
        crate::store::migrations::apply_single_migration_for_test(
            &mut conn,
            4,
            crate::store::migrations_versioned::timestamp_normalization_sql_for_test(),
        )
        .unwrap();
        let again: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT next_attempt_at FROM notify_dead_letters ORDER BY id")
                .unwrap();
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        };
        assert_eq!(again, rendered, "the normalization must be idempotent");
    }
}

//! Owner-surface migration tests (#129, versioned entries v7 and v8), split
//! from `migration_tests.rs` to keep every file under the 500-line gate.
//!
//! v8 (paused `CHECK`) and v9 (retiring `bound_chat_id`) are full table
//! rebuilds. A rebuild enumerates columns explicitly, so it silently drops any
//! column a sibling change added but the list does not name — invisible in
//! review, loud only at runtime. The first test below is the permanent guard
//! against exactly that; the rest pin the cutover's own properties.

use std::collections::BTreeSet;

use rusqlite::{params, Connection};
use tempfile::tempdir;

use super::{migrations, Store};

fn column_names(conn: &Connection, table: &str) -> BTreeSet<String> {
    let mut stmt = conn
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .unwrap();
    let names = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<BTreeSet<_>, _>>()
        .unwrap();
    names
}

/// Bring a file to the schema as it stands *before* #129's versioned entries:
/// the `SCHEMA_SQL` baseline plus every additive column the ad-hoc lane
/// converges, stamped at the last pre-#129 version.
fn open_at_pre_129_schema(path: &std::path::Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    // The live `SCHEMA_SQL` / `standing_rules_schema::ensure_schema` already
    // carry #129's shape, so a genuine pre-#129 file has to be built from the
    // *old* table definitions: `bound_chat_id`, and a free-form `status` with
    // no CHECK. Everything else converges through the ad-hoc lane below,
    // exactly as it would on a real upgrade.
    conn.execute_batch(
        "CREATE TABLE task_grants (
            id TEXT PRIMARY KEY,
            task_token TEXT NOT NULL UNIQUE,
            expires_at TEXT NOT NULL,
            grant_json TEXT NOT NULL,
            pending_message_digest TEXT NOT NULL,
            bound_chat_id INTEGER NOT NULL
        );
        CREATE TABLE standing_rules (
            rule_id TEXT PRIMARY KEY,
            artifact_id TEXT NOT NULL,
            version INTEGER NOT NULL,
            action_id TEXT NOT NULL,
            rule_json TEXT NOT NULL,
            quota_max INTEGER NOT NULL,
            quota_window_secs INTEGER NOT NULL,
            rate_max INTEGER NOT NULL,
            rate_window_secs INTEGER NOT NULL,
            expires_after_secs INTEGER NOT NULL,
            dark_window_timeout_secs INTEGER,
            dark_window_default TEXT,
            status TEXT NOT NULL,
            activated_at INTEGER NOT NULL,
            last_used_at INTEGER,
            revoked_at INTEGER,
            needs_review_since INTEGER
        );
        CREATE TABLE standing_rule_pending_actions (
            pending_id TEXT PRIMARY KEY,
            rule_id TEXT NOT NULL,
            rule_version INTEGER NOT NULL,
            task_grant_id TEXT NOT NULL,
            action_id TEXT NOT NULL,
            bound_chat_id INTEGER NOT NULL,
            payload_ref_json TEXT,
            dark_window_default TEXT NOT NULL CHECK(dark_window_default IN ('allow', 'deny')),
            request_fingerprint TEXT NOT NULL,
            requested_at INTEGER NOT NULL,
            resolved_at INTEGER,
            resolution TEXT CHECK(resolution IN ('allowed', 'denied', 'stale')),
            dispatch_state TEXT NOT NULL DEFAULT 'none'
                CHECK(dispatch_state IN ('none', 'claimed', 'dispatched')),
            token_consumed_at INTEGER,
            dispatch_receipt_digest TEXT
        );",
    )
    .unwrap();
    conn.execute_batch(super::SCHEMA_SQL).unwrap();
    migrations::apply_ad_hoc_migrations(&conn).unwrap();
    conn.execute_batch("PRAGMA user_version = 7;").unwrap();
    conn
}

/// THE GUARD. Snapshot every column the live schema has before #129's
/// rebuilds run, then run them, then assert nothing was lost except the one
/// column the cutover deliberately retires.
///
/// This is deliberately written against `PRAGMA table_info` rather than a
/// hand-listed set of column names, so it keeps working — and keeps failing
/// loudly — when a future change adds a column to either table. It is the
/// standing answer to "an explicit-column rebuild silently dropped a sibling
/// change's column": #128's `reviewed_scope_digest`/`compatibility_digest` and
/// #135's `dark_window_max_pending` are covered by construction, not by name.
#[test]
fn versioned_rebuilds_preserve_every_pre_existing_column() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");

    let (rules_before, pending_before, grants_before) = {
        let conn = open_at_pre_129_schema(&path);
        let snapshot = (
            column_names(&conn, "standing_rules"),
            column_names(&conn, "standing_rule_pending_actions"),
            column_names(&conn, "task_grants"),
        );
        drop(conn);
        snapshot
    };
    // Sanity: the baseline really does contain the sibling changes' columns,
    // so a passing assertion below is evidence and not a vacuous truth.
    assert!(rules_before.contains("dark_window_max_pending"));
    assert!(pending_before.contains("reviewed_scope_digest"));
    assert!(pending_before.contains("compatibility_digest"));
    assert!(pending_before.contains("bound_chat_id"));
    assert!(grants_before.contains("bound_chat_id"));

    let store = Store::open(&path).unwrap();
    let conn = store.conn.lock();
    let rules_after = column_names(&conn, "standing_rules");
    let pending_after = column_names(&conn, "standing_rule_pending_actions");
    let grants_after = column_names(&conn, "task_grants");

    // `standing_rules` is rebuilt only to gain a CHECK: nothing is retired.
    let lost: Vec<_> = rules_before.difference(&rules_after).collect();
    assert!(
        lost.is_empty(),
        "v8 rebuild dropped standing_rules column(s): {lost:?}"
    );

    // `bound_chat_id` is the one column #129 deliberately retires. Every other
    // pre-existing column MUST survive both rebuilds.
    for (table, before, after) in [
        (
            "standing_rule_pending_actions",
            &pending_before,
            &pending_after,
        ),
        ("task_grants", &grants_before, &grants_after),
    ] {
        let lost: Vec<_> = before
            .difference(after)
            .filter(|c| c.as_str() != "bound_chat_id")
            .collect();
        assert!(
            lost.is_empty(),
            "v9 rebuild dropped {table} column(s) it must have carried: {lost:?}"
        );
        assert!(
            !after.contains("bound_chat_id"),
            "{table} still carries the retired bound_chat_id"
        );
        assert!(
            after.contains("owner_surface_json"),
            "{table} must carry the channel-neutral owner binding"
        );
    }
}

/// The rebuilds must be data-preserving, not just shape-preserving: a row
/// present before the destructive migrations is still there after, with its
/// sibling-change column values intact.
#[test]
fn versioned_rebuilds_preserve_row_data_including_sibling_columns() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    {
        let conn = open_at_pre_129_schema(&path);
        conn.execute(
            "INSERT INTO standing_rules (
                rule_id, artifact_id, version, action_id, rule_json,
                quota_max, quota_window_secs, rate_max, rate_window_secs,
                expires_after_secs, status, activated_at,
                reviewed_scope_digest, dark_window_max_pending
            ) VALUES ('r1', 'a1', 1, 'email.create_draft', '{}', 5, 60, 5, 60, 0,
                      'active', 1, 'sha256:scope', 3)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO task_grants \
             (id, task_token, expires_at, grant_json, pending_message_digest, bound_chat_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                "01J00000000000000000000000",
                "hashed-token",
                "2099-01-01T00:00:00.000000000Z",
                "{}",
                format!("sha256:{}", "0".repeat(64)),
                555_i64,
            ],
        )
        .unwrap();
    }

    let store = Store::open(&path).unwrap();
    let conn = store.conn.lock();
    let (status, scope, max_pending): (String, Option<String>, Option<i64>) = conn
        .query_row(
            "SELECT status, reviewed_scope_digest, dark_window_max_pending \
             FROM standing_rules WHERE rule_id = 'r1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(status, "active");
    assert_eq!(scope.as_deref(), Some("sha256:scope"));
    assert_eq!(
        max_pending,
        Some(3),
        "#135's reviewed exception allowance must survive #129's rebuild"
    );
    let grants: i64 = conn
        .query_row("SELECT COUNT(*) FROM task_grants", [], |row| row.get(0))
        .unwrap();
    assert_eq!(grants, 1, "v9 must not drop legacy grant rows");
}

/// The paused status is DB-enforced after v7, and the values outside the
/// reviewed set are refused rather than silently stored as free-form text.
#[test]
fn v8_enforces_the_paused_status_set() {
    let store = Store::open_in_memory().unwrap();
    let conn = store.conn.lock();
    let err = conn.execute(
        "INSERT INTO standing_rules (
            rule_id, artifact_id, version, action_id, rule_json,
            quota_max, quota_window_secs, rate_max, rate_window_secs,
            expires_after_secs, status, activated_at
        ) VALUES ('r2', 'a2', 1, 'email.create_draft', '{}', 1, 60, 1, 60, 0, 'whatever', 1)",
        [],
    );
    assert!(
        err.is_err(),
        "a status outside the reviewed set must be refused by the CHECK"
    );
}

/// A pre-v9 grant has no authenticated owner surface. The migration cannot
/// derive one from the chat integer it dropped, so every read of that row
/// fails closed instead of authorizing replies to an unverified target.
#[test]
fn legacy_grant_without_an_owner_surface_fails_closed() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    {
        let conn = open_at_pre_129_schema(&path);
        conn.execute(
            "INSERT INTO task_grants \
             (id, task_token, expires_at, grant_json, pending_message_digest, bound_chat_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                "01J00000000000000000000000",
                super::budget_support::hash_task_token("legacy-token"),
                "2099-01-01T00:00:00.000000000Z",
                "{}",
                format!("sha256:{}", "0".repeat(64)),
                555_i64,
            ],
        )
        .unwrap();
    }

    let store = Store::open(&path).unwrap();
    let err = store
        .find_task_grant_by_token("legacy-token")
        .expect_err("a legacy grant with no owner surface must not resolve");
    assert!(
        matches!(err, super::StoreError::BadOwnerSurface(_)),
        "expected BadOwnerSurface, got {err:?}"
    );
}

/// The documented AD-139 downgrade restores the pre-v9 column on both tables
/// so an older binary can read the file again.
#[test]
fn v9_downgrade_restores_the_legacy_column() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    let store = Store::open(&path).unwrap();
    let mut conn = store.conn.lock();
    migrations::revert_versioned_migrations_for_test(&mut conn, 8).unwrap();
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(user_version, 8);
    for table in ["task_grants", "standing_rule_pending_actions"] {
        assert!(
            column_names(&conn, table).contains("bound_chat_id"),
            "{table} must regain bound_chat_id on downgrade"
        );
    }
}

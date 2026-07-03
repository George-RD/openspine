//! Ad-hoc, no-`PRAGMA user_version` migrations for schema changes made
//! after a `data/kernel.db` may already exist on disk (see `store/mod.rs`'s
//! module doc comment: `CREATE TABLE IF NOT EXISTS` in `SCHEMA_SQL` only
//! ever helps a brand-new file). Split out of `store/mod.rs` to keep that
//! file under the 500-line gate — mirrors the `budget_support`/
//! `gate_support` split.

use rusqlite::Connection;

use super::StoreError;

/// Run `sql` (expected to be an idempotent `ALTER TABLE ... ADD COLUMN
/// ...`), treating SQLite's "duplicate column name" failure as success —
/// that just means this migration already ran against this file on an
/// earlier `open()`. Any other error still propagates.
fn add_column_if_missing(conn: &Connection, sql: &str) -> Result<(), StoreError> {
    match conn.execute(sql, []) {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(_, Some(msg)))
            if msg.contains("duplicate column name") =>
        {
            Ok(())
        }
        Err(err) => Err(err.into()),
    }
}

pub(super) fn apply_ad_hoc_migrations(conn: &Connection) -> Result<(), StoreError> {
    // D-040 follow-up: `action_requests.used` backs
    // `try_consume_action_request`'s single-approval guard, added after
    // this table first shipped.
    add_column_if_missing(
        conn,
        "ALTER TABLE action_requests ADD COLUMN used INTEGER NOT NULL DEFAULT 0",
    )?;
    // Review follow-up on D-046: `grant_counters.model_calls` backs the
    // atomic `try_count_model_call` upsert (same TOCTOU-avoidance rationale
    // as `artifact_puts`), added after this table first shipped with only
    // `artifact_puts`.
    add_column_if_missing(
        conn,
        "ALTER TABLE grant_counters ADD COLUMN model_calls INTEGER NOT NULL DEFAULT 0",
    )?;
    // 5b: `proposed_artifacts` (in its sibling module, for the size gate).
    super::proposed_artifacts::ensure_schema(conn)
}

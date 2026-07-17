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
    // AD-105: aggregate bus coordinates on audit_log for existing DBs.
    // CREATE TABLE IF NOT EXISTS only helps brand-new files; these columns
    // were added after audit_log first shipped.
    add_column_if_missing(
        conn,
        "ALTER TABLE audit_log ADD COLUMN aggregate_id TEXT NOT NULL DEFAULT 'system'",
    )?;
    add_column_if_missing(
        conn,
        "ALTER TABLE audit_log ADD COLUMN aggregate_seq INTEGER NOT NULL DEFAULT 0",
    )?;
    // AD-105: durable consumer checkpoints + bus indexes (idempotent replay).
    // Indexes live here (not SCHEMA_SQL) so legacy DBs get columns first.
    // - idx_audit_id: unique event IDs even if a legacy table lacked UNIQUE.
    // - idx_audit_aggregate: lookup by stream.
    // - idx_audit_aggregate_seq_unique: per-aggregate monotonicity for new
    //   rows (seq > 0); partial so legacy sentinel seq=0 may repeat.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS consumer_checkpoints (
            consumer_id TEXT PRIMARY KEY,
            last_acked_global_seq INTEGER NOT NULL DEFAULT 0,
            checkpoint_json TEXT NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_audit_id
            ON audit_log (id);
        CREATE INDEX IF NOT EXISTS idx_audit_aggregate
            ON audit_log (aggregate_id, aggregate_seq);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_audit_aggregate_seq_unique
            ON audit_log (aggregate_id, aggregate_seq)
            WHERE aggregate_seq > 0;",
    )?;
    // AD-138: digest, notification dead-letter, and connector counter tables.
    super::failure_surfacing_types::ensure_schema(conn)?;
    add_column_if_missing(
        conn,
        "ALTER TABLE notify_dead_letters ADD COLUMN claimed_until TEXT",
    )?;
    add_column_if_missing(
        conn,
        "ALTER TABLE notify_dead_letters ADD COLUMN task_grant_id TEXT",
    )?;
    add_column_if_missing(
        conn,
        "ALTER TABLE notify_dead_letters ADD COLUMN digest_item_ids TEXT NOT NULL DEFAULT ''",
    )?;
    add_column_if_missing(
        conn,
        "ALTER TABLE notify_dead_letters ADD COLUMN claim_token TEXT",
    )?;
    // implement-failure-surfacing-contract: preserve the audit contract across
    // dead-letter retries of `/digest <ULID>` detail deliveries. Generic
    // notifications leave these NULL; nullable => legacy rows unaffected.
    add_column_if_missing(
        conn,
        "ALTER TABLE notify_dead_letters ADD COLUMN semantic_kind TEXT",
    )?;
    add_column_if_missing(
        conn,
        "ALTER TABLE notify_dead_letters ADD COLUMN detail_ref TEXT",
    )?;
    add_column_if_missing(
        conn,
        "ALTER TABLE notify_dead_letters ADD COLUMN page_index INTEGER",
    )?;
    add_column_if_missing(
        conn,
        "ALTER TABLE notify_dead_letters ADD COLUMN page_count INTEGER",
    )?;
    add_column_if_missing(
        conn,
        "ALTER TABLE notify_dead_letters ADD COLUMN availability_outcome TEXT",
    )?;
    // 5b: `proposed_artifacts` (in its sibling module, for the size gate).
    // implement-failure-surfacing-contract: sensitive digest detail moves to
    // the encrypted artifact store; SQLite keeps only a bounded non-sensitive
    // summary + the artifact hash ref (reuses the `text_ref` DLQ convention).
    // Idempotent: an existing table already has the column.
    add_column_if_missing(conn, "ALTER TABLE digest_items ADD COLUMN text_ref TEXT")?;
    // Legacy digest rows predate encrypted text refs. Fail closed by
    // replacing their plaintext summary with a fixed class-derived label;
    // never fabricate encryption for bytes whose provenance is unknown.
    conn.execute(
        "UPDATE digest_items SET summary = '[' || class || '] legacy failure detail unavailable' WHERE text_ref IS NULL",
        [],
    )?;
    super::proposed_artifacts::ensure_schema(conn)?;
    // `define-lineage-and-eval-store`: nullable lineage column on
    // proposed_artifacts (non-retrofittable set). No DEFAULT — legacy rows
    // keep NULL (unknown provenance). Unknown MUST NOT be rewritten as root.
    add_column_if_missing(
        conn,
        "ALTER TABLE proposed_artifacts ADD COLUMN lineage_json TEXT",
    )?;
    // `define-lineage-and-eval-store`: eval-verdict/fitness store as its own
    // `define-lineage-and-eval-store`: eval-verdict/fitness store as its own
    // indexed table (non-retrofittable set; AD-111 verdict landing).
    super::eval_verdict_store::ensure_schema(conn)?;
    super::workflow_timers::ensure_schema(conn)?;
    // AD-143: durable global per-day spend ledger (kernel-wide admission boundary).
    super::spend::ensure_schema(conn)?;
    add_column_if_missing(
        conn,
        "ALTER TABLE daily_spend ADD COLUMN alert_state INTEGER NOT NULL DEFAULT 0",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS workflow_step_registry (
            run_id TEXT NOT NULL,
            step_id TEXT NOT NULL,
            pending_seq INTEGER NOT NULL,
            receipt_seq INTEGER,
            completed_seq INTEGER,
            PRIMARY KEY(run_id, step_id)
        );",
    )?;
    add_column_if_missing(
        conn,
        "ALTER TABLE workflow_step_registry ADD COLUMN receipt_seq INTEGER",
    )?;
    add_column_if_missing(
        conn,
        "ALTER TABLE workflow_step_registry ADD COLUMN completed_seq INTEGER",
    )
}

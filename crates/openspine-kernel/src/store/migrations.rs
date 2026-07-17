//! Schema migrations. Two lanes (AD-139):
//!
//! 1. The additive ad-hoc lane ([`apply_ad_hoc_migrations`]): idempotent
//!    `ALTER TABLE ... ADD COLUMN` / `CREATE TABLE IF NOT EXISTS` that runs on
//!    every open for both fresh and pre-existing files. It never destroys
//!    data, so a legacy `data/kernel.db` converges to the baseline schema
//!    without any row being rewritten or dropped. Split out of `store/mod.rs`
//!    to keep that file under the 500-line gate.
//!
//! 2. The versioned `PRAGMA user_version` lane
//!    ([`apply_versioned_migrations`]): stamps the baseline once the ad-hoc
//!    lane has converged the schema, then applies forward migrations — each
//!    with a documented `down` path (see [`revert_versioned_migrations_for_test`]).
//!    The first *destructive* (non-idempotent-additive) schema change migrates
//!    off the ad-hoc lane onto a versioned entry, per AD-139's upgrade path.

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

// ---- versioned PRAGMA user_version framework (AD-139) -------------------

/// Schema version corresponding to the additive ad-hoc lane in
/// [`apply_ad_hoc_migrations`]. A database whose `PRAGMA user_version` is
/// below this is either a legacy file (created before this framework existed)
/// or a brand-new file; in both cases the ad-hoc lane already converges its
/// schema to this baseline, so we stamp it here without touching any row.
pub(super) const BASELINE_USER_VERSION: i64 = 1;

/// One forward schema migration. The SQL script `up` and the corresponding
/// `PRAGMA user_version` stamp are executed and committed within a single
/// SQLite transaction, guaranteeing versioned schema atomicity. The inverse
/// `down` is held in the test-only [`VERSIONED_DOWNS`] map, keyed by version.
pub(super) struct VersionedMigration {
    pub version: i64,
    pub up: &'static str,
}

/// Forward versioned migrations beyond [`BASELINE_USER_VERSION`].
/// Entry v2 adds the `boot_meta` table that backs boot clock-regression
/// detection ([`super::boot_clock`]); it is purely additive and reversible.
pub(super) const VERSIONED_MIGRATIONS: &[VersionedMigration] = &[VersionedMigration {
    version: 2,
    up: "CREATE TABLE IF NOT EXISTS boot_meta (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );",
}];

/// Inverse `down` SQL for each versioned migration (AD-139 downgrade path),
/// test-only — production never reverts. Kept in lockstep with
/// [`VERSIONED_MIGRATIONS`] by version.
#[cfg(test)]
const VERSIONED_DOWNS: &[(i64, &str)] = &[(2, "DROP TABLE IF EXISTS boot_meta;")];

/// Latest reachable schema version: the ad-hoc baseline, or the highest
/// versioned migration if any exist.
pub(super) fn latest_user_version() -> i64 {
    VERSIONED_MIGRATIONS
        .iter()
        .map(|m| m.version)
        .max()
        .unwrap_or(BASELINE_USER_VERSION)
        .max(BASELINE_USER_VERSION)
}

fn read_user_version(conn: &Connection) -> Result<i64, StoreError> {
    Ok(conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))?)
}

fn set_user_version(conn: &Connection, version: i64) -> Result<(), StoreError> {
    conn.execute_batch(&format!("PRAGMA user_version = {version}"))?;
    Ok(())
}

/// Preserve legacy DBs verbatim, then advance the version stamp.
///
/// The ad-hoc lane runs first (unchanged from pre-AD-139 behavior) so a legacy
/// on-disk DB converges to the baseline schema exactly as it always did — no
/// row is rewritten or dropped. We then stamp [`BASELINE_USER_VERSION`] (a
/// no-op once stamped) and apply every forward versioned migration in order,
/// advancing `PRAGMA user_version` after each.
fn apply_single_migration_inner(
    conn: &mut Connection,
    version: i64,
    up_sql: &str,
) -> Result<(), StoreError> {
    let tx = conn.transaction()?;
    tx.execute_batch(up_sql)?;
    tx.execute_batch(&format!("PRAGMA user_version = {}", version))?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
pub(super) fn apply_single_migration_for_test(
    conn: &mut Connection,
    version: i64,
    up_sql: &str,
) -> Result<(), StoreError> {
    apply_single_migration_inner(conn, version, up_sql)
}

/// Preserve legacy DBs verbatim, then advance the version stamp.
///
/// The ad-hoc lane runs first (unchanged from pre-AD-139 behavior) so a legacy
/// on-disk DB converges to the baseline schema exactly as it always did — no
/// row is rewritten or dropped. We then stamp [`BASELINE_USER_VERSION`] (a
/// no-op once stamped) and apply every forward versioned migration in order,
/// advancing `PRAGMA user_version` after each.
pub(super) fn apply_versioned_migrations(conn: &mut Connection) -> Result<(), StoreError> {
    let current = read_user_version(conn)?;
    let latest = latest_user_version();
    if current > latest {
        return Err(StoreError::UnsupportedVersion { current, latest });
    }
    conn.execute_batch(super::SCHEMA_SQL)?;
    apply_ad_hoc_migrations(conn)?;
    if current < BASELINE_USER_VERSION {
        set_user_version(conn, BASELINE_USER_VERSION)?;
    }
    let applied_from = current.max(BASELINE_USER_VERSION);
    for m in VERSIONED_MIGRATIONS
        .iter()
        .filter(|m| m.version > applied_from)
    {
        apply_single_migration_inner(conn, m.version, m.up)?;
    }
    Ok(())
}

/// Test-only: revert every versioned migration above `target` in reverse
/// order, applying each `down` and decrementing `PRAGMA user_version`. Proves
/// the documented AD-139 downgrade path end-to-end. Never runs in production.
#[cfg(test)]
pub(super) fn revert_versioned_migrations_for_test(
    conn: &mut Connection,
    target: i64,
) -> Result<(), StoreError> {
    let target = target.max(BASELINE_USER_VERSION);
    let current = read_user_version(conn)?;
    for (version, down) in VERSIONED_DOWNS
        .iter()
        .filter(|(v, _)| *v > target && *v <= current)
        .rev()
    {
        let tx = conn.transaction()?;
        tx.execute_batch(down)?;
        tx.execute_batch(&format!("PRAGMA user_version = {}", version - 1))?;
        tx.commit()?;
    }
    Ok(())
}

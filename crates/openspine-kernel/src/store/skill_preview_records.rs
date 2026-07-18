//! Skill preview records (AD-041/AD-110): durable record of the provenance +
//! digest the owner was shown before approving/rejecting a mined skill.
//!
//! When the owner views a skill via `/promote <id> <version>`, a preview
//! record is persisted with the exact content_digest shown AND the owner
//! principal who viewed it. When the owner approves, the decision is bound to
//! that preview record by both digest and owner principal:
//! `promote_skill`/`reject_skill` consume the record inside their own
//! transaction, so the decision can only land for the same digest the owner
//! previewed and only from the same owner principal. This prevents a TOCTOU
//! where the skill body changes between preview and approval, and stops a
//! preview recorded by one owner context from being consumed by another.

use jiff::Timestamp;
use openspine_schemas::digest::Digest;
use rusqlite::params;
use rusqlite::Transaction;
use ulid::Ulid;

use super::StoreError;

pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS skill_preview_records (
            id TEXT NOT NULL PRIMARY KEY,
            skill_id TEXT NOT NULL,
            version INTEGER NOT NULL,
            content_digest TEXT NOT NULL,
            previewed_at INTEGER NOT NULL,
            consumed INTEGER NOT NULL DEFAULT 0,
            provenance_summary TEXT NOT NULL DEFAULT '',
            prior_diff_summary TEXT NOT NULL DEFAULT '',
            current_diff_summary TEXT NOT NULL DEFAULT '',
            rendered_summary TEXT NOT NULL DEFAULT '',
            owner_principal TEXT NOT NULL DEFAULT ''
         );
         CREATE INDEX IF NOT EXISTS idx_preview_records_skill
            ON skill_preview_records (skill_id, version);",
    )?;
    // Databases created before the bound-principal columns existed: add them
    // idempotently so a prior preview table still loads.
    migrate_bound_columns(conn)?;
    Ok(())
}

fn migrate_bound_columns(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    for (name, sql) in [
        (
            "provenance_summary",
            "ALTER TABLE skill_preview_records ADD COLUMN provenance_summary TEXT NOT NULL DEFAULT ''",
        ),
        (
            "prior_diff_summary",
            "ALTER TABLE skill_preview_records ADD COLUMN prior_diff_summary TEXT NOT NULL DEFAULT ''",
        ),
        (
            "current_diff_summary",
            "ALTER TABLE skill_preview_records ADD COLUMN current_diff_summary TEXT NOT NULL DEFAULT ''",
        ),
        (
            "rendered_summary",
            "ALTER TABLE skill_preview_records ADD COLUMN rendered_summary TEXT NOT NULL DEFAULT ''",
        ),
        (
            "owner_principal",
            "ALTER TABLE skill_preview_records ADD COLUMN owner_principal TEXT NOT NULL DEFAULT ''",
        ),
    ] {
        let exists: bool = conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM pragma_table_info('skill_preview_records') WHERE name = '{name}'"
                ),
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count > 0)
            .unwrap_or(false);
        if !exists {
            conn.execute_batch(sql)?;
        }
    }
    Ok(())
}

/// Record the bounded owner-facing preview summary and its digest binding.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_skill_preview(
    conn: &rusqlite::Connection,
    skill_id: &str,
    version: u32,
    owner_principal: &str,
    content_digest: &Digest,
    provenance_summary: &str,
    prior_diff_summary: &str,
    current_diff_summary: &str,
    rendered_summary: &str,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO skill_preview_records \
         (id, skill_id, version, content_digest, previewed_at, consumed, \
          provenance_summary, prior_diff_summary, current_diff_summary, rendered_summary, owner_principal) \
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, ?9, ?10)",
        params![
            Ulid::new().to_string(),
            skill_id,
            version as i64,
            content_digest.as_str(),
            Timestamp::now().as_nanosecond() as i64,
            provenance_summary,
            prior_diff_summary,
            current_diff_summary,
            rendered_summary,
            owner_principal,
        ],
    )?;
    Ok(())
}

/// Verify and consume a preview record for the given skill id+version+digest
/// AND owner principal, inside an existing transaction. Returns Ok(()) if an
/// unconsumed matching record exists (and marks it consumed), or an error if
/// no matching unconsumed record exists. Because this runs inside the
/// promotion/rejection transaction, the digest+principal binding is atomic
/// with the shelf transition: a failed promotion leaves the preview spent
/// only if its own commit succeeds (verdict-before-effect), never dangling.
pub(crate) fn consume_skill_preview_conn(
    tx: &Transaction<'_>,
    skill_id: &str,
    version: u32,
    owner_principal: &str,
    content_digest: &Digest,
) -> Result<(), StoreError> {
    let affected = tx.execute(
        "UPDATE skill_preview_records SET consumed = 1 \
         WHERE skill_id = ?1 AND version = ?2 AND owner_principal = ?3 \
           AND content_digest = ?4 AND consumed = 0 AND rendered_summary <> ''",
        params![
            skill_id,
            version as i64,
            owner_principal,
            content_digest.as_str()
        ],
    )?;
    if affected == 0 {
        return Err(StoreError::SkillLifecycle(format!(
            "no unconsumed preview record for {skill_id} v{version} \
             owner {owner_principal} digest {content_digest}; \
             owner must preview before approving"
        )));
    }
    Ok(())
}

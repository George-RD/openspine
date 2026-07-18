//! Owner promotion tap decision record (AD-041/AD-110).
//!
//! `skill_promotion_decisions` is the durable record of the owner's explicit
//! approve/reject decision that lands (or refuses) a mined skill on the
//! approved shelf. Every row is written in the SAME transaction as the
//! skill's state transition (see `skill_store::promote_skill`/`reject_skill`),
//! so the decision and the shelf state can never diverge (atomic owner-tap +
//! activation). This is distinct from the AD-110 evaluator verdict (which
//! records the automated review in `eval_verdict_store`); this table is the
//! owner's authorization record.
//!
//! AD-041 states "one decision per skill ever, not per use": the table
//! enforces this with a `UNIQUE(skill_id, version)` constraint — a second
//! owner tap on an already-decided skill version fails closed with a typed
//! error inside the same transaction, rather than silently persisting a
//! contradictory second row.

use jiff::Timestamp;
use openspine_schemas::digest::Digest;
use openspine_schemas::skill::SkillState;
use rusqlite::params;
use ulid::Ulid;

use super::StoreError;

pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS skill_promotion_decisions (
            id TEXT NOT NULL PRIMARY KEY,
            skill_id TEXT NOT NULL,
            version INTEGER NOT NULL,
            decision TEXT NOT NULL,
            owner_principal_id TEXT NOT NULL,
            content_digest TEXT NOT NULL,
            recorded_at INTEGER NOT NULL,
            result_state TEXT NOT NULL,
            UNIQUE(skill_id, version)
         );
         CREATE INDEX IF NOT EXISTS idx_promotion_decisions_skill
            ON skill_promotion_decisions (skill_id, version);
         -- Enforce AD-041 'one decision per skill version, ever' even on
         -- databases created before the UNIQUE column constraint existed.
         CREATE UNIQUE INDEX IF NOT EXISTS
            ux_promotion_decisions_skill_version
            ON skill_promotion_decisions (skill_id, version);",
    )?;
    Ok(())
}

fn is_unique_constraint_violation(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(ffi_err, Some(msg))
            if ffi_err.code == rusqlite::ErrorCode::ConstraintViolation
                && (msg.contains("UNIQUE") || msg.contains("unique") || msg.contains("constraint failed"))
    )
}

/// Persist the owner promotion tap decision row in the caller's transaction,
/// atomic with the shelf state transition it accompanies. `decision` is
/// `"approve"` or `"reject"` — the OWNER's intent, not the evaluator's
/// outcome: an owner `Approve` whose AD-110 review denies still persists
/// `decision="approve"` with `result_state=Rejected` (never mislabeled as an
/// owner rejection). Fails closed with a clear error if this skill
/// id+version already has a decision recorded (AD-041: one decision ever).
pub(super) fn persist_promotion_decision_conn(
    tx: &rusqlite::Transaction<'_>,
    skill_id: &str,
    version: u32,
    decision: &str,
    owner_principal_id: Ulid,
    content_digest: &Digest,
    result_state: SkillState,
) -> Result<(), StoreError> {
    let res = tx.execute(
        "INSERT INTO skill_promotion_decisions \
         (id, skill_id, version, decision, owner_principal_id, content_digest, \
          recorded_at, result_state) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            Ulid::new().to_string(),
            skill_id,
            version as i64,
            decision,
            owner_principal_id.to_string(),
            content_digest.as_str(),
            Timestamp::now().as_nanosecond() as i64,
            serde_json::to_string(&result_state)?,
        ],
    );
    match res {
        Ok(_) => Ok(()),
        Err(err) if is_unique_constraint_violation(&err) => Err(StoreError::SkillLifecycle(
            format!("skill {skill_id} v{version} already has an owner promotion decision recorded (AD-041: one decision per skill version, ever)"),
        )),
        Err(err) => Err(StoreError::from(err)),
    }
}

/// Test helper: return the recorded owner promotion-tap decisions for a
/// skill id+version, most-recent first. Used to prove the owner tap table
/// is durably written (atomic with activation) and correctly labeled.
#[cfg(test)]
pub(crate) fn recent_promotion_decisions_for_test(
    store: &super::Store,
    skill_id: &str,
    version: u32,
) -> Result<Vec<(String, String, String)>, StoreError> {
    let conn = store.conn.lock();
    let mut stmt = conn.prepare(
        "SELECT decision, owner_principal_id, result_state \
         FROM skill_promotion_decisions \
         WHERE skill_id = ?1 AND version = ?2 ORDER BY recorded_at DESC",
    )?;
    let rows = stmt.query_map(params![skill_id, version as i64], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

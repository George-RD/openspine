// The `skills` table is created in production via `ensure_schema` (called from
// the migrations). The write/transition API (insert/promote/reject) is
// exercised by `crate::skill::tests` and now reaches the production runtime
// through the `skill.context` action (see `api/skill_context.rs`), so the
// previously-allowed dead surface is mostly gone; a small residual remains
// until the full injection call site lands, and is allowed here.
#![allow(dead_code)]

//! SQLite store for the `skills` artifact table (AD-040/AD-041).
//!
//! One table holds every installed skill with its provenance and shelf
//! state. Inserts branch by provenance (trusted => `Installed` immediately;
//! [`SkillProvenance::MinerDistilled`] => `PendingReview`), and the reviewed
//! promotion transition is the *only* path that moves a `PendingReview` skill
//! to `Installed` (and it consumes an unforgeable
//! [`crate::skill::review::SkillReviewPassed`] token bound to the row's exact
//! content digest — see that module). The matcher (read path) can never
//! create a row; see [`super::skill_read_queries`].
//!
//! Version discipline (AD-041 install/update ceremony): content edits bump
//! `version`; installing or promoting a version atomically RETIRES every
//! lower-version `Installed` row for the same `id` (reusing
//! [`SkillState::Retired`] — the same terminal state used for owner
//! withdrawals). Activating a version is refused if a higher (or equal)
//! version already exists (for trusted installs, across ALL states — so a
//! stale lower version can never revive and a higher-or-equal version can
//! never be silently superseded by a downgrade; for promotion, only a higher
//! *Installed* version blocks — a higher version still under review does not).
//! The shelf therefore always exposes at most the highest active version per
//! `id`.
//!
//! The owner promotion tap (AD-041/AD-110) — the durable record of the
//! owner's approve/reject decision — lives in `skill_promotion_decisions`
//! (see that module); it is written in the same transaction as the shelf
//! state transition by `promote_skill`/`reject_skill` below.
//!
//! D-012 discipline: the skill `body` is competence text, never a private
//! payload, but we still store only its `sha256` digest (not the body bytes)
//! in any audit/verdict reference and never emit the body as plaintext into
//! the audit chain. The `content_digest` column is that binding.
//!
//! Schema-versioning (AD-040/AD-041, v3 migration): every row carries the
//! `schema_version` it was written under so hydration can fail closed on an
//! unsupported version instead of fabricating `1`. The ad-hoc `ensure_schema`
//! deliberately omits the column; the v3 versioned migration adds it (DEFAULT
//! 1) so a fresh or legacy table converges without a row rewrite.
use jiff::Timestamp;
use openspine_schemas::digest::Digest;
use openspine_schemas::skill::{Skill, SkillState};
use rusqlite::params;
use ulid::Ulid;

use super::{skill_read_queries, Store, StoreError};
use crate::skill::ceremony::CeremonyToken;
use rusqlite::OptionalExtension;

pub(crate) type SkillRow = (
    String,
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    i64, // schema_version (v3 migration column)
);

pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
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
         CREATE INDEX IF NOT EXISTS idx_skills_state
            ON skills (state);
         CREATE INDEX IF NOT EXISTS idx_skills_content_digest
            ON skills (content_digest);",
    )?;
    Ok(())
}

/// Fail-closed guard: only schema version 1 is supported. We do not silently
/// coerce — an unsupported version is rejected before any mutation/insert.
pub(super) fn validate_schema_version(schema_version: u32) -> Result<(), StoreError> {
    if schema_version == 1 {
        Ok(())
    } else {
        Err(StoreError::UnsupportedSkillSchemaVersion(schema_version))
    }
}

/// Retire (set `Retired`) every `Installed` row of the same `id` whose version
/// is lower than `new_version`. Runs inside the caller's transaction so the
/// retirement is atomic with whatever transition is promoting `new_version`.
/// Returns the number of rows retired (for an audit-trail count).
fn retire_older_installed_siblings(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
    new_version: u32,
) -> Result<usize, StoreError> {
    let retired = tx.execute(
        "UPDATE skills SET state = ?1 \
         WHERE id = ?2 AND state = ?3 AND version < ?4",
        params![
            serde_json::to_string(&SkillState::Retired)?,
            id,
            serde_json::to_string(&SkillState::Installed)?,
            new_version as i64,
        ],
    )?;
    Ok(retired)
}

/// Refuse to activate `new_version` if a higher (or equal) version of the
/// same `id` already exists in ANY state (PendingReview/Rejected/Retired/
/// Installed). This blocks both a stale lower version reviving and a
/// higher-or-equal version being silently superseded by a downgrade. Used by
/// `insert_skill` (trusted/UserInstalled commits straight to `Installed`).
fn guard_no_equal_or_higher_any_state(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
    new_version: u32,
) -> Result<(), StoreError> {
    let existing: Option<i64> = tx.query_row(
        "SELECT MAX(version) FROM skills WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    if let Some(ver) = existing {
        if ver >= new_version as i64 {
            return Err(StoreError::SkillLifecycle(format!(
                "skill {id} already has version {ver} (>= new version {new_version}); \
                 refusing install/update to avoid a stale or downgraded shelf"
            )));
        }
    }
    Ok(())
}

/// Refuse to promote `new_version` if a higher version of the same `id` is
/// already `Installed`. A higher version in a non-`Installed` state (e.g. a
/// still-pending review) does not block the promotion of this one — the
/// normal ceremony installs the highest reviewed version last.
fn guard_no_higher_installed(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
    new_version: u32,
) -> Result<(), StoreError> {
    let existing: Option<i64> = tx.query_row(
        "SELECT MAX(version) FROM skills WHERE id = ?1 AND state = ?2",
        params![id, serde_json::to_string(&SkillState::Installed)?],
        |r| r.get(0),
    )?;
    if let Some(ver) = existing {
        if ver >= new_version as i64 {
            return Err(StoreError::SkillLifecycle(format!(
                "skill {id} already has Installed version {ver}; refusing to \
                 activate version {new_version} (would revive stale visibility)"
            )));
        }
    }
    Ok(())
}

/// Insert a skill row inside a BEGIN IMMEDIATE transaction, branching the
/// initial state by provenance (AD-041), and audit-before-effect: a single
/// audit row records the install act under the write lock that the row
/// shares, so the two can never diverge. Trusted provenance commits straight
/// to `Installed` (after the monotonic guard across ALL states and an atomic
/// retire of any lower `Installed` version of the same `id`); mined
/// provenance lands `PendingReview`. Unsupported schema versions are rejected
/// fail-closed.
const MAX_SKILL_ID_BYTES: usize = 128;

pub(crate) fn insert_skill(
    store: &Store,
    skill: &Skill,
    now: Timestamp,
    _token: &CeremonyToken,
) -> Result<(), StoreError> {
    if skill.id.len() > MAX_SKILL_ID_BYTES {
        return Err(StoreError::SkillLifecycle(format!(
            "skill id exceeds {MAX_SKILL_ID_BYTES} bytes"
        )));
    }
    validate_schema_version(skill.schema_version)?;
    let initial_state = if skill.provenance.requires_promotion_review() {
        SkillState::PendingReview
    } else {
        SkillState::Installed
    };
    let mut skill = skill.clone();
    skill.state = initial_state;
    let now_nanos = now.as_nanosecond() as i64;

    let mut conn = store.conn.lock();
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if initial_state == SkillState::Installed {
        guard_no_equal_or_higher_any_state(&tx, &skill.id, skill.version)?;
        let retired = retire_older_installed_siblings(&tx, &skill.id, skill.version)?;
        if retired > 0 {
            Store::append_audit_conn(
                &tx,
                "skill.retired_prior_versions",
                None,
                None,
                Some(&format!(
                    "id={} retired_versions={} activated_version={} reason=superseded",
                    skill.id, retired, skill.version
                )),
                None,
                &[],
                &[],
            )?;
        }
    }
    tx.execute(
        "INSERT INTO skills \
         (id, version, provenance, state, title, body, task_shape_json, \
          visibility_json, content_digest, installed_at, schema_version) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            &skill.id,
            skill.version as i64,
            serde_json::to_string(&skill.provenance)?,
            serde_json::to_string(&skill.state)?,
            &skill.title,
            &skill.body,
            serde_json::to_string(&skill.task_shape)?,
            serde_json::to_string(&skill.visibility)?,
            skill.content_digest.as_str(),
            now_nanos,
            skill.schema_version as i64,
        ],
    )?;
    Store::append_audit_conn(
        &tx,
        "skill.installed",
        None,
        None,
        Some(&format!(
            "provenance={:?} state={:?} schema_version={} digest={}",
            skill.provenance, skill.state, skill.schema_version, skill.content_digest
        )),
        None,
        &[],
        &[],
    )?;
    tx.commit()?;
    Ok(())
}

/// Transition a `PendingReview` miner-distilled skill to `Installed`,
/// consuming an unforgeable review-passed token bound to the row's exact
/// content digest. Returns the updated skill on success. Promotion atomically
/// retires any lower `Installed` version of the same `id` and refuses to
/// activate if a higher version is already `Installed`.
pub(crate) fn promote_skill(
    store: &Store,
    token: &crate::skill::review::SkillReviewPassed,
    owner_principal_id: Ulid,
    _ceremony_token: &CeremonyToken,
) -> Result<Skill, StoreError> {
    let mut conn = store.conn.lock();
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let existing: SkillRow = tx
        .query_row(
            "SELECT id, version, provenance, state, title, body, \
             task_shape_json, visibility_json, content_digest, installed_at, \
             schema_version \
             FROM skills WHERE id = ?1 AND version = ?2",
            params![token.skill_id(), token.version() as i64],
            skill_read_queries::read_skill_row,
        )
        .optional()?
        .ok_or_else(|| StoreError::SkillNotFound(token.skill_id().to_string()))?;
    let existing = skill_read_queries::map_skill_row(existing)?;
    if existing.state != SkillState::PendingReview {
        return Err(StoreError::SkillLifecycle(format!(
            "skill {} v{} is in state {:?}, not pending_review",
            existing.id, existing.version, existing.state
        )));
    }
    if *token.digest() != existing.content_digest {
        return Err(StoreError::SkillDigestMismatch(format!(
            "review token digest {} does not bind to stored skill digest {}",
            token.digest(),
            existing.content_digest
        )));
    }
    guard_no_higher_installed(&tx, &existing.id, existing.version)?;
    let retired = retire_older_installed_siblings(&tx, &existing.id, existing.version)?;
    // Digest- AND owner-principal-bound preview consumption: atomic with the
    // promotion so an approval can only land for the exact digest the owner
    // previewed, bound to the same owner principal (AD-041/AD-110).
    crate::store::skill_preview_records::consume_skill_preview_conn(
        &tx,
        &existing.id,
        existing.version,
        &owner_principal_id.to_string(),
        &existing.content_digest,
    )?;

    tx.execute(
        "UPDATE skills SET state = ?1 WHERE id = ?2 AND version = ?3",
        params![
            serde_json::to_string(&SkillState::Installed)?,
            &existing.id,
            existing.version as i64,
        ],
    )?;
    let mut audit_detail = format!(
        "digest={} evaluator=ad110_mined_promotion_review",
        existing.content_digest
    );
    if retired > 0 {
        audit_detail.push_str(&format!(" retired_prior_versions={retired}"));
    }
    Store::append_audit_conn(
        &tx,
        "skill.promoted",
        None,
        None,
        Some(&audit_detail),
        None,
        &[],
        &[],
    )?;

    // Durable owner-tap decision row, atomic with the activation. The
    // decision is "approve" — the OWNER's intent — regardless of the
    // evaluator outcome (which is captured separately in the eval-verdict
    // store); the result_state records the actual shelf outcome.
    super::skill_promotion_decisions::persist_promotion_decision_conn(
        &tx,
        &existing.id,
        existing.version,
        "approve",
        owner_principal_id,
        &existing.content_digest,
        SkillState::Installed,
    )?;

    // Test-only fault: simulate the commit failing after the verdict has
    // already been recorded (in the separate AD-110 transaction). The skill
    // must stay `PendingReview` — verdict-before-effect, atomic.
    #[cfg(test)]
    if store
        .fail_next_skill_promotion_tx
        .swap(false, std::sync::atomic::Ordering::SeqCst)
    {
        return Err(StoreError::SkillLifecycle(
            "injected promote transaction failure (test)".to_string(),
        ));
    }

    tx.commit()?;

    let mut promoted = existing;
    promoted.state = SkillState::Installed;
    Ok(promoted)
}

/// Transition a `PendingReview` miner-distilled skill to `Rejected`,
/// recording the motivation (never the skill body as plaintext). `decision`
/// is the owner's intent label persisted to the promotion-tap table:
/// `"reject"` for an explicit owner rejection, `"approve"` when the owner
/// approved but the AD-110 evaluator denied (so the record stays truthful
/// about intent without being mislabeled as a rejection).
pub(crate) fn reject_skill(
    store: &Store,
    skill_id: &str,
    version: u32,
    reason: &str,
    owner_principal_id: Ulid,
    decision: &str,
    _ceremony_token: &CeremonyToken,
) -> Result<(), StoreError> {
    let mut conn = store.conn.lock();
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let affected = tx.execute(
        "UPDATE skills SET state = ?1 \
         WHERE id = ?2 AND version = ?3 AND state = ?4",
        params![
            serde_json::to_string(&SkillState::Rejected)?,
            skill_id,
            version as i64,
            serde_json::to_string(&SkillState::PendingReview)?,
        ],
    )?;
    if affected == 0 {
        return Err(StoreError::SkillLifecycle(format!(
            "skill {skill_id} v{version} is not in pending_review"
        )));
    }
    let digest_str: String = tx.query_row(
        "SELECT content_digest FROM skills WHERE id = ?1 AND version = ?2",
        params![skill_id, version as i64],
        |r| r.get(0),
    )?;
    let digest = Digest::parse(&digest_str).map_err(|e| {
        StoreError::BadDigest(format!(
            "stored skill {skill_id} v{version} digest unparseable: {e}"
        ))
    })?;
    // Owner-principal-bound preview consumption, atomic with the rejection
    // (same digest + principal the owner previewed, never dangling).
    crate::store::skill_preview_records::consume_skill_preview_conn(
        &tx,
        skill_id,
        version,
        &owner_principal_id.to_string(),
        &digest,
    )?;
    // Durable owner-tap decision row, atomic with the rejection.
    super::skill_promotion_decisions::persist_promotion_decision_conn(
        &tx,
        skill_id,
        version,
        decision,
        owner_principal_id,
        &digest,
        SkillState::Rejected,
    )?;
    Store::append_audit_conn(
        &tx,
        "skill.rejected",
        None,
        None,
        Some(reason),
        None,
        &[],
        &[],
    )?;
    tx.commit()?;
    Ok(())
}
// Re-export the read-only helpers (kept in `skill_read_queries` to keep this
// write-path module focused) so existing call sites in tests/ceremony keep
// importing them from `store::skill_store`.
#[cfg(test)]
pub(crate) use skill_read_queries::count_skill_rows_for_test;
pub(crate) use skill_read_queries::get_skill;

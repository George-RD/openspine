//! Read-only skill queries: row hydration + the matcher's shelf read. This is
//! a pure read surface — it physically cannot insert or update a skill row, so
//! the matcher (which uses [`installed_skills_for_agent_and_pack`]) can never
//! install.

use openspine_schemas::digest::Digest;
use openspine_schemas::skill::{Skill, SkillState, SkillVisibility};
use rusqlite::{params, OptionalExtension};
use std::collections::HashMap;

use super::skill_store::SkillRow;
use super::Store;
use super::StoreError;

pub(super) fn read_skill_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SkillRow> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, i64>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, String>(3)?,
        row.get::<_, String>(4)?,
        row.get::<_, String>(5)?,
        row.get::<_, String>(6)?,
        row.get::<_, String>(7)?,
        row.get::<_, String>(8)?,
        row.get::<_, i64>(9)?,
        row.get::<_, i64>(10)?,
    ))
}

pub(super) fn map_skill_row(row: SkillRow) -> Result<Skill, StoreError> {
    let provenance = serde_json::from_str::<openspine_schemas::skill::SkillProvenance>(&row.2)
        .map_err(StoreError::Serde)?;
    let state = serde_json::from_str::<SkillState>(&row.3).map_err(StoreError::Serde)?;
    let task_shape: Vec<String> = serde_json::from_str(&row.6).map_err(StoreError::Serde)?;
    let visibility: SkillVisibility = serde_json::from_str(&row.7).map_err(StoreError::Serde)?;
    let content_digest = Digest::parse(row.8).map_err(|e| StoreError::BadDigest(e.to_string()))?;
    // Fail-closed hydration: range-check the stored i64 before widening and
    // validate against the supported set. A malformed/negative stored value
    // must NOT silently become `1` (which would defeat the unsupported-version
    // guard), so we reject it here rather than casting with `as`.
    let schema_version = u32::try_from(row.10)
        .map_err(|_| StoreError::BadDigest(format!("negative skill schema_version: {}", row.10)))?;
    super::skill_store::validate_schema_version(schema_version)?;
    Ok(Skill {
        id: row.0,
        schema_version,
        version: row.1 as u32,
        provenance,
        state,
        title: row.4,
        body: row.5,
        task_shape,
        visibility,
        content_digest,
    })
}

/// Read one exact skill id+version (used by the promotion path and tests).
pub(crate) fn get_skill(
    store: &Store,
    skill_id: &str,
    version: u32,
) -> Result<Option<Skill>, StoreError> {
    let conn = store.conn.lock();
    let row = conn
        .query_row(
            "SELECT id, version, provenance, state, title, body, \
             task_shape_json, visibility_json, content_digest, installed_at, \
             schema_version \
             FROM skills WHERE id = ?1 AND version = ?2",
            params![skill_id, version as i64],
            read_skill_row,
        )
        .optional()?;
    row.map(map_skill_row).transpose()
}

/// AD-042 matcher path: every skill currently on the approved shelf, scoped
/// to the named agent OR pack (deny-by-default — a skill must be visible to
/// at least one of them to ever be selected). Read-only — this query returns
/// rows; it never inserts or updates, so the matcher physically cannot
/// install.
///
/// Version discipline (security): the highest active version per `id` is
/// chosen FIRST across *all* `Installed` rows, and only that winner is then
/// checked for agent/pack visibility. A narrowing update that hides the new
/// version from an agent therefore hides the skill entirely for that agent —
/// it can never fall back to a stale, wider-visible older version.
pub(crate) fn installed_skills_for_agent_and_pack(
    store: &Store,
    agent_id: &str,
    pack_id: &str,
) -> Result<Vec<Skill>, StoreError> {
    let conn = store.conn.lock();
    let mut stmt = conn.prepare(
        "SELECT id, version, provenance, state, title, body, \
         task_shape_json, visibility_json, content_digest, installed_at, \
         schema_version \
         FROM skills WHERE state = ?1",
    )?;
    let rows = stmt.query_map(
        params![serde_json::to_string(&SkillState::Installed)?],
        read_skill_row,
    )?;
    // Step 1: keep only the highest version per id across ALL installed rows.
    let mut by_id: HashMap<String, Skill> = HashMap::new();
    for r in rows {
        let skill = map_skill_row(r?)?;
        by_id
            .entry(skill.id.clone())
            .and_modify(|cur| {
                if skill.version > cur.version {
                    *cur = skill.clone();
                }
            })
            .or_insert(skill);
    }
    // Step 2: only the winners are checked for agent/pack visibility.
    let mut out: Vec<Skill> = by_id
        .into_values()
        .filter(|skill| skill.visibility.is_visible_to(agent_id, pack_id))
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| b.version.cmp(&a.version)));
    Ok(out)
}

#[cfg(test)]
pub(crate) fn count_skill_rows_for_test(store: &Store) -> Result<usize, StoreError> {
    let conn = store.conn.lock();
    Ok(conn.query_row("SELECT COUNT(*) FROM skills", [], |row| {
        row.get::<_, i64>(0)
    })? as usize)
}

/// Find the highest version of `skill_id` that is lower than `current_version`,
/// in any state. Returns `None` when no prior version exists (first install).
/// Used by the `/promote` preview to show the actual prior content diff rather
/// than assuming `current_version - 1`.
pub(crate) fn highest_prior_version(
    store: &Store,
    skill_id: &str,
    current_version: u32,
) -> Result<Option<Skill>, StoreError> {
    let conn = store.conn.lock();
    let row = conn
        .query_row(
            "SELECT id, version, provenance, state, title, body, \
             task_shape_json, visibility_json, content_digest, installed_at, \
             schema_version \
             FROM skills WHERE id = ?1 AND version < ?2 AND state = ?3 \
             ORDER BY version DESC LIMIT 1",
            params![
                skill_id,
                current_version as i64,
                serde_json::to_string(&SkillState::Installed)?,
            ],
            read_skill_row,
        )
        .optional()?;
    row.map(map_skill_row).transpose()
}

/// Kernel-bound provenance token minted by `skill.context`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillContextSelection {
    pub id: ulid::Ulid,
    pub task_grant_id: ulid::Ulid,
    pub agent_id: String,
    pub pack_id: String,
    pub skill_id: String,
    pub skill_version: u32,
    pub task_class: String,
    pub expires_at: jiff::Timestamp,
    pub used: bool,
}

pub(crate) fn insert_skill_context_selection(
    store: &Store,
    selection: &SkillContextSelection,
) -> Result<(), StoreError> {
    let conn = store.conn.lock();
    conn.execute(
        "INSERT INTO skill_context_selections
         (id, task_grant_id, agent_id, pack_id, skill_id, skill_version, task_class, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            selection.id.to_string(),
            selection.task_grant_id.to_string(),
            selection.agent_id,
            selection.pack_id,
            selection.skill_id,
            selection.skill_version,
            selection.task_class,
            selection.expires_at.to_string(),
        ],
    )?;
    Ok(())
}
pub(crate) fn find_live_skill_context_selection(
    store: &Store,
    token_id: ulid::Ulid,
    grant_id: ulid::Ulid,
) -> Result<Option<SkillContextSelection>, StoreError> {
    live_skill_context_selections(store, grant_id).map(|selections| {
        selections
            .into_iter()
            .find(|selection| selection.id == token_id && !selection.used)
    })
}

pub(crate) fn live_skill_context_selections(
    store: &Store,
    grant_id: ulid::Ulid,
) -> Result<Vec<SkillContextSelection>, StoreError> {
    let conn = store.conn.lock();
    let mut stmt = conn.prepare(
        "SELECT id, task_grant_id, agent_id, pack_id, skill_id, skill_version,
                task_class, expires_at, used
         FROM skill_context_selections
         WHERE task_grant_id = ?1
         ORDER BY skill_id ASC, skill_version DESC, id ASC",
    )?;
    let now = jiff::Timestamp::now();
    let rows = stmt.query_map(params![grant_id.to_string()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, i64>(8)?,
        ))
    })?;
    let mut selections = Vec::new();
    for row in rows {
        let (id, task_grant_id, agent_id, pack_id, skill_id, version, task_class, expires, used) =
            row?;
        let expires_at = expires
            .parse::<jiff::Timestamp>()
            .map_err(|_| StoreError::BadDigest("invalid skill token expiry".into()))?;
        if expires_at <= now {
            continue;
        }
        selections.push(SkillContextSelection {
            id: id
                .parse()
                .map_err(|_| StoreError::BadDigest("invalid skill token id".into()))?,
            task_grant_id: task_grant_id
                .parse()
                .map_err(|_| StoreError::BadDigest("invalid grant id".into()))?,
            agent_id,
            pack_id,
            skill_id,
            skill_version: version as u32,
            task_class,
            expires_at,
            used: used != 0,
        });
    }
    Ok(selections)
}

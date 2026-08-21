//! The destructive dead-letter owner-surface migration owned by #217
//! (spec #208 D-006): cut the last `store -> telegram` reach by retiring the
//! Telegram-shaped `chat_id INTEGER` column from `notify_dead_letters` and
//! persisting the channel-neutral `OwnerSurfaceRef` as `owner_surface_json`
//! instead (the `task_grants.owner_surface_json` precedent).
//!
//! Unlike the v9 grant/pending cutover (`migration_owner_review`), this row is
//! backfilled rather than left NULL: every `notify_dead_letters` row belongs to
//! the single configured owner (the `idx_principal_owner_singleton` unique
//! index on `principals` enforces one owner), so a legacy `chat_id` promotes
//! deterministically to a `verified_telegram` surface carrying that chat id.
//! `OwnerSurfaceRef` deserializes by field name (`serde(deny_unknown_fields)`,
//! not by field order) and these rows are only ever deserialized, so the SQL
//! `json_object` backfill is deterministic and safe. A row for which no owner
//! principal exists backfills `principal_id = NULL`, which fails to deserialize
//! and is refused at read time (fail closed), never delivered.

/// Entry v10 rebuilds `notify_dead_letters`, replacing `chat_id INTEGER NOT
/// NULL` with `owner_surface_json TEXT` and backfilling each row's surface from
/// its legacy chat id plus the single owner principal. The `chat_id` column is
/// dropped by omission from the rebuilt table.
///
/// Column list = the `ensure_schema` baseline (16 columns after
/// `failure_surfacing_types::ensure_schema` and the ad-hoc lane converge) with
/// `chat_id` replaced by `owner_surface_json`: id, enqueued_at,
/// owner_surface_json, text_ref, task_grant_id, digest_item_ids, attempts,
/// next_attempt_at, claimed_until, claim_token, semantic_kind, detail_ref,
/// page_index, page_count, availability_outcome, state = 16. `notify_dead_letters`
/// carries no indexes, so none are recreated.
pub(super) const DEAD_LETTER_OWNER_SURFACE_UP: &str = "CREATE TABLE notify_dead_letters_new (
    id TEXT PRIMARY KEY,
    enqueued_at TEXT NOT NULL,
    owner_surface_json TEXT,
    text_ref TEXT NOT NULL,
    task_grant_id TEXT,
    digest_item_ids TEXT NOT NULL DEFAULT '',
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT NOT NULL,
    claimed_until TEXT,
    claim_token TEXT,
    semantic_kind TEXT,
    detail_ref TEXT,
    page_index INTEGER,
    page_count INTEGER,
    availability_outcome TEXT,
    state TEXT NOT NULL DEFAULT 'pending'
);
INSERT INTO notify_dead_letters_new (
    id, enqueued_at, owner_surface_json, text_ref, task_grant_id,
    digest_item_ids, attempts, next_attempt_at, claimed_until, claim_token,
    semantic_kind, detail_ref, page_index, page_count, availability_outcome, state
)
SELECT id, enqueued_at,
    json_object(
        'kind', 'telegram_private',
        'principal_id', (SELECT id FROM principals WHERE is_owner = 1 LIMIT 1),
        'thread_binding', NULL,
        'surface_id', CAST(chat_id AS TEXT)
    ),
    text_ref, task_grant_id,
    digest_item_ids, attempts, next_attempt_at, claimed_until, claim_token,
    semantic_kind, detail_ref, page_index, page_count, availability_outcome, state
FROM notify_dead_letters;
DROP TABLE notify_dead_letters;
ALTER TABLE notify_dead_letters_new RENAME TO notify_dead_letters;";

/// The documented AD-139 downgrade restores the pre-v10 `chat_id` column
/// additively (defaulted, because a surface reference carries no chat id
/// outside the Telegram adapter) while `owner_surface_json` stays — the shape
/// the ad-hoc lane converges a legacy file to. Test-only; production never
/// reverts.
#[cfg(test)]
pub(super) const DEAD_LETTER_OWNER_SURFACE_DOWN: &str =
    "ALTER TABLE notify_dead_letters ADD COLUMN chat_id INTEGER NOT NULL DEFAULT 0;";

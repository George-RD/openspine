//! The two destructive schema migrations owned by review lifecycle #129.
//!
//! Both are full table rebuilds, because SQLite cannot add a `CHECK` or drop a
//! column in place on every supported version. A rebuild enumerates columns
//! explicitly, which is the failure shape that silently loses a sibling
//! change's column: it is invisible in review and only loud at runtime. The
//! column lists below are therefore derived from the *live* tables — the
//! `ensure_schema` baseline in `store::standing_rules_schema` plus every
//! additive column the ad-hoc lane in `store::migrations` converges — and
//! `store::migration_owner_surface_tests::versioned_rebuilds_preserve_every_
//! pre_existing_column` is a permanent guard that fails if a future change
//! adds a column these lists do not name.

/// Entry v8 rebuilds `standing_rules` to add
/// `CHECK(status IN ('active','paused','needs_review','revoked'))`, so the
/// owner-controlled `paused` status is a typed, DB-enforced value rather than
/// free-form text. Every existing row is preserved verbatim (all stored status
/// values are already in the allowed set), so no data is rewritten or dropped.
///
/// Column list = 17 baseline + `reviewed_scope_digest` / `compatibility_digest`
/// (#128) + `dark_window_max_pending` (#135, the reviewed per-rule outstanding
/// exception allowance behind D-159) = 20.
pub(super) const PAUSED_STATUS_UP: &str = "CREATE TABLE standing_rules_new (
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
    status TEXT NOT NULL CHECK(status IN ('active', 'paused', 'needs_review', 'revoked')),
    activated_at INTEGER NOT NULL,
    last_used_at INTEGER,
    revoked_at INTEGER,
    needs_review_since INTEGER,
    reviewed_scope_digest TEXT,
    compatibility_digest TEXT,
    dark_window_max_pending INTEGER
);
INSERT INTO standing_rules_new (
    rule_id, artifact_id, version, action_id, rule_json,
    quota_max, quota_window_secs, rate_max, rate_window_secs,
    expires_after_secs, dark_window_timeout_secs, dark_window_default,
    status, activated_at, last_used_at, revoked_at, needs_review_since,
    reviewed_scope_digest, compatibility_digest, dark_window_max_pending
)
SELECT rule_id, artifact_id, version, action_id, rule_json,
    quota_max, quota_window_secs, rate_max, rate_window_secs,
    expires_after_secs, dark_window_timeout_secs, dark_window_default,
    status, activated_at, last_used_at, revoked_at, needs_review_since,
    reviewed_scope_digest, compatibility_digest, dark_window_max_pending
FROM standing_rules;
DROP TABLE standing_rules;
ALTER TABLE standing_rules_new RENAME TO standing_rules;
CREATE INDEX IF NOT EXISTS idx_standing_rules_action ON standing_rules (action_id, status);";

/// The documented downgrade drops the `CHECK` while preserving every row.
#[cfg(test)]
pub(super) const PAUSED_STATUS_DOWN: &str = "CREATE TABLE standing_rules_old (
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
    needs_review_since INTEGER,
    reviewed_scope_digest TEXT,
    compatibility_digest TEXT,
    dark_window_max_pending INTEGER
);
INSERT INTO standing_rules_old (
    rule_id, artifact_id, version, action_id, rule_json,
    quota_max, quota_window_secs, rate_max, rate_window_secs,
    expires_after_secs, dark_window_timeout_secs, dark_window_default,
    status, activated_at, last_used_at, revoked_at, needs_review_since,
    reviewed_scope_digest, compatibility_digest, dark_window_max_pending
)
SELECT rule_id, artifact_id, version, action_id, rule_json,
    quota_max, quota_window_secs, rate_max, rate_window_secs,
    expires_after_secs, dark_window_timeout_secs, dark_window_default,
    status, activated_at, last_used_at, revoked_at, needs_review_since,
    reviewed_scope_digest, compatibility_digest, dark_window_max_pending
FROM standing_rules;
DROP TABLE standing_rules;
ALTER TABLE standing_rules_old RENAME TO standing_rules;
CREATE INDEX IF NOT EXISTS idx_standing_rules_action ON standing_rules (action_id, status);";

/// Entry v9 retires the Telegram-shaped `bound_chat_id INTEGER` column from
/// the two tables that persisted a grant's owner binding, leaving the
/// channel-neutral `owner_surface_json` the ad-hoc lane already added.
///
/// Both statements name only columns that exist in *both* the pre-v9 and
/// post-v9 shapes: a legacy file has `bound_chat_id` plus the additively added
/// `owner_surface_json`, and a fresh file created from the current
/// `SCHEMA_SQL` / `standing_rules_schema::ensure_schema` has only
/// `owner_surface_json`. The rebuild therefore runs unchanged on either,
/// dropping the legacy column by omission. Legacy rows keep
/// `owner_surface_json NULL`, which every reader treats as an unbound grant
/// and refuses (fail closed) — a chat id alone cannot be promoted into a
/// principal-bound surface reference.
///
/// `standing_rule_pending_actions` column list = 16 baseline (with
/// `bound_chat_id` replaced by `owner_surface_json`) + `reviewed_scope_digest`
/// / `compatibility_digest` (#135's D-160 consume-time context revalidation)
/// = 18.
pub(super) const OWNER_SURFACE_UP: &str = "CREATE TABLE task_grants_new (
    id TEXT PRIMARY KEY,
    task_token TEXT NOT NULL UNIQUE,
    expires_at TEXT NOT NULL,
    grant_json TEXT NOT NULL,
    pending_message_digest TEXT NOT NULL,
    owner_surface_json TEXT
);
INSERT INTO task_grants_new (
    id, task_token, expires_at, grant_json, pending_message_digest,
    owner_surface_json
)
SELECT id, task_token, expires_at, grant_json, pending_message_digest,
    owner_surface_json
FROM task_grants;
DROP TABLE task_grants;
ALTER TABLE task_grants_new RENAME TO task_grants;
CREATE TABLE standing_rule_pending_actions_new (
    pending_id TEXT PRIMARY KEY,
    rule_id TEXT NOT NULL,
    rule_version INTEGER NOT NULL,
    task_grant_id TEXT NOT NULL,
    action_id TEXT NOT NULL,
    owner_surface_json TEXT,
    payload_ref_json TEXT,
    dark_window_default TEXT NOT NULL CHECK(dark_window_default IN ('allow', 'deny')),
    request_fingerprint TEXT NOT NULL,
    requested_at INTEGER NOT NULL,
    resolved_at INTEGER,
    resolution TEXT CHECK(resolution IN ('allowed', 'denied', 'stale')),
    dispatch_state TEXT NOT NULL DEFAULT 'none'
        CHECK(dispatch_state IN ('none', 'claimed', 'dispatched')),
    token_consumed_at INTEGER,
    dispatch_receipt_digest TEXT,
    owner_attention_since INTEGER,
    reviewed_scope_digest TEXT,
    compatibility_digest TEXT,
    FOREIGN KEY(rule_id) REFERENCES standing_rules(rule_id) ON DELETE CASCADE
);
INSERT INTO standing_rule_pending_actions_new (
    pending_id, rule_id, rule_version, task_grant_id, action_id,
    owner_surface_json, payload_ref_json, dark_window_default,
    request_fingerprint, requested_at, resolved_at, resolution,
    dispatch_state, token_consumed_at, dispatch_receipt_digest,
    owner_attention_since, reviewed_scope_digest, compatibility_digest
)
SELECT pending_id, rule_id, rule_version, task_grant_id, action_id,
    owner_surface_json, payload_ref_json, dark_window_default,
    request_fingerprint, requested_at, resolved_at, resolution,
    dispatch_state, token_consumed_at, dispatch_receipt_digest,
    owner_attention_since, reviewed_scope_digest, compatibility_digest
FROM standing_rule_pending_actions;
DROP TABLE standing_rule_pending_actions;
ALTER TABLE standing_rule_pending_actions_new
    RENAME TO standing_rule_pending_actions;
CREATE UNIQUE INDEX IF NOT EXISTS idx_standing_rule_pending_fingerprint
    ON standing_rule_pending_actions (rule_id, rule_version, request_fingerprint);";

/// The documented downgrade restores the pre-v9 shape: `bound_chat_id` comes
/// back (defaulted, because a surface reference carries no chat id outside the
/// Telegram adapter) while `owner_surface_json` stays, which is exactly the
/// shape the ad-hoc lane converges a legacy file to.
#[cfg(test)]
pub(super) const OWNER_SURFACE_DOWN: &str =
    "ALTER TABLE task_grants ADD COLUMN bound_chat_id INTEGER NOT NULL DEFAULT 0;
ALTER TABLE standing_rule_pending_actions
    ADD COLUMN bound_chat_id INTEGER NOT NULL DEFAULT 0;";

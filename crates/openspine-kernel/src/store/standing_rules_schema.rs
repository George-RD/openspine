//! The `standing_rules`, `standing_rule_usage`, `standing_rule_pending_actions`
//! and `standing_rule_timer_links` table definitions. Split from
//! `standing_rules.rs` to keep both files under the 500-line gate; additive
//! column evolution lives in `migrations.rs`.

use super::StoreError;

pub fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS standing_rules (
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
            needs_review_since INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_standing_rules_action
            ON standing_rules (action_id, status);
        CREATE TABLE IF NOT EXISTS standing_rule_usage (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            rule_id TEXT NOT NULL,
            version INTEGER NOT NULL,
            kind TEXT NOT NULL CHECK(kind IN ('quota', 'rate')),
            used_at INTEGER NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('reserved', 'committed', 'waiver')),
            reservation_id TEXT NOT NULL,
            FOREIGN KEY(rule_id) REFERENCES standing_rules(rule_id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_standing_rule_usage_window
            ON standing_rule_usage (rule_id, kind, status, used_at);
        CREATE TABLE IF NOT EXISTS standing_rule_pending_actions (
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
            FOREIGN KEY(rule_id) REFERENCES standing_rules(rule_id) ON DELETE CASCADE
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_standing_rule_pending_fingerprint
            ON standing_rule_pending_actions (rule_id, rule_version, request_fingerprint);
        CREATE TABLE IF NOT EXISTS standing_rule_timer_links (
            timer_id TEXT PRIMARY KEY,
            pending_id TEXT NOT NULL,
            applied_at INTEGER,
            FOREIGN KEY(pending_id) REFERENCES standing_rule_pending_actions(pending_id) ON DELETE CASCADE
        );",
    )?;
    Ok(())
}

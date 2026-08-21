//! Owner-addressable resolution of a pending dark-window exception, and the
//! fingerprint→pending lookup the owner notification binds its buttons to.
//! Split from `standing_rules_pending.rs` to keep both files under the
//! 500-line gate.

use jiff::Timestamp;
use rusqlite::{params, OptionalExtension};

use super::standing_rules::timestamp_to_epoch_nanos;
use super::{Store, StoreError};

impl Store {
    /// Resolve the pending identity for a stable request fingerprint so the
    /// owner notification can bind Allow/Deny buttons to the exact row.
    pub fn pending_id_for_fingerprint(
        &self,
        rule_id: &str,
        rule_version: u32,
        fingerprint: &str,
    ) -> Result<Option<String>, StoreError> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row(
                "SELECT pending_id FROM standing_rule_pending_actions
                 WHERE rule_id = ?1 AND rule_version = ?2 AND request_fingerprint = ?3",
                params![rule_id, rule_version as i64, fingerprint],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Owner-addressable resolution of a pending dark-window action (P1-9):
    /// the owner may allow or deny the specific pending action before the
    /// timer fires. First write wins and is idempotent; a late tap after the
    /// timer already fired is a harmless no-op (the fired default path has
    /// already decided `allowed`). Cancelling here means the fired timer will
    /// find `resolution = 'denied'`/`'stale'` and apply no authority.
    pub fn resolve_pending_action(
        &self,
        pending_id: &str,
        allow: bool,
        now: Timestamp,
    ) -> Result<bool, StoreError> {
        let resolution = if allow { "allowed" } else { "denied" };
        let now_nanos = timestamp_to_epoch_nanos(now)?;
        self.with_immediate_tx(|tx| {
            // Only set if still unresolved (first write wins).
            let changed = tx.execute(
                "UPDATE standing_rule_pending_actions \
                 SET resolved_at = ?2, resolution = ?3 \
                 WHERE pending_id = ?1 AND resolved_at IS NULL",
                params![pending_id, now_nanos, resolution],
            )?;
            if changed >= 1 {
                Self::append_audit_conn(
                    tx,
                    "standing_rule.pending_resolved",
                    None,
                    None,
                    Some(&format!(
                        "pending {pending_id} resolved by owner: {resolution}"
                    )),
                    None,
                    &[],
                    &[],
                )?;
            }
            Ok(changed >= 1)
        })
    }
}

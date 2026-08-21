//! Pause, resume, and revoke transitions for standing rules.

use jiff::Timestamp;
use rusqlite::{params, OptionalExtension};

use super::{standing_rules::timestamp_to_epoch_nanos, Store, StoreError};

impl Store {
    /// Revoke a standing rule, making it invisible to gate consultation.
    /// Repeating the transition or naming an unknown rule returns `Ok(false)`.
    pub fn revoke_standing_rule(&self, rule_id: &str, now: Timestamp) -> Result<bool, StoreError> {
        let now_nanos = timestamp_to_epoch_nanos(now)?;
        self.with_immediate_tx(|tx| {
            let changed = tx.execute(
                "UPDATE standing_rules SET status = 'revoked', revoked_at = ?2 \
                 WHERE rule_id = ?1 AND status != 'revoked'",
                params![rule_id, now_nanos],
            )?;
            if changed == 1 {
                // #135: staling the open exceptions in the revoke's own
                // transaction guarantees nothing a revoked rule scheduled can
                // still fire (claim treats `stale` as terminal). Every version is
                // swept, not just the current one: revoke retires the rule id, and
                // an exception minted under an older version is just as fireable.
                super::standing_rules_exceptions::stale_pending_exceptions_in_tx(
                    tx, rule_id, None, now_nanos,
                )?;
                Self::append_audit_conn(
                    tx,
                    "standing_rule.revoked",
                    None,
                    None,
                    Some(rule_id),
                    None,
                    &[],
                    &[],
                )?;
            }
            Ok(changed == 1)
        })
    }

    /// Pause an active rule, immediately excluding it from gate consultation.
    ///
    /// A pause restores ordinary owner approval *immediately*, so an exception
    /// minted under the rule must not still fire while the owner has withdrawn
    /// it. Open exceptions are staled in the same transaction, exactly as
    /// revoke does (#135); resume re-earns authority through revalidation
    /// rather than by inheriting a pending default the owner paused.
    pub(crate) fn pause_standing_rule(
        &self,
        rule_id: &str,
        now: Timestamp,
    ) -> Result<super::PauseStandingRuleOutcome, StoreError> {
        let now_nanos = timestamp_to_epoch_nanos(now)?;
        self.with_immediate_tx(|tx| {
            let changed = tx.execute(
                "UPDATE standing_rules SET status = 'paused' \
                 WHERE rule_id = ?1 AND status = 'active'",
                params![rule_id],
            )?;
            if changed == 1 {
                super::standing_rules_exceptions::stale_pending_exceptions_in_tx(
                    tx, rule_id, None, now_nanos,
                )?;
                Self::append_audit_conn(
                    tx,
                    "standing_rule.paused",
                    None,
                    None,
                    Some(rule_id),
                    None,
                    &[],
                    &[],
                )?;
                return Ok(super::PauseStandingRuleOutcome::Paused);
            }

            let status: Option<String> = tx
                .query_row(
                    "SELECT status FROM standing_rules WHERE rule_id = ?1 \
                     ORDER BY version DESC LIMIT 1",
                    params![rule_id],
                    |row| row.get(0),
                )
                .optional()?;
            let outcome = if status.as_deref() == Some("paused") {
                super::PauseStandingRuleOutcome::AlreadyPaused
            } else {
                Self::append_audit_conn(
                    tx,
                    "standing_rule.pause_refused",
                    None,
                    None,
                    Some(status.as_deref().unwrap_or("missing")),
                    None,
                    &[],
                    &[],
                )?;
                super::PauseStandingRuleOutcome::Refused
            };
            Ok(outcome)
        })
    }

    /// Resume an exactly versioned paused rule after caller-side revalidation.
    pub fn resume_standing_rule(&self, rule_id: &str, version: u32) -> Result<bool, StoreError> {
        self.with_immediate_tx(|tx| {
            let changed = tx.execute(
                "UPDATE standing_rules SET status = 'active' \
                 WHERE rule_id = ?1 AND version = ?2 AND status = 'paused'",
                params![rule_id, version as i64],
            )?;
            if changed == 1 {
                Self::append_audit_conn(
                    tx,
                    "standing_rule.resumed",
                    None,
                    None,
                    Some(rule_id),
                    None,
                    &[],
                    &[],
                )?;
            }
            Ok(changed == 1)
        })
    }

    /// Highest stored version for `rule_id`, regardless of lifecycle status.
    pub fn standing_rule_latest_version(&self, rule_id: &str) -> Result<Option<u32>, StoreError> {
        let conn = self.conn.lock();
        let version: Option<i64> = conn
            .query_row(
                "SELECT MAX(version) FROM standing_rules WHERE rule_id = ?1",
                params![rule_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        Ok(version.map(|value| value as u32))
    }
}

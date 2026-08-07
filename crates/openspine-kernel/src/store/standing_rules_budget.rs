//! Standing-rule budget reservation, finalize, cancel, drift detection, and
//! remaining-headroom queries (AD-106). Split from `standing_rules.rs` to
//! keep both files under the 500-line file-size gate (mirrors the D-050
//! `budget_support` / `budget_support_tests` split).
//!
//! Quota (volume, e.g. 5/week) and rate (velocity, e.g. 1/hour) are
//! independent sliding-window counters. A successful gate consultation
//! *reserves* one unit of each (status `reserved`) inside one `BEGIN
//! IMMEDIATE` transaction so a saturated window can never be overspent under
//! concurrent dispatches (D-050 TOCTOU precedent). The reservation is only
//! *finalized* (status `reserved` → `committed`) once the effect actually
//! dispatches, and atomically *cancelled* (rows deleted) if dispatch fails —
//! so failed/denied effects never consume the budget (AD-106 failed-effects
//! rule, P1-6). `finalize` re-checks the rule is still the consulted version,
//! so a v2 activation that swapped the action between consult and finalize
//! cannot charge the in-flight action to the wrong rule (P1-4).

use super::standing_rules::{
    rule_from_row, rule_row_from_row, timestamp_to_epoch_nanos, RuleRow, StandingRule,
    RULE_ROW_COLUMNS,
};
use super::{Store, StoreError};
use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use rusqlite::params;
use rusqlite::OptionalExtension;
use rusqlite::TransactionBehavior;
use ulid::Ulid;

const NANOS_PER_SEC: i64 = 1_000_000_000;

impl Store {
    /// Atomically reserve one unit of a standing rule's `quota` and `rate`
    /// windows inside one transaction. Returns `Some(reservation_id)` when
    /// both windows have headroom (the caller may proceed and finalize on
    /// success), or `None` when the rule is drifted/revoked/expired/unknown
    /// or either window is saturated (the caller falls back to normal
    /// approval). Headroom counts both `reserved` and `committed` rows so a
    /// concurrent in-flight reservation is never overspent.
    pub fn reserve_standing_rule_budget(
        &self,
        rule_id: &str,
        used_at: Timestamp,
    ) -> Result<Option<String>, StoreError> {
        let now_nanos = timestamp_to_epoch_nanos(used_at)?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row = tx
            .query_row(
                "SELECT quota_max, quota_window_secs, rate_max, rate_window_secs, version \
                 FROM standing_rules WHERE rule_id = ?1 AND status = 'active'",
                params![rule_id],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((quota_max, quota_win, rate_max, rate_win, version)) = row else {
            return Ok(None);
        };
        let quota_count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM standing_rule_usage \
             WHERE rule_id = ?1 AND kind = 'quota' \
               AND status IN ('reserved', 'committed') AND used_at >= ?2",
            params![rule_id, now_nanos - quota_win * NANOS_PER_SEC],
            |r| r.get(0),
        )?;
        let rate_count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM standing_rule_usage \
             WHERE rule_id = ?1 AND kind = 'rate' \
               AND status IN ('reserved', 'committed') AND used_at >= ?2",
            params![rule_id, now_nanos - rate_win * NANOS_PER_SEC],
            |r| r.get(0),
        )?;
        if quota_count >= quota_max || rate_count >= rate_max {
            // Saturated: leave the transaction (rolled back) and report no
            // reservation so the caller falls back to dark-window/approval.
            return Ok(None);
        }
        let reservation_id = ulid::Ulid::new().to_string();
        tx.execute(
            "INSERT INTO standing_rule_usage (rule_id, version, kind, used_at, status, reservation_id)
             VALUES (?1, ?2, 'quota', ?3, 'reserved', ?4),
                    (?1, ?2, 'rate', ?3, 'reserved', ?4)",
            params![rule_id, version, now_nanos, reservation_id],
        )?;
        tx.commit()?;
        Ok(Some(reservation_id))
    }

    /// Finalize a reserved budget as committed after a successful dispatch.
    /// Re-checks the rule is still the consulted version; if a higher version
    /// changed the action between consult and finalize, the reservation is
    /// cancelled (not charged) and `false` is returned, so the in-flight
    /// action cannot consume the wrong rule's budget (P1-4). On commit, runs
    /// the AD-010 drift check against committed usage and, if three distinct
    /// calibrated rate windows were each saturated, moves the rule to
    /// `needs_review`.
    pub fn finalize_standing_rule_reservation(
        &self,
        rule_id: &str,
        version: u32,
        reservation_id: &str,
        used_at: Timestamp,
    ) -> Result<bool, StoreError> {
        self.finalize_reservation_inner(rule_id, version, reservation_id, used_at, true)
    }

    /// Finalize a reservation minted by a fired dark-window exception (#135).
    /// Identical to [`Self::finalize_standing_rule_reservation`] except that
    /// it does not refresh the lapse-after-unused clock: owner silence is not
    /// evidence that the owner is still exercising the responsibility (D-161).
    pub fn finalize_standing_rule_exception_reservation(
        &self,
        rule_id: &str,
        version: u32,
        reservation_id: &str,
        used_at: Timestamp,
    ) -> Result<bool, StoreError> {
        self.finalize_reservation_inner(rule_id, version, reservation_id, used_at, false)
    }

    fn finalize_reservation_inner(
        &self,
        rule_id: &str,
        version: u32,
        reservation_id: &str,
        used_at: Timestamp,
        refresh_lapse_clock: bool,
    ) -> Result<bool, StoreError> {
        let now_nanos = timestamp_to_epoch_nanos(used_at)?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: bool = tx
            .query_row(
                "SELECT 1 FROM standing_rules WHERE rule_id = ?1 AND version = ?2 AND status = 'active'",
                params![rule_id, version as i64],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .is_some();
        let reserved_shape: (i64, i64) = tx.query_row(
            "SELECT COUNT(*), COUNT(DISTINCT kind) FROM standing_rule_usage \
             WHERE reservation_id = ?1 AND rule_id = ?2 AND version = ?3 AND status = 'reserved'",
            params![reservation_id, rule_id, version as i64],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if reserved_shape != (2, 2) {
            tx.commit()?;
            return Ok(false);
        }
        if !current {
            tx.execute(
                "DELETE FROM standing_rule_usage WHERE reservation_id = ?1 AND status = 'reserved'",
                params![reservation_id],
            )?;
            tx.commit()?;
            return Ok(false);
        }
        let changed = tx.execute(
            "UPDATE standing_rule_usage SET status = 'committed' \
             WHERE reservation_id = ?1 AND rule_id = ?2 AND version = ?3 AND status = 'reserved'",
            params![reservation_id, rule_id, version as i64],
        )?;
        if changed != 2 {
            return Ok(false);
        }
        Self::note_standing_rule_use_in_tx(&tx, rule_id, now_nanos, refresh_lapse_clock)?;
        tx.commit()?;
        Ok(true)
    }

    /// Record that a rule was used for an effect that may have taken hold, and
    /// re-evaluate the AD-010 drift trigger.
    ///
    /// Called on the finalize path and on the *retain* path. A retained
    /// `DeliveryUnknown` reservation stays `reserved` rather than becoming
    /// `committed` (D-157), so without this entry point a rule that saturates
    /// only through ambiguous outcomes would refresh no expiry and never be
    /// flagged for owner re-review — fail-open on the review trigger, which is
    /// exactly the signal the owner most needs when writes keep going
    /// unconfirmed.
    pub fn note_standing_rule_use(
        &self,
        rule_id: &str,
        used_at: Timestamp,
    ) -> Result<(), StoreError> {
        self.note_standing_rule_use_with_lapse(rule_id, used_at, true)
    }

    /// As [`Self::note_standing_rule_use`], but `refresh_lapse_clock = false`
    /// for a fired dark-window exception (D-161).
    pub fn note_standing_rule_use_with_lapse(
        &self,
        rule_id: &str,
        used_at: Timestamp,
        refresh_lapse_clock: bool,
    ) -> Result<(), StoreError> {
        let now_nanos = timestamp_to_epoch_nanos(used_at)?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::note_standing_rule_use_in_tx(&tx, rule_id, now_nanos, refresh_lapse_clock)?;
        tx.commit()?;
        Ok(())
    }

    /// Refresh the lapse-expiry clock and apply the AD-010 drift trigger: when
    /// three distinct calibrated rate windows were each saturated, the rule
    /// moves to `needs_review`.
    ///
    /// Saturation counts `reserved` and `committed` usage, matching every
    /// other budget query in this module (`reserve_standing_rule_budget`,
    /// `standing_rule_remaining`, both consult paths). A reserved unit is
    /// spent budget for enforcement, so it is spent budget for the review
    /// trigger too — counting only `committed` here would let a rule saturate
    /// indefinitely through retained ambiguous outcomes without ever being
    /// surfaced to the owner.
    fn note_standing_rule_use_in_tx(
        tx: &rusqlite::Transaction<'_>,
        rule_id: &str,
        now_nanos: i64,
        refresh_lapse_clock: bool,
    ) -> Result<(), StoreError> {
        // #135/D-161: a fired dark-window exception does NOT refresh the
        // lapse-after-unused clock. That clock exists so a responsibility the
        // owner has stopped exercising retires itself; owner silence must not
        // keep alive exactly the rule it should retire. The drift trigger
        // below still runs, because a rule saturating through silence is
        // precisely one the owner should re-review.
        if refresh_lapse_clock {
            tx.execute(
                "UPDATE standing_rules SET last_used_at = ?2 WHERE rule_id = ?1",
                params![rule_id, now_nanos],
            )?;
        }
        let drift: Option<(i64, i64, i64)> = tx
            .query_row(
                "SELECT rate_window_secs, rate_max, activated_at \
                 FROM standing_rules WHERE rule_id = ?1",
                params![rule_id],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((rate_win, rate_max, activated_at)) = drift else {
            return Ok(());
        };
        let recent_rate_windows: i64 = tx.query_row(
            "SELECT COUNT(*) FROM (
                    SELECT (used_at - ?3) / (?2 * 1000000000) AS widx
                    FROM standing_rule_usage
                    WHERE rule_id = ?1 AND kind = 'rate' \
                      AND status IN ('reserved', 'committed')
                    GROUP BY widx
                    HAVING COUNT(*) >= ?4
                )",
            params![rule_id, rate_win, activated_at, rate_max],
            |r| r.get(0),
        )?;
        if recent_rate_windows >= 3 {
            // #135: a rule pushed to needs_review by drift must leave no
            // fireable exception behind, on every version it holds.
            super::standing_rules_exceptions::stale_pending_exceptions_in_tx(
                tx, rule_id, None, now_nanos,
            )?;
            tx.execute(
                "UPDATE standing_rules SET status = 'needs_review', needs_review_since = ?2 \
                 WHERE rule_id = ?1 AND status = 'active'",
                params![rule_id, now_nanos],
            )?;
            Self::append_audit_conn(
                tx,
                "standing_rule.drift_detected",
                None,
                None,
                Some("rate window repeatedly saturated; re-confirmation required"),
                None,
                &[],
                &[],
            )?;
        }
        Ok(())
    }

    /// Cancel a reserved budget (effect failed or denied, or rule superseded):
    /// delete the `reserved` rows so no budget is consumed. Committed rows are
    /// never touched (P1-6 / AD-106 failed-effects rule).
    pub fn cancel_standing_rule_reservation(&self, reservation_id: &str) -> Result<(), StoreError> {
        // Test-only one-shot failure: when armed, the next reservation
        // cancel fails so a regression can prove the caller re-arms a fired
        // one-use token only after a *successful* cancel — a failed cancel
        // must leave the pending row `claimed` and the budget reserved.
        if self
            .fail_next_reservation_cancel
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
        }
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM standing_rule_usage WHERE reservation_id = ?1 AND status = 'reserved'",
            params![reservation_id],
        )?;
        Ok(())
    }

    /// Remaining headroom for a rule at `now` — returned in the gate response
    /// (AD-013 calibration signal + AD-106 "agents self-adjust without extra
    /// round-trips"). Counts both reserved and committed usage as consumed.
    pub fn standing_rule_remaining(
        &self,
        rule_id: &str,
        now: Timestamp,
    ) -> Result<(u32, u32), StoreError> {
        if self
            .fail_next_standing_rule_remaining
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            // Test-only one-shot failure (SrFinalSec retraction regression):
            // the post-reservation headroom read must fail loudly so the
            // caller cancels the reserved budget instead of leaking it.
            return Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
        }
        let conn = self.conn.lock();
        let (quota_max, quota_win, rate_max, rate_win): (i64, i64, i64, i64) = conn.query_row(
            "SELECT quota_max, quota_window_secs, rate_max, rate_window_secs \
             FROM standing_rules WHERE rule_id = ?1",
            params![rule_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        let now_nanos = timestamp_to_epoch_nanos(now)?;
        let quota_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM standing_rule_usage \
             WHERE rule_id = ?1 AND kind = 'quota' \
               AND status IN ('reserved', 'committed') AND used_at >= ?2",
            params![rule_id, now_nanos - quota_win * NANOS_PER_SEC],
            |row| row.get(0),
        )?;
        let rate_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM standing_rule_usage \
             WHERE rule_id = ?1 AND kind = 'rate' \
               AND status IN ('reserved', 'committed') AND used_at >= ?2",
            params![rule_id, now_nanos - rate_win * NANOS_PER_SEC],
            |row| row.get(0),
        )?;
        Ok((
            quota_max.saturating_sub(quota_count).max(0) as u32,
            rate_max.saturating_sub(rate_count).max(0) as u32,
        ))
    }

    /// Atomically consult and reserve in one `BEGIN IMMEDIATE`: look up the
    /// active, non-expired, non-revoked rule for `action`, bind it to the
    /// current request, and reserve quota+rate *inside the same transaction*
    /// the lookup ran in — so a saturated window can never be overspent (D-050)
    /// and a v2 action-swap between lookup and reserve (P1-4 TOCTOU) cannot
    /// admit request A under B's budget. Returns `Some((rule, reservation_id))`
    /// when a rule matched *and* both windows had headroom (the caller may
    /// proceed and finalize on success), or `None` when no rule matched or
    /// budget was exhausted (the caller falls back to normal approval). The
    /// dark-window timer is NOT scheduled here (that needs a request
    /// fingerprint + grant context) — the gate schedules it separately only
    /// when `allow == false` and a context is present.
    pub fn consult_and_reserve_standing_rule(
        &self,
        action: &ActionId,
        now: Timestamp,
    ) -> Result<Option<(StandingRule, Option<String>)>, StoreError> {
        let now_nanos = timestamp_to_epoch_nanos(now)?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Same column projection as `active_standing_rule_for_action`, but on
        // the reservation transaction so no rule/version swap can occur
        // between lookup and reserve (P1-4). Expiry uses last_used_at
        // (falling back to activation), matching the canonical matcher.
        let row: Option<RuleRow> = tx
            .query_row(
                &format!(
                    "SELECT {RULE_ROW_COLUMNS} FROM standing_rules \
                     WHERE action_id = ?1 AND status = 'active' \
                     ORDER BY version DESC LIMIT 1"
                ),
                params![action.to_string()],
                rule_row_from_row,
            )
            .optional()?;
        let Some(row) = row else {
            return Ok(None);
        };
        let rule = rule_from_row(row, "active")?;
        let reference = timestamp_to_epoch_nanos(rule.last_used_at.unwrap_or(rule.activated_at))?;
        let deadline = reference + rule.expires_after_secs * 1_000_000_000;
        // Canonical exact-deadline boundary: a rule lapses the instant `now`
        // reaches `deadline` (`deadline <= now`), matching the strict
        // fired-token SQL (`elapsed < expiry`) and the live lookup path
        // (D-073). One canonical boundary for both admission paths.
        if deadline <= now_nanos {
            // Expired: mark needs_review (canonical matcher behavior) and fall
            // back to normal approval. #135: stale its open exceptions in the
            // same transaction so none can still fire.
            super::standing_rules_exceptions::stale_pending_exceptions_in_tx(
                &tx,
                &rule.rule_id,
                Some(rule.version),
                now_nanos,
            )?;
            tx.execute(
                "UPDATE standing_rules SET status = 'needs_review', needs_review_since = ?2 \
                 WHERE rule_id = ?1 AND status = 'active'",
                params![rule.rule_id, now_nanos],
            )?;
            tx.commit()?;
            return Ok(None);
        }
        // Reserve headroom using reserved+committed so a concurrent in-flight
        // reservation is never overspent (D-050).
        let quota_used: i64 = tx.query_row(
            "SELECT COUNT(*) FROM standing_rule_usage \
             WHERE rule_id = ?1 AND kind = 'quota' \
               AND status IN ('reserved', 'committed') AND used_at >= ?2",
            params![
                rule.rule_id,
                now_nanos - rule.quota.window_secs * NANOS_PER_SEC
            ],
            |row| row.get(0),
        )?;
        let rate_used: i64 = tx.query_row(
            "SELECT COUNT(*) FROM standing_rule_usage \
             WHERE rule_id = ?1 AND kind = 'rate' \
               AND status IN ('reserved', 'committed') AND used_at >= ?2",
            params![
                rule.rule_id,
                now_nanos - rule.rate.window_secs * NANOS_PER_SEC
            ],
            |row| row.get(0),
        )?;
        let reservation_id = if quota_used < rule.quota.max as i64
            && rate_used < rule.rate.max as i64
        {
            let reservation_id = Ulid::new().to_string();
            tx.execute(
                "INSERT INTO standing_rule_usage (rule_id, version, kind, used_at, status, reservation_id)
                 VALUES (?1, ?2, 'quota', ?3, 'reserved', ?4),
                        (?1, ?2, 'rate', ?3, 'reserved', ?4)",
                params![rule.rule_id, rule.version as i64, now_nanos, reservation_id],
            )?;
            Some(reservation_id)
        } else {
            None
        };
        tx.commit()?;
        Ok(Some((rule, reservation_id)))
    }
}

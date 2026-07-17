//! Durable global per-day spend accounting (AD-143).
//!
//! The ledger is intentionally separate from grant counters: task budgets remain
//! scoped to one grant, while this table is the kernel-wide admission boundary
//! that counts model and connector usage across all lanes.
//!
//! Two atomic primitives back the kill-switch:
//! - [`Store::reserve_daily_model_call`] / [`Store::reserve_daily_connector_call`]
//!   — an atomic increment-and-check used at every usage boundary. On denial
//!   (the configured cap is already reached) they atomically record the one-time
//!   daily breach and report `first_breach`, so the caller can emit the required
//!   immediate owner notification exactly once even when the cap is crossed
//!   between admission and the actual call.
//! - [`Store::check_and_mark_daily_breach`] — the admission-gate variant used at
//!   the lane boundary (read + one-time breach mark under one lock).
//!
//! Counting is fail-closed: a ledger write failure must deny the call, never let
//! spend escape durable accounting.

use jiff::Timestamp;
use rusqlite::{params, OptionalExtension};

use super::{Store, StoreError};

/// SQLite's signed integer limit is the largest safe cap representation.
fn cap_i64(cap: u64) -> Result<i64, StoreError> {
    i64::try_from(cap).map_err(|_| StoreError::NumericRange)
}

/// Canonical UTC day key used by the daily ledger.
pub(crate) fn utc_day(now: Timestamp) -> String {
    now.to_string()
        .split('T')
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Record the one-time daily breach under a held connection lock; returns
/// `true` only if this call set `breached_at` (the first breach of the day).
fn mark_breach(conn: &rusqlite::Transaction<'_>, day: &str) -> Result<bool, StoreError> {
    conn.execute(
        "INSERT OR IGNORE INTO daily_spend (day, model_calls, connector_calls)
         VALUES (?1, 0, 0)",
        params![day],
    )?;
    conn.execute(
        "UPDATE daily_spend
         SET breached_at = ?2, alert_state = 1
         WHERE day = ?1 AND breached_at IS NULL",
        params![day, Timestamp::now().to_string()],
    )?;
    Ok(conn.changes() == 1)
}

impl Store {
    /// Atomically reserve one global model-call unit for `day`, incrementing
    /// only while the resulting total stays below `cap`. Returns
    /// `(allowed, first_breach)`: `allowed` is whether the call may proceed, and
    /// `first_breach` reports whether this denial is the first breach of the day
    /// (so the caller emits the owner notification exactly once). A `cap` of `0`
    /// always denies and marks the breach. The increment-or-breach is the single
    /// decision point, so concurrent callers can never both pass the last slot.
    pub fn reserve_daily_model_call(
        &self,
        day: &str,
        cap: u64,
    ) -> Result<(bool, bool), StoreError> {
        let cap = cap_i64(cap)?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let result = if cap <= 0 {
            (false, mark_breach(&tx, day)?)
        } else {
            tx.execute(
                "INSERT INTO daily_spend (day, model_calls, connector_calls)
                 VALUES (?1, 1, 0)
                 ON CONFLICT(day) DO UPDATE SET model_calls = model_calls + 1
                 WHERE model_calls < ?2",
                params![day, cap],
            )?;
            if tx.changes() == 1 {
                (true, false)
            } else {
                (false, mark_breach(&tx, day)?)
            }
        };
        tx.commit()?;
        Ok(result)
    }
    pub fn reserve_daily_connector_call(
        &self,
        day: &str,
        cap: u64,
    ) -> Result<(bool, bool), StoreError> {
        let cap = cap_i64(cap)?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let result = if cap <= 0 {
            (false, mark_breach(&tx, day)?)
        } else {
            tx.execute(
                "INSERT INTO daily_spend (day, model_calls, connector_calls)
                 VALUES (?1, 0, 1)
                 ON CONFLICT(day) DO UPDATE SET connector_calls = connector_calls + 1
                 WHERE connector_calls < ?2",
                params![day, cap],
            )?;
            if tx.changes() == 1 {
                (true, false)
            } else {
                (false, mark_breach(&tx, day)?)
            }
        };
        tx.commit()?;
        Ok(result)
    }

    pub fn check_and_mark_daily_breach(
        &self,
        day: &str,
        model_cap: u64,
        connector_cap: u64,
    ) -> Result<(bool, bool), StoreError> {
        let model_cap = cap_i64(model_cap)?;
        let connector_cap = cap_i64(connector_cap)?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let counts: Option<(i64, i64)> = tx
            .query_row(
                "SELECT model_calls, connector_calls FROM daily_spend WHERE day = ?1",
                params![day],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (model, connector) = counts.unwrap_or((0, 0));
        let result = if model >= model_cap || connector >= connector_cap {
            (false, mark_breach(&tx, day)?)
        } else {
            (true, false)
        };
        tx.commit()?;
        Ok(result)
    }

    /// Transition alert_state from pending (1) to in_flight (2). Returns true
    /// if this caller successfully claimed it (changes() == 1).
    pub fn claim_daily_breach_alert(&self, day: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE daily_spend SET alert_state = 2 WHERE day = ?1 AND alert_state = 1",
            params![day],
        )?;
        Ok(conn.changes() == 1)
    }

    /// Transition alert_state from in_flight (2) to cleared (0) upon successful send.
    pub fn complete_daily_breach_alert(&self, day: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE daily_spend SET alert_state = 0 WHERE day = ?1 AND alert_state = 2",
            params![day],
        )?;
        Ok(())
    }

    /// Re-arm a claimed alert when notification was not durably delivered.
    pub fn rearm_daily_breach_alert(&self, day: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE daily_spend SET alert_state = 1 WHERE day = ?1 AND alert_state = 2",
            params![day],
        )?;
        Ok(())
    }

    /// Reset all in_flight (2) alerts back to pending (1) for crash recovery.
    pub fn reset_inflight_breach_alerts(&self) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE daily_spend SET alert_state = 1 WHERE alert_state = 2",
            [],
        )?;
        Ok(())
    }

    /// Return every day whose breach notification is pending.
    pub fn pending_daily_breach_alert_days(&self) -> Result<Vec<String>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT day FROM daily_spend WHERE alert_state = 1 ORDER BY day")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    #[cfg(test)]
    pub(crate) fn daily_spend_counts(&self, day: &str) -> Result<(u64, u64), StoreError> {
        let conn = self.conn.lock();
        let counts: Option<(i64, i64)> = conn
            .query_row(
                "SELECT model_calls, connector_calls FROM daily_spend WHERE day = ?1",
                params![day],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        counts
            .map(|(model, connector)| {
                Ok((
                    u64::try_from(model).map_err(|_| StoreError::NumericRange)?,
                    u64::try_from(connector).map_err(|_| StoreError::NumericRange)?,
                ))
            })
            .unwrap_or(Ok((0, 0)))
    }

    #[cfg(test)]
    fn daily_breach_alert_state(&self, day: &str) -> Result<i64, StoreError> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row(
                "SELECT alert_state FROM daily_spend WHERE day = ?1",
                params![day],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }
}

pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS daily_spend (
            day TEXT PRIMARY KEY,
            model_calls INTEGER NOT NULL DEFAULT 0,
            connector_calls INTEGER NOT NULL DEFAULT 0,
            breached_at TEXT,
            alert_state INTEGER NOT NULL DEFAULT 0
        );",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    #[test]
    fn model_reserve_is_atomic_and_capped() {
        let store = Store::open_in_memory().expect("store");
        assert!(store.reserve_daily_model_call("2026-07-17", 2).unwrap().0);
        assert!(store.reserve_daily_model_call("2026-07-17", 2).unwrap().0);
        let denied = store.reserve_daily_model_call("2026-07-17", 2).unwrap();
        assert!(!denied.0);
        assert!(denied.1, "denial at cap must report first breach");
        assert_eq!(store.daily_spend_counts("2026-07-17").unwrap(), (2, 0));
        assert_eq!(
            store.pending_daily_breach_alert_days().unwrap(),
            vec!["2026-07-17"]
        );
    }

    #[test]
    fn connector_reserve_is_atomic_capped_and_day_scoped() {
        let store = Store::open_in_memory().expect("store");
        assert!(
            store
                .reserve_daily_connector_call("2026-07-17", 1)
                .unwrap()
                .0
        );
        let denied = store.reserve_daily_connector_call("2026-07-17", 1).unwrap();
        assert!(!denied.0);
        assert!(denied.1);
        assert_eq!(
            store.pending_daily_breach_alert_days().unwrap(),
            vec!["2026-07-17"]
        );
        assert!(
            store
                .reserve_daily_connector_call("2026-07-18", 1)
                .unwrap()
                .0
        );
        assert_eq!(store.daily_spend_counts("2026-07-17").unwrap(), (0, 1));
        assert_eq!(store.daily_spend_counts("2026-07-18").unwrap(), (0, 1));
    }

    #[test]
    fn breach_is_recorded_exactly_once_per_day() {
        let store = Store::open_in_memory().expect("store");
        // Drive the model counter to the cap so the next admission breaches.
        assert!(store.reserve_daily_model_call("2026-07-17", 1).unwrap().0);
        let (adm1, first1) = store
            .check_and_mark_daily_breach("2026-07-17", 1, 10)
            .unwrap();
        assert!(!adm1);
        assert!(first1, "first over-cap attempt must report first breach");
        // Concurrent/replayed checks must NOT report a second first breach.
        let (adm2, first2) = store
            .check_and_mark_daily_breach("2026-07-17", 1, 10)
            .unwrap();
        assert!(!adm2);
        assert!(!first2);
        // A new day with a zero cap is a fresh breach.
        assert!(
            store
                .check_and_mark_daily_breach("2026-07-18", 0, 0)
                .unwrap()
                .1
        );
    }

    #[test]
    fn fresh_day_with_zero_cap_is_a_hard_denial() {
        let store = Store::open_in_memory().expect("store");
        // No row exists yet; a zero cap must still deny (no silent admission).
        let (adm, first) = store
            .check_and_mark_daily_breach("2026-07-17", 0, 0)
            .unwrap();
        assert!(!adm);
        assert!(
            first,
            "first breach must be reported even on a fresh zero-cap day"
        );
    }

    #[test]
    fn admission_below_cap_is_allowed_and_does_not_breach() {
        let store = Store::open_in_memory().expect("store");
        let (adm, first) = store
            .check_and_mark_daily_breach("2026-07-17", 5, 5)
            .unwrap();
        assert!(adm);
        assert!(!first);
    }

    #[test]
    fn alert_fires_drains_and_rearms_on_new_day() {
        let store = Store::open_in_memory().expect("store");
        let (_, first) = store
            .check_and_mark_daily_breach("2026-07-17", 0, 0)
            .unwrap();
        assert!(first);
        assert_eq!(store.daily_breach_alert_state("2026-07-17").unwrap(), 1);
        assert!(store.claim_daily_breach_alert("2026-07-17").unwrap());
        store.complete_daily_breach_alert("2026-07-17").unwrap();
        assert_eq!(store.daily_breach_alert_state("2026-07-17").unwrap(), 0);
        let (_, next_first) = store
            .check_and_mark_daily_breach("2026-07-18", 0, 0)
            .unwrap();
        assert!(next_first);
        assert_eq!(store.daily_breach_alert_state("2026-07-18").unwrap(), 1);
    }

    #[test]
    fn inflight_alert_is_rearmed_for_crash_recovery() {
        let store = Store::open_in_memory().expect("store");
        let (_, first) = store
            .check_and_mark_daily_breach("2026-07-17", 0, 0)
            .unwrap();
        assert!(first);
        assert!(store.claim_daily_breach_alert("2026-07-17").unwrap());
        assert_eq!(store.daily_breach_alert_state("2026-07-17").unwrap(), 2);
        store.reset_inflight_breach_alerts().unwrap();
        assert_eq!(store.daily_breach_alert_state("2026-07-17").unwrap(), 1);
        assert!(store.claim_daily_breach_alert("2026-07-17").unwrap());
        store.complete_daily_breach_alert("2026-07-17").unwrap();
        assert_eq!(store.daily_breach_alert_state("2026-07-17").unwrap(), 0);
    }

    #[test]
    fn pending_alert_recovery_includes_prior_utc_days() {
        let store = Store::open_in_memory().expect("store");
        assert!(
            store
                .check_and_mark_daily_breach("2026-07-16", 0, 0)
                .unwrap()
                .1
        );
        assert!(
            store
                .check_and_mark_daily_breach("2026-07-17", 0, 0)
                .unwrap()
                .1
        );
        assert_eq!(
            store.pending_daily_breach_alert_days().unwrap(),
            vec!["2026-07-16", "2026-07-17"]
        );
    }

    #[test]
    fn same_day_breach_is_consumed_once_without_rearming() {
        let store = Store::open_in_memory().expect("store");
        assert!(
            store
                .check_and_mark_daily_breach("2026-07-17", 0, 0)
                .unwrap()
                .1
        );
        assert!(store.claim_daily_breach_alert("2026-07-17").unwrap());
        store.complete_daily_breach_alert("2026-07-17").unwrap();
        let (admitted, first) = store
            .check_and_mark_daily_breach("2026-07-17", 0, 0)
            .unwrap();
        assert!(!admitted);
        assert!(!first);
        assert!(store.pending_daily_breach_alert_days().unwrap().is_empty());
    }
    #[test]
    fn file_backed_ledger_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spend.db");
        {
            let store = Store::open(&path).unwrap();
            assert!(store.reserve_daily_model_call("2026-07-17", 2).unwrap().0);
        }
        let reopened = Store::open(&path).unwrap();
        assert_eq!(reopened.daily_spend_counts("2026-07-17").unwrap(), (1, 0));
        assert_eq!(reopened.daily_spend_counts("2026-07-18").unwrap(), (0, 0));
    }

    #[test]
    fn concurrent_reservations_never_cross_cap() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let barrier = Arc::new(Barrier::new(8));
        let mut joins = Vec::new();
        for _ in 0..8 {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            joins.push(thread::spawn(move || {
                barrier.wait();
                store.reserve_daily_model_call("2026-07-17", 3).unwrap().0
            }));
        }
        let allowed = joins
            .into_iter()
            .map(|join| join.join().expect("reservation worker"))
            .filter(|allowed| *allowed)
            .count();
        assert_eq!(allowed, 3);
        assert_eq!(store.daily_spend_counts("2026-07-17").unwrap(), (3, 0));
    }
}

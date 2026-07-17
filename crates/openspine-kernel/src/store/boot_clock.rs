//! Boot clock high-water regression detection (AD-139).
//!
//! Timestamps, token expiry, breaker timeouts, and audit rows all trust the
//! wall clock (NTP assumed; the audit chain itself orders by append `seq`).
//! AD-139 requires clock-regression detection at boot: if the wall clock
//! moved backwards past the persisted high-water mark beyond a tolerance
//! window, the kernel surfaces it loudly rather than serving on top of a
//! regressed clock. Split out of `store/mod.rs` for the 500-line gate.

use super::{Store, StoreError};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};

/// Tolerance for minor NTP slew / monotonic-vs-wall jitter. A regression
/// larger than this window is treated as a real clock step backwards.
pub(super) const CLOCK_REGRESSION_TOLERANCE_MS: i64 = 60_000;

/// Key under which the high-water millis is persisted in `boot_meta`.
const HIGH_WATER_KEY: &str = "clock.high_water_ms";

/// Result of a boot clock check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootClockCheck {
    /// The wall clock is sane; the high-water was advanced to `now_ms` if it
    /// was later than the previous mark.
    Ok { high_water_ms: i64 },
    /// The wall clock regressed past the high-water beyond tolerance. The
    /// caller MUST surface this loudly (e.g. refuse to start); the stored
    /// high-water is NOT lowered.
    Regressed { high_water_ms: i64, now_ms: i64 },
}

impl Store {
    fn classify_boot_clock_tx(
        tx: &Transaction<'_>,
        now_ms: i64,
    ) -> Result<BootClockCheck, StoreError> {
        let prev: Option<i64> = tx
            .query_row(
                "SELECT value FROM boot_meta WHERE key = ?1",
                rusqlite::params![HIGH_WATER_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|s| s.parse::<i64>())
            .transpose()
            .map_err(|err| StoreError::TimestampRange(format!("boot high-water: {err}")))?;
        match prev {
            Some(high_water_ms)
                if now_ms < high_water_ms.saturating_sub(CLOCK_REGRESSION_TOLERANCE_MS) =>
            {
                Ok(BootClockCheck::Regressed {
                    high_water_ms,
                    now_ms,
                })
            }
            _ => Ok(BootClockCheck::Ok {
                high_water_ms: prev.map_or(now_ms, |p| p.max(now_ms)),
            }),
        }
    }

    fn commit_boot_clock_tx(
        tx: &Transaction<'_>,
        check: &BootClockCheck,
    ) -> Result<(), StoreError> {
        let BootClockCheck::Ok { high_water_ms } = check else {
            return Ok(());
        };
        tx.execute(
            "INSERT INTO boot_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = CASE
               WHEN CAST(excluded.value AS INTEGER) > CAST(boot_meta.value AS INTEGER)
               THEN excluded.value ELSE boot_meta.value END",
            rusqlite::params![HIGH_WATER_KEY, high_water_ms.to_string()],
        )?;
        Ok(())
    }

    /// Compare and persist the supplied wall-clock millis in one
    /// `BEGIN IMMEDIATE` transaction. The read, classification, and
    /// max-preserving upsert share one serialized snapshot.
    pub fn check_boot_clock(&self, now_ms: i64) -> Result<BootClockCheck, StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let check = Self::classify_boot_clock_tx(&tx, now_ms)?;
        Self::commit_boot_clock_tx(&tx, &check)?;
        tx.commit()?;
        Ok(check)
    }

    /// Validate a candidate boot timestamp without persisting it. Startup
    /// uses this before fallible initialization so a failed attempt cannot
    /// poison retries with a future high-water.
    pub fn validate_boot_clock(&self, now_ms: i64) -> Result<BootClockCheck, StoreError> {
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction()?;
        Self::classify_boot_clock_tx(&tx, now_ms)
    }

    /// Re-read and persist a validated boot timestamp once startup is ready.
    /// The immediate transaction prevents a stale candidate from lowering the
    /// high-water if another writer advanced it meanwhile.
    pub fn commit_boot_clock(&self, now_ms: i64) -> Result<BootClockCheck, StoreError> {
        self.check_boot_clock(now_ms)
    }

    /// Durably record a runtime wall-clock observation, preserving the
    /// greatest value seen across heartbeats and process restarts.
    pub fn observe_runtime_clock(&self, now_ms: i64) -> Result<i64, StoreError> {
        let check = self.check_boot_clock(now_ms)?;
        match check {
            BootClockCheck::Ok { high_water_ms } => Ok(high_water_ms),
            BootClockCheck::Regressed {
                high_water_ms,
                now_ms,
            } => Err(StoreError::ClockRegression(format!(
                "runtime wall clock regressed: now ({now_ms} ms) is behind persisted high-water ({high_water_ms} ms)"
            ))),
        }
    }
}

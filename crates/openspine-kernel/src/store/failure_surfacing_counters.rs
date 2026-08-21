//! Per-connector success/failure counters (AD-138). Split from
//! `failure_surfacing.rs` to keep that file under the 500-line gate; the same
//! counters AD-103's breaker and AD-013's calibration signal read.

use super::{Store, StoreError};
use rusqlite::params;
#[cfg(test)]
use rusqlite::OptionalExtension;

impl Store {
    /// Increment `connector`'s `outcome` counter ("success" or "failure").
    /// The same counters AD-103's breaker and AD-013's calibration signal
    /// will read (AD-138) — this change only owns the write side.
    pub fn increment_connector_outcome(
        &self,
        connector: &str,
        outcome: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO connector_counters (connector, outcome, count) VALUES (?1, ?2, 1) \
             ON CONFLICT(connector, outcome) DO UPDATE SET count = count + 1",
            params![connector, outcome],
        )?;
        Ok(())
    }

    /// Current count for one `(connector, outcome)` pair; `0` if never
    /// recorded.
    #[cfg(test)]
    pub fn connector_counter(&self, connector: &str, outcome: &str) -> Result<i64, StoreError> {
        let conn = self.conn.lock();
        let count: Option<i64> = conn
            .query_row(
                "SELECT count FROM connector_counters WHERE connector = ?1 AND outcome = ?2",
                params![connector, outcome],
                |row| row.get(0),
            )
            .optional()?;
        Ok(count.unwrap_or(0))
    }
}

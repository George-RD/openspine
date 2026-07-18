//! Durable pending evidence for Gmail draft writes (candidate extension).
//!
//! A row is inserted *before* the provider write, referencing the
//! `action_request_id` whose `action_requests` row already durably holds
//! everything needed to reconstruct the write (`payload_ref`, `target_digest`,
//! `target_ref.id` — see `pipeline::approval::create_approved_draft` and
//! `Store::find_action_request`), so this table does not duplicate that
//! payload.
//!
//! A confirmed provider response resolves the row. A timeout deliberately
//! leaves it `pending`: the runtime never claims the write failed just because
//! the response was lost. Because `gmail.create_draft` has no idempotency key,
//! this crate performs NO automatic resend — an operator reconciles a `pending`
//! row manually (re-checking Gmail for an already-created draft), which is why
//! the row stays queryable rather than being silently retried. This is an
//! unnumbered candidate extension analogous to the canonical owner-delivery
//! decision.

use super::{Store, StoreError};
use rusqlite::params;

impl Store {
    pub(crate) fn insert_pending_draft_write(
        &self,
        id: ulid::Ulid,
        grant_id: ulid::Ulid,
        action_request_id: ulid::Ulid,
        thread_id: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO pending_draft_writes
             (id, grant_id, action_request_id, thread_id, created_at, state)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending')",
            params![
                id.to_string(),
                grant_id.to_string(),
                action_request_id.to_string(),
                thread_id,
                jiff::Timestamp::now().to_string(),
            ],
        )?;
        Ok(())
    }

    /// Mark a row definitively done (confirmed success or confirmed failure).
    /// Never called for a delivery-unknown timeout, which must leave the row
    /// queryable as `pending` for manual reconciliation.
    pub(crate) fn resolve_pending_draft_write(&self, id: ulid::Ulid) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE pending_draft_writes SET state = 'resolved', resolved_at = ?2
             WHERE id = ?1",
            params![id.to_string(), jiff::Timestamp::now().to_string()],
        )?;
        Ok(())
    }
}

pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS pending_draft_writes (
            id TEXT PRIMARY KEY,
            grant_id TEXT NOT NULL,
            action_request_id TEXT NOT NULL,
            thread_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            state TEXT NOT NULL CHECK (state IN ('pending', 'resolved')),
            resolved_at TEXT
        );",
    )?;
    Ok(())
}
#[cfg(test)]
impl Store {
    /// Test-only observability for the draft-write pending-evidence candidate
    /// (D-071 precedent): how many draft writes are still
    /// awaiting manual reconciliation (delivery-unknown, never claimed
    /// failed by an automatic resend).
    pub(crate) fn count_pending_draft_writes(&self) -> Result<usize, StoreError> {
        let conn = self.conn.lock();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pending_draft_writes WHERE state = 'pending'",
            [],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count).unwrap_or(0))
    }
}

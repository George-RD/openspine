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
use openspine_schemas::digest::{digest_of, Digest};
use rusqlite::params;
use serde_json::json;

/// Stable identity for one protected Gmail draft request. The fingerprint
/// names only the action and kernel-resolved references; it never includes
/// body plaintext, recipients, timestamps, or provider IDs.
pub(crate) fn draft_request_fingerprint(
    action: &str,
    thread_id: &str,
    target_digest: &Digest,
    payload_digest: &Digest,
) -> String {
    digest_of(&json!({
        "action": action,
        "thread_id": thread_id,
        "target_digest": target_digest,
        "payload_digest": payload_digest,
    }))
    .to_string()
}

impl Store {
    #[cfg(test)]
    pub(crate) fn insert_pending_draft_write(
        &self,
        id: ulid::Ulid,
        grant_id: ulid::Ulid,
        action_request_id: ulid::Ulid,
        thread_id: &str,
        request_fingerprint: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO pending_draft_writes
             (id, grant_id, action_request_id, thread_id, request_fingerprint, created_at, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending')",
            params![
                id.to_string(),
                grant_id.to_string(),
                action_request_id.to_string(),
                thread_id,
                request_fingerprint,
                jiff::Timestamp::now().to_string(),
            ],
        )?;
        Ok(())
    }
    /// Check-and-insert the pending-write fence on an open transaction, so the
    /// seam ([`Store::begin_effect`]) can pair the claim with its audit row in
    /// one `Immediate` transaction. Returns `false` (no insert) when an
    /// unresolved fence already exists for `request_fingerprint`. The caller
    /// owns the enclosing `BEGIN IMMEDIATE`, which is what serializes concurrent
    /// claims (D-050 TOCTOU closure).
    pub(super) fn claim_pending_draft_write_conn(
        tx: &rusqlite::Transaction,
        id: ulid::Ulid,
        grant_id: ulid::Ulid,
        action_request_id: ulid::Ulid,
        thread_id: &str,
        request_fingerprint: &str,
    ) -> Result<bool, StoreError> {
        let pending: i64 = tx.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM pending_draft_writes
                WHERE request_fingerprint = ?1 AND state = 'pending'
            )",
            params![request_fingerprint],
            |row| row.get(0),
        )?;
        if pending != 0 {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO pending_draft_writes
             (id, grant_id, action_request_id, thread_id, request_fingerprint, created_at, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending')",
            params![
                id.to_string(),
                grant_id.to_string(),
                action_request_id.to_string(),
                thread_id,
                request_fingerprint,
                jiff::Timestamp::now().to_string(),
            ],
        )?;
        Ok(true)
    }

    /// Atomically claim the protected request for one provider write. The
    /// check and insert share one `BEGIN IMMEDIATE` transaction, so concurrent
    /// callbacks with the same fingerprint cannot both reach Gmail. Production
    /// claims run through [`Store::begin_effect`], which pairs the claim with
    /// its audit row; this standalone wrapper exercises the claim serialization
    /// in isolation.
    #[cfg(test)]
    pub(crate) fn claim_pending_draft_write(
        &self,
        id: ulid::Ulid,
        grant_id: ulid::Ulid,
        action_request_id: ulid::Ulid,
        thread_id: &str,
        request_fingerprint: &str,
    ) -> Result<bool, StoreError> {
        self.with_immediate_tx(|tx| {
            Self::claim_pending_draft_write_conn(
                tx,
                id,
                grant_id,
                action_request_id,
                thread_id,
                request_fingerprint,
            )
        })
    }

    /// Whether an unresolved provider write already exists for this exact
    /// protected request reference. A hit is a retry fence, not an
    /// exactly-once claim: an operator may resolve it after checking Gmail.
    pub(crate) fn has_pending_draft_write(
        &self,
        request_fingerprint: &str,
    ) -> Result<bool, StoreError> {
        let conn = self.conn.lock();
        let pending: i64 = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM pending_draft_writes
                WHERE request_fingerprint = ?1 AND state = 'pending'
            )",
            params![request_fingerprint],
            |row| row.get(0),
        )?;
        Ok(pending != 0)
    }

    /// Mark a row `resolved` on an open transaction, so the seam
    /// ([`Store::settle_effect`]) can pair the resolution with its audit row in
    /// one `Immediate` transaction. Never called for a delivery-unknown
    /// timeout, which must leave the row `pending` for manual reconciliation.
    pub(super) fn resolve_pending_draft_write_conn(
        tx: &rusqlite::Transaction,
        id: ulid::Ulid,
    ) -> Result<(), StoreError> {
        tx.execute(
            "UPDATE pending_draft_writes SET state = 'resolved', resolved_at = ?2
             WHERE id = ?1",
            params![id.to_string(), jiff::Timestamp::now().to_string()],
        )?;
        Ok(())
    }

    /// Mark a row definitively done (confirmed success or confirmed failure).
    /// Production resolutions run through [`Store::settle_effect`]; this
    /// standalone wrapper resolves a row directly for regression tests.
    #[cfg(test)]
    pub(crate) fn resolve_pending_draft_write(&self, id: ulid::Ulid) -> Result<(), StoreError> {
        self.with_immediate_tx(|tx| Self::resolve_pending_draft_write_conn(tx, id))
    }
}

pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS pending_draft_writes (
            id TEXT PRIMARY KEY,
            grant_id TEXT NOT NULL,
            action_request_id TEXT NOT NULL,
            thread_id TEXT NOT NULL,
            request_fingerprint TEXT NOT NULL,
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
#[cfg(test)]
mod tests {
    use super::*;
    use openspine_schemas::digest::Digest;

    #[test]
    fn pending_request_fingerprint_migrates_and_fences_by_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kernel.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE pending_draft_writes (
                    id TEXT PRIMARY KEY,
                    grant_id TEXT NOT NULL,
                    action_request_id TEXT NOT NULL,
                    thread_id TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    state TEXT NOT NULL,
                    resolved_at TEXT
                );",
            )
            .unwrap();
        }
        let store = Store::open(&path).unwrap();
        let digest = Digest::parse(format!("sha256:{}", "a".repeat(64))).unwrap();
        let fingerprint =
            draft_request_fingerprint("email.create_draft", "thread-1", &digest, &digest);
        store
            .insert_pending_draft_write(
                ulid::Ulid::new(),
                ulid::Ulid::new(),
                ulid::Ulid::new(),
                "thread-1",
                &fingerprint,
            )
            .unwrap();
        assert!(store.has_pending_draft_write(&fingerprint).unwrap());
        assert!(!store
            .has_pending_draft_write(&draft_request_fingerprint(
                "email.create_draft",
                "thread-2",
                &digest,
                &digest
            ))
            .unwrap());
    }
    #[test]
    fn concurrent_pending_claims_have_one_winner() {
        // Two independent connections against one file race to claim the same
        // fingerprint. `with_immediate_tx`'s BEGIN IMMEDIATE must serialize
        // them so exactly one INSERT lands (D-050 write-serialization / TOCTOU
        // closure, cf. D-167/D-169). The loser blocks on the write lock, then
        // finds the pending row inside its own transaction and returns
        // Ok(false) rather than a second INSERT.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kernel.db");
        let first = Store::open(&path).unwrap();
        let second = Store::open(&path).unwrap();
        first
            .conn
            .lock()
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        second
            .conn
            .lock()
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let digest = Digest::parse(format!("sha256:{}", "b".repeat(64))).unwrap();
        let fingerprint =
            draft_request_fingerprint("email.create_draft", "thread-1", &digest, &digest);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let run = |store: Store| {
            let barrier = std::sync::Arc::clone(&barrier);
            let fingerprint = fingerprint.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.claim_pending_draft_write(
                    ulid::Ulid::new(),
                    ulid::Ulid::new(),
                    ulid::Ulid::new(),
                    "thread-1",
                    &fingerprint,
                )
            })
        };
        let first_handle = run(first);
        let second_handle = run(second);
        let first_result = first_handle.join().unwrap();
        let second_result = second_handle.join().unwrap();
        // Neither writer errored; the loser blocked and then observed the row.
        assert!(first_result.is_ok(), "first: {first_result:?}");
        assert!(second_result.is_ok(), "second: {second_result:?}");
        let winners = usize::from(matches!(first_result, Ok(true)))
            + usize::from(matches!(second_result, Ok(true)));
        assert_eq!(winners, 1);
        let observer = Store::open(&path).unwrap();
        assert_eq!(observer.count_pending_draft_writes().unwrap(), 1);
    }
}

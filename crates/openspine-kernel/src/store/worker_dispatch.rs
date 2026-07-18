// openspine:allow-large-module reason: receipt-bound worker dispatch persistence and idempotency share one transaction boundary
//! Worker commissioning dispatch state (AD-035 / D-083 / D-073).
//!
//! A commissioned worker owns one row here: `dispatched` once its grant +
//! briefcase are atomically persisted, `terminal` once it reports a result
//! (or is known to have failed). The result is recorded as a bus event
//! (audit_log row) with the worker grant id as its aggregate — the master
//! consumes it through the ordinary event-bus path (AD-035: results return
//! as events). Recording is receipt-keyed and fail-closed (D-083): a result
//! for an already-terminal dispatch is rejected, never replayed.

use super::{Store, StoreError};
use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::briefcase::Briefcase;
use openspine_schemas::digest::Digest;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::worker::{WorkerIdentity, WorkerResult};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use ulid::Ulid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum WorkerDispatchState {
    Dispatched,
    Terminal,
}

/// Persist the worker dispatch, grant, briefcase, and authority receipt in
/// one transaction. The explicit receipt key makes commission retries safe
/// without collapsing intentionally identical commissions.
///
#[allow(clippy::too_many_arguments)]
pub fn record_worker_commissioned(
    store: &Store,
    parent_grant_id: Ulid,
    grant: &TaskGrant,
    pending_ref: &ArtifactRef,
    token_ref: &ArtifactRef,
    bound_chat_id: i64,
    briefcase: &Briefcase,
    receipt: &str,
    request_digest: &Digest,
    identity: &WorkerIdentity,
    connector: &str,
) -> Result<Ulid, StoreError> {
    let mut redacted = grant.clone();
    redacted.task_token = String::new();
    let mut conn = store.conn.lock();
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(StoreError::from)?;
    if !grant.effectively_allows(&ActionId::new("worker.report_result")) {
        return Err(StoreError::WorkerCannotReportResults);
    }
    let already_recorded: Option<String> = tx
        .query_row(
            "SELECT grant_id FROM worker_dispatch WHERE receipt_key = ?1 LIMIT 1",
            params![receipt],
            |row| row.get(0),
        )
        .optional()
        .map_err(StoreError::from)?;
    if let Some(original_grant_id) = already_recorded {
        tx.commit().map_err(StoreError::from)?;
        return Ulid::from_string(&original_grant_id)
            .map_err(|_| StoreError::BadUlid("worker_dispatch.grant_id".into()));
    }
    // A worker may commission only while its worker parent is still live.
    // Master grants do not have a worker_dispatch row and remain valid.
    let parent_state: Option<String> = tx
        .query_row(
            "SELECT state FROM worker_dispatch WHERE grant_id = ?1",
            params![parent_grant_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(StoreError::from)?;
    if matches!(parent_state.as_deref(), Some("terminal")) {
        return Err(StoreError::WorkerDispatchAlreadyFailed);
    }
    // A fresh commission is the only restart/recomposition entry point. Fail
    // closed once this connector has exhausted the default 3/30s intensity
    // budget; duplicate receipts above are still idempotent and return their
    // original grant without creating another dispatch.
    // Count the connector failures already charged within the sliding window.
    // The configured limit is the last failure that exhausts continuation: once
    // `recent_failures >= restart_limit` rows are present, the next fresh
    // commission is refused (the failed worker's grant is never inherited).
    let cutoff = (Timestamp::now() - std::time::Duration::from_secs(30)).to_string();
    let recent_failures: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM connector_restart_ledger \
             WHERE connector = ?1 AND occurred_at > ?2",
            params![connector, cutoff],
            |row| row.get(0),
        )
        .map_err(StoreError::from)?;
    if recent_failures >= 3 {
        return Err(StoreError::WorkerRestartCapExceeded(connector.to_string()));
    }
    tx.execute(
        "INSERT INTO task_grants
         (id, task_token, expires_at, grant_json, pending_message_digest, bound_chat_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            grant.id.to_string(),
            super::budget_support::hash_task_token(&grant.task_token),
            grant.expires_at.to_string(),
            serde_json::to_string(&redacted)?,
            pending_ref.digest.as_str(),
            bound_chat_id,
        ],
    )?;
    tx.execute(
        "INSERT INTO briefcases (task_grant_id, briefcase_json) VALUES (?1, ?2)",
        params![grant.id.to_string(), serde_json::to_string(briefcase)?],
    )?;
    tx.execute(
        "INSERT INTO worker_dispatch
         (grant_id, parent_grant_id, state, receipt_key, request_digest, token_ref,
          identity_owner, identity_conversation, identity_task, connector, created_at, updated_at)
         VALUES (?1, ?2, 'dispatched', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        params![
            grant.id.to_string(),
            parent_grant_id.to_string(),
            receipt,
            request_digest.as_str(),
            serde_json::to_string(token_ref)?,
            identity.owner,
            identity.conversation,
            identity.task,
            connector,
            jiff::Timestamp::now().to_string(),
        ],
    )?;
    Store::append_audit_conn_with_options(
        &tx,
        "authority.granted",
        None,
        None,
        None,
        Some(grant.id),
        &[],
        std::slice::from_ref(pending_ref),
        None,
        None,
    )?;
    tx.commit().map_err(StoreError::from)?;
    Ok(grant.id)
}
/// Resolution of a commission receipt lookup. The receipt is bound to the
/// commissioning parent grant id AND the canonical request digest, so a
/// receipt reused under a different parent or a different request payload is
/// reported as a mismatch rather than silently resolving to the prior grant
/// (blocker 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommissionReceipt {
    /// No dispatch row carries this receipt key.
    None,
    /// The receipt matches this parent grant and request digest; the prior
    /// grant id is returned for an idempotent retry.
    Match { worker_grant_id: Ulid },
    /// A dispatch row exists for this receipt but under a different parent
    /// grant or request digest — a caller must reject this (BadRequest),
    /// never reuse the prior grant.
    Mismatch,
}

/// Resolve a commission receipt, binding it to `parent_grant_id` and
/// `request_digest`. Callers must treat [`CommissionReceipt::Mismatch`] as a
/// rejection (a receipt is single-use per parent+request).
pub fn commissioned_grant_for_receipt(
    store: &Store,
    parent_grant_id: Ulid,
    request_digest: &Digest,
    receipt: &str,
) -> Result<CommissionReceipt, StoreError> {
    let conn = store.conn.lock();
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT grant_id, parent_grant_id, request_digest \
             FROM worker_dispatch WHERE receipt_key = ?1",
            params![receipt],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(StoreError::from)?;
    match row {
        None => Ok(CommissionReceipt::None),
        Some((grant_id, stored_parent, stored_digest)) => {
            let matches = stored_parent == parent_grant_id.to_string()
                && stored_digest == request_digest.as_str();
            if matches {
                Ok(CommissionReceipt::Match {
                    worker_grant_id: Ulid::from_string(&grant_id)
                        .map_err(|_| StoreError::BadUlid("worker_dispatch.grant_id".into()))?,
                })
            } else {
                Ok(CommissionReceipt::Mismatch)
            }
        }
    }
}

/// Return every dispatched worker that is stranded (no result recorded, past
/// the grace window) and has NOT yet been surfaced to the owner. The caller
/// should notify the owner via failure_surfacing and then call
/// [`mark_worker_stranded_notified`] to prevent duplicate notification.
pub fn pending_worker_dispatches(
    store: &Store,
    cutoff: jiff::Timestamp,
) -> Result<Vec<(Ulid, ArtifactRef)>, StoreError> {
    let conn = store.conn.lock();
    let cutoff = cutoff.to_string();
    let rows = conn
        .prepare(
            "SELECT grant_id, token_ref FROM worker_dispatch \
             WHERE state = 'dispatched' AND token_ref != '' \
             AND recovery_claimed_at IS NULL \
             AND created_at < ?1",
        )?
        .query_map(params![cutoff], |r| {
            let grant_id: String = r.get(0)?;
            let raw: String = r.get(1)?;
            Ok((grant_id, raw))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)?;
    rows.into_iter()
        .map(|(grant_id, raw)| {
            let grant_id = Ulid::from_string(&grant_id)
                .map_err(|_| StoreError::BadUlid("worker_dispatch.grant_id".into()))?;
            let token_ref = serde_json::from_str::<ArtifactRef>(&raw).map_err(StoreError::from)?;
            Ok((grant_id, token_ref))
        })
        .collect()
}

/// Atomically enqueue an owner notification and mark a stranded dispatch as
/// surfaced. A failed enqueue or conditional mark rolls back both operations,
/// preserving eligibility for a later startup/watchdog sweep.
pub fn surface_stranded_worker(
    store: &Store,
    chat_id: i64,
    text_ref: &str,
    grant_id: Ulid,
    reason: &str,
) -> Result<(), StoreError> {
    if chat_id == 0 {
        return Err(StoreError::OwnerNotificationFailed(
            "stranded worker notification requires a resolvable owner chat".to_string(),
        ));
    }
    let mut conn = store.conn.lock();
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    Store::append_audit_conn(
        &tx,
        "owner.notify_failed",
        Some(&ActionId::new("owner.notify")),
        None,
        Some(reason),
        Some(grant_id),
        &[],
        &[],
    )?;
    let now = Timestamp::now().to_string();
    let notification_id = Ulid::new().to_string();
    tx.execute(
        "INSERT INTO notify_dead_letters \
         (id, enqueued_at, chat_id, text_ref, task_grant_id, digest_item_ids, attempts, next_attempt_at, state) \
         VALUES (?1, ?2, ?3, ?4, ?5, '', 0, ?2, 'pending')",
        params![
            notification_id,
            now,
            chat_id,
            text_ref,
            grant_id.to_string(),
        ],
    )?;
    let changed = tx.execute(
        "UPDATE worker_dispatch SET recovery_claimed_at = ?2 \
         WHERE grant_id = ?1 AND state = 'dispatched' AND recovery_claimed_at IS NULL",
        params![grant_id.to_string(), now],
    )?;
    if changed != 1 {
        return Err(StoreError::WorkerDispatchNotFound);
    }
    tx.commit()?;
    Ok(())
}

/// Legacy marking helper retained for focused store tests; production recovery
/// uses [`surface_stranded_worker`] so enqueue and marking are atomic.
#[cfg(test)]
pub fn mark_worker_stranded_notified(store: &Store, grant_id: Ulid) -> Result<(), StoreError> {
    let conn = store.conn.lock();
    let now = jiff::Timestamp::now().to_string();
    conn.execute(
        "UPDATE worker_dispatch SET recovery_claimed_at = ?2 \
         WHERE grant_id = ?1 AND recovery_claimed_at IS NULL",
        params![grant_id.to_string(), now],
    )?;
    Ok(())
}

/// Find dispatched rows that are older than `max_age` and have no result
/// recorded — these are workers whose shell exited without reporting a result.
/// Returns (grant_id, parent_grant_id) for each stranded worker.
/// The caller resolves the parent grant's bound owner chat before enqueueing.
pub fn stranded_worker_timeouts(
    store: &Store,
    max_age: std::time::Duration,
) -> Result<Vec<(Ulid, Ulid)>, StoreError> {
    let conn = store.conn.lock();
    let cutoff = (jiff::Timestamp::now() - max_age).to_string();
    let mut stmt = conn.prepare(
        "SELECT grant_id, parent_grant_id FROM worker_dispatch \
         WHERE state = 'dispatched' AND created_at < ?1 \
         AND recovery_claimed_at IS NULL",
    )?;
    let rows = stmt.query_map(params![cutoff], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)?
        .into_iter()
        .map(|(g, p)| {
            Ok((
                Ulid::from_string(&g)
                    .map_err(|_| StoreError::BadUlid("worker_dispatch.grant_id".into()))?,
                Ulid::from_string(&p)
                    .map_err(|_| StoreError::BadUlid("worker_dispatch.parent_grant_id".into()))?,
            ))
        })
        .collect()
}

/// Record a worker's structured result as a bus event and mark the dispatch
/// `terminal`. Receipt-keyed and fail-closed: if the dispatch is already
/// `terminal`, returns [`StoreError::WorkerResultAlreadyRecorded`] rather than
/// emitting a second event (D-083 / D-073).
///
/// The free-text notes (if any) are referenced by digest only — never stored
/// inline — so D-012 plaintext discipline holds on the audit ledger.
pub fn record_worker_result(
    store: &Store,
    worker_grant_id: Ulid,
    result: &WorkerResult,
) -> Result<(), StoreError> {
    let payload_json = serde_json::to_string(result)?;

    let mut conn = store.conn.lock();
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(StoreError::from)?;

    // D-083 / D-073 receipt check, inside the same transaction as the
    // terminal flip below so there is no TOCTOU window between "not yet
    // terminal" and "mark terminal": a result for an already-terminal
    // dispatch must not be replayed. Fail closed (honest denial, not a
    // crash).
    let state: Option<String> = tx
        .query_row(
            "SELECT state FROM worker_dispatch WHERE grant_id = ?1",
            params![worker_grant_id.to_string()],
            |r| r.get(0),
        )
        .optional()
        .map_err(StoreError::from)?;
    match state.as_deref() {
        None => return Err(StoreError::WorkerDispatchNotFound),
        Some("terminal") => return Err(StoreError::WorkerResultAlreadyRecorded),
        _ => {}
    }

    tx.execute(
        "UPDATE worker_dispatch SET state='terminal', updated_at=?2 WHERE grant_id=?1",
        params![
            worker_grant_id.to_string(),
            jiff::Timestamp::now().to_string()
        ],
    )?;

    // Append the result as a bus event on the worker grant's aggregate,
    // carrying the structured payload as JSON so the master's consumer sees
    // the actual outcome, not just a marker (D-073). The free-text notes
    // and each request's detail reference are emitted as `payload_refs`
    // (digest references, never bare ULIDs) so the owner's `/digest` path
    // can reach the DetailReceipt without any plaintext on the ledger
    // (D-012 / Fit 7). The rest of the result is JSON-inlined here for the
    // consumer, but every untrusted ref is a digest, never inline text.
    let mut payload_refs: Vec<ArtifactRef> = Vec::new();
    if let Some(notes_ref) = &result.notes_ref {
        payload_refs.push(notes_ref.clone());
    }
    for request in &result.requests {
        if let Some(detail_ref) = &request.detail_ref {
            payload_refs.push(detail_ref.clone());
        }
    }
    Store::append_audit_conn_with_options(
        &tx,
        "worker.result",
        None,
        None,
        None,
        Some(worker_grant_id),
        &[],
        &payload_refs,
        None,
        Some(&payload_json),
    )?;

    tx.commit().map_err(StoreError::from)?;
    Ok(())
}

/// Read a commissioned worker's current dispatch state.
#[allow(dead_code)]
pub fn worker_dispatch_state(
    store: &Store,
    worker_grant_id: Ulid,
) -> Result<Option<WorkerDispatchState>, StoreError> {
    let conn = store.conn.lock();
    let state: Option<String> = conn
        .query_row(
            "SELECT state FROM worker_dispatch WHERE grant_id = ?1",
            params![worker_grant_id.to_string()],
            |r| r.get(0),
        )
        .optional()
        .map_err(StoreError::from)?;
    Ok(match state.as_deref() {
        Some("dispatched") => Some(WorkerDispatchState::Dispatched),
        Some("terminal") => Some(WorkerDispatchState::Terminal),
        _ => None,
    })
}

/// Return whether this worker grant has reached any terminal dispatch state.
/// Authentication uses this marker so no worker token remains usable after
/// either a recorded failure or a successfully recorded result.
pub fn worker_dispatch_failed(store: &Store, worker_grant_id: Ulid) -> Result<bool, StoreError> {
    let conn = store.conn.lock();
    let terminal: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM worker_dispatch \
             WHERE grant_id = ?1 AND state = 'terminal'",
            params![worker_grant_id.to_string()],
            |r| r.get(0),
        )
        .optional()
        .map_err(StoreError::from)?;
    Ok(terminal.is_some())
}
/// Read the master (parent) grant id a commissioned worker descends from.
pub fn worker_parent_grant(
    store: &Store,
    worker_grant_id: Ulid,
) -> Result<Option<Ulid>, StoreError> {
    let conn = store.conn.lock();
    let row: Option<String> = conn
        .query_row(
            "SELECT parent_grant_id FROM worker_dispatch WHERE grant_id = ?1",
            params![worker_grant_id.to_string()],
            |r| r.get(0),
        )
        .optional()
        .map_err(StoreError::from)?;
    Ok(row.and_then(|s| Ulid::from_string(&s).ok()))
}
pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS worker_dispatch (
            grant_id TEXT PRIMARY KEY,
            parent_grant_id TEXT NOT NULL,
            state TEXT NOT NULL CHECK(state IN ('dispatched', 'terminal')),
            receipt_key TEXT NOT NULL DEFAULT '',
            request_digest TEXT NOT NULL DEFAULT '',
            token_ref TEXT NOT NULL DEFAULT '',
            recovery_claimed_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_worker_dispatch_parent
            ON worker_dispatch (parent_grant_id, state, updated_at);",
    )?;
    // Best-effort additive migration. Preserves the four legacy dispatch
    // columns (receipt_key, request_digest, token_ref, recovery_claimed_at)
    // alongside the new supervision columns (identity tuple, trusted
    // connector binding, restart counter, failure timestamp). Column types
    // differ, so each carries its own ADD form; the state CHECK is
    // intentionally unchanged (('dispatched','terminal')) so existing stores
    // are never rebuilt.
    for (col, ty) in [
        ("receipt_key", "TEXT"),
        ("request_digest", "TEXT"),
        ("token_ref", "TEXT"),
        ("recovery_claimed_at", "TEXT"),
        ("connector", "TEXT"),
        ("identity_owner", "TEXT"),
        ("identity_conversation", "TEXT"),
        ("identity_task", "TEXT"),
        ("restart_count", "INTEGER"),
        ("failed_at", "TEXT"),
    ] {
        if let Err(err) = conn.execute(
            &format!("ALTER TABLE worker_dispatch ADD COLUMN {col} {ty}"),
            [],
        ) {
            if !matches!(
                &err,
                rusqlite::Error::SqliteFailure(_, Some(msg)) if msg.contains("duplicate column name")
            ) {
                return Err(err.into());
            }
        }
    }
    // Safe for both fresh and legacy DBs: drop any legacy non-unique receipt
    // index first (CREATE UNIQUE INDEX IF NOT EXISTS on the same name is a
    // no-op over an existing non-unique index), then create a partial UNIQUE
    // index that binds each non-empty receipt to a single dispatch row. A
    // concurrent commission therefore cannot double-insert the same receipt
    // (idempotency and parent+request binding hold even under a lost-update
    // race). Empty/default rows are excluded so legacy data with no receipt
    // never trips the unique constraint.
    let _ = conn.execute_batch("DROP INDEX IF EXISTS idx_worker_dispatch_receipt;");
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_worker_dispatch_receipt_unique
            ON worker_dispatch (receipt_key) WHERE receipt_key <> '';",
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "worker_dispatch_tests.rs"]
mod worker_dispatch_tests;

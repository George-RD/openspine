//! Worker supervision with authority reset (AD-100) and identity-based
//! addressing (AD-102).
//!
//! A supervised worker that crashes is NEVER allowed to transfer its grant to
//! a replacement. The supervisor records a structured `worker_failed` event,
//! charges the per-connector restart ledger, and leaves the dead dispatch
//! terminal. Continuation can only happen through the *normal* pipeline
//! (re-composition mints a fresh grant) — failure handling never mints or
//! inherits a grant.
//!
//! Workers are addressed by an identity tuple `(owner, conversation, task)`
//! (AD-102), never by process handle. In-flight processing is serialized at
//! the `(owner, conversation)` scope so two tasks in one conversation cannot
//! race on briefcase/counter updates.
use super::{Store, StoreError};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::worker::{WorkerFailed, WorkerFailureReason, WorkerIdentity};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::json;
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::Duration;
use ulid::Ulid;

static COMMISSION_ADMISSION: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Serialize commission admission through the cap precheck and durable insert.
/// The guard is released before any worker execution await.
pub fn worker_commission_admission() -> MutexGuard<'static, ()> {
    COMMISSION_ADMISSION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Check the connector restart budget before minting worker authority.
pub fn connector_restart_cap_available(
    store: &Store,
    connector: &str,
    now: jiff::Timestamp,
    window: Duration,
    restart_limit: u32,
) -> Result<bool, StoreError> {
    let conn = store.conn.lock();
    let cutoff = (now - window).to_string();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM connector_restart_ledger \
             WHERE connector = ?1 AND occurred_at > ?2",
            rusqlite::params![connector, cutoff],
            |row| row.get(0),
        )
        .map_err(StoreError::from)?;
    Ok(count < i64::from(restart_limit))
}

/// Record a structured worker failure (AD-100).
///
/// Runs the terminal flip and the `worker.failed` audit event in one
/// `BEGIN IMMEDIATE` transaction, mirroring [`super::worker_dispatch::record_worker_result`]:
/// the failure and a concurrent late `worker.result` compete for the same
/// `dispatched -> terminal` flip, so exactly one wins and the dispatched row
/// can never accept both outcomes.
///
/// Fail-closed: if the dispatch is already terminal (completed or previously
/// failed), returns [`StoreError::WorkerDispatchAlreadyFailed`] and emits
/// nothing. Never re-emits a failure for a settled dispatch.
///
/// Charges the per-connector restart ledger and reports whether re-composition
/// is still permitted under `restart_limit` within `window`. This does NOT
/// respawn the worker — recomposition must run the normal pipeline and mint a
/// brand-new grant (AD-100 authority reset).
#[allow(clippy::type_complexity)]
pub fn record_worker_failed(
    store: &Store,
    worker_grant_id: Ulid,
    reason: WorkerFailureReason,
    detail_ref: Option<&ArtifactRef>,
    now: jiff::Timestamp,
    window: Duration,
    restart_limit: u32,
) -> Result<WorkerFailed, StoreError> {
    let _admission = worker_commission_admission();
    let mut conn = store.conn.lock();
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(StoreError::from)?;

    // Identity + connector columns are NULL on pre-supervision rows (legacy
    // stores that predate this change). Read them as Option and treat None as
    // an explicitly unbound identity — never silently rewrite it to a
    // synthetic connector, so an unbound row cannot masquerade as a real one
    // or dodge its own restart bucket.
    let row: Option<(
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = tx
        .query_row(
            "SELECT state, parent_grant_id, identity_owner, identity_conversation, \
             identity_task, connector, failed_at \
             FROM worker_dispatch WHERE grant_id = ?1",
            params![worker_grant_id.to_string()],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .optional()
        .map_err(StoreError::from)?;

    let (state, parent, io, ic, it, connector, failed_at) = match row {
        None => return Err(StoreError::WorkerDispatchNotFound),
        Some(t) => t,
    };
    let io = io.unwrap_or_default();
    let ic = ic.unwrap_or_default();
    let it = it.unwrap_or_default();
    if connector.is_none() {
        // Legacy rows cannot be assigned a restart bucket. They are still
        // terminalized in this transaction so the stale token is revoked.
        let changed = tx
            .execute(
                "UPDATE worker_dispatch SET state = 'terminal', failed_at = ?2,
                 updated_at = ?2 WHERE grant_id = ?1 AND state = 'dispatched'",
                params![worker_grant_id.to_string(), now.to_string()],
            )
            .map_err(StoreError::from)?;
        if changed != 1 {
            return Err(StoreError::WorkerDispatchAlreadyFailed);
        }
        let payload = json!({
            "worker_grant_id": worker_grant_id.to_string(),
            "reason": reason,
            "detail_ref": detail_ref,
            "connector": null,
            "recomposition_permitted": false,
            "restart_count": 0,
            "restart_limit": restart_limit,
        })
        .to_string();
        let payload_refs = detail_ref.map(std::slice::from_ref).unwrap_or(&[]);
        Store::append_audit_conn_with_options(
            &tx,
            "worker.failed",
            None,
            None,
            None,
            Some(worker_grant_id),
            &[],
            payload_refs,
            None,
            Some(&payload),
        )?;
        tx.commit().map_err(StoreError::from)?;
        return Err(StoreError::WorkerConnectorUnbound);
    }
    let connector = connector.ok_or(StoreError::WorkerConnectorUnbound)?;
    // Already terminal (completed result, or a prior failure). Fail closed:
    // a settled dispatch never accepts a second outcome.
    if state != "dispatched" {
        return Err(StoreError::WorkerDispatchAlreadyFailed);
    }
    let _ = failed_at; // informational only; state already gates re-entry.

    // Count connector failures in the sliding window BEFORE charging this one.
    // The configured limit is the last failure that exhausts continuation:
    // the event for that failure is denied and the commission boundary refuses
    // any fresh recomposition until the window expires.
    let cutoff = (now - window).to_string();
    let prior: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM connector_restart_ledger \
             WHERE connector = ?1 AND occurred_at > ?2",
            params![connector, cutoff],
            |r| r.get(0),
        )
        .map_err(StoreError::from)?;
    let prior = prior as u32;
    let restart_count = prior + 1;
    // Continuation is permitted only while the *post-increment* failure count
    // stays under the limit. The limit-hitting failure therefore reports
    // `recomposition_permitted = false`, matching the commission boundary
    // that refuses the next fresh recomposition.
    let recomposition_permitted = restart_count < restart_limit;

    // Competing terminal flip: only a still-`dispatched` row flips. If a late
    // result won the race, this affects 0 rows and we fail closed.
    let changed = tx
        .execute(
            "UPDATE worker_dispatch \
             SET state = 'terminal', failed_at = ?2, restart_count = ?3, updated_at = ?4 \
             WHERE grant_id = ?1 AND state = 'dispatched'",
            params![
                worker_grant_id.to_string(),
                now.to_string(),
                restart_count as i64,
                now.to_string()
            ],
        )
        .map_err(StoreError::from)?;
    if changed != 1 {
        return Err(StoreError::WorkerDispatchAlreadyFailed);
    }

    // Charge the per-connector restart ledger.
    tx.execute(
        "INSERT INTO connector_restart_ledger (id, connector, occurred_at) \
         VALUES (?1, ?2, ?3)",
        params![Ulid::new().to_string(), connector, now.to_string()],
    )
    .map_err(StoreError::from)?;
    // Reclaim the crashed worker's in-flight conversation slot atomically so
    // the conversation is not permanently locked (AD-102 continuation path).
    // The claim is keyed by this worker's grant id, so only its own slot is
    // released — a newer/parallel claim is never cleared.
    tx.execute(
        "DELETE FROM conversation_in_flight \
         WHERE owner = ?1 AND conversation = ?2 AND grant_id = ?3",
        params![io, ic, worker_grant_id.to_string()],
    )
    .map_err(StoreError::from)?;

    let event = WorkerFailed {
        worker_grant_id,
        parent_grant_id: Ulid::from_string(&parent)
            .map_err(|_| StoreError::BadUlid("worker_dispatch.parent_grant_id".into()))?,
        identity: WorkerIdentity {
            owner: io,
            conversation: ic,
            task: it,
        },
        connector,
        reason,
        detail_ref: detail_ref.cloned(),
        restart_count,
        restart_limit,
        recomposition_permitted,
        occurred_at: now,
    };
    let payload_json = serde_json::to_string(&event)?;
    let mut payload_refs: Vec<ArtifactRef> = Vec::new();
    if let Some(d) = detail_ref {
        payload_refs.push(d.clone());
    }
    Store::append_audit_conn_with_options(
        &tx,
        "worker.failed",
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
    Ok(event)
}

/// Count connector failures within `window` ending at `now`. Used by the
/// supervisor and by tests to assert the cap holds under a flaky connector.
#[cfg(test)]
pub fn connector_restart_count_in_window(
    store: &Store,
    connector: &str,
    window: Duration,
    now: jiff::Timestamp,
) -> Result<u32, StoreError> {
    let conn = store.conn.lock();
    let cutoff = (now - window).to_string();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM connector_restart_ledger \
             WHERE connector = ?1 AND occurred_at > ?2",
            params![connector, cutoff],
            |r| r.get(0),
        )
        .map_err(StoreError::from)?;
    Ok(count as u32)
}

/// Claim the in-flight message slot for one conversation (AD-102). Returns
/// [`StoreError::ConversationInFlight`] if the conversation already has a
/// message being processed. Atomic: the `(owner, conversation)` primary key
/// makes the acquisition a single conditional insert, so two concurrent
/// messages cannot both claim the same conversation.
pub fn claim_conversation_in_flight(
    store: &Store,
    owner: &str,
    conversation: &str,
    grant_id: Ulid,
) -> Result<(), StoreError> {
    let mut conn = store.conn.lock();
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(StoreError::from)?;
    match tx.execute(
        "INSERT INTO conversation_in_flight (owner, conversation, grant_id, claimed_at) \
         VALUES (?1, ?2, ?3, ?4)",
        params![
            owner,
            conversation,
            grant_id.to_string(),
            jiff::Timestamp::now().to_string()
        ],
    ) {
        Ok(_) => {
            tx.commit().map_err(StoreError::from)?;
            Ok(())
        }
        Err(rusqlite::Error::SqliteFailure(_, Some(msg)))
            if msg.contains("UNIQUE constraint failed") =>
        {
            Err(StoreError::ConversationInFlight(conversation.to_string()))
        }
        Err(e) => Err(StoreError::from(e)),
    }
}

/// Release the in-flight message slot for a conversation (AD-102). Idempotent:
/// releasing a conversation that is not currently claimed is a no-op.
#[cfg(test)]
pub fn release_conversation_in_flight(
    store: &Store,
    owner: &str,
    conversation: &str,
) -> Result<(), StoreError> {
    let conn = store.conn.lock();
    conn.execute(
        "DELETE FROM conversation_in_flight WHERE owner = ?1 AND conversation = ?2",
        params![owner, conversation],
    )
    .map_err(StoreError::from)?;
    Ok(())
}
/// Release only the claim held by `grant_id`. Handler cleanup uses this
/// grant-id-guarded form so a newer message cannot be cleared after a failure
/// reclaimed and another worker acquired the same conversation.
pub fn release_conversation_in_flight_for_grant(
    store: &Store,
    owner: &str,
    conversation: &str,
    grant_id: Ulid,
) -> Result<(), StoreError> {
    let conn = store.conn.lock();
    conn.execute(
        "DELETE FROM conversation_in_flight \
         WHERE owner = ?1 AND conversation = ?2 AND grant_id = ?3",
        params![owner, conversation, grant_id.to_string()],
    )
    .map_err(StoreError::from)?;
    Ok(())
}

pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS connector_restart_ledger (
            id TEXT PRIMARY KEY,
            connector TEXT NOT NULL,
            occurred_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_connector_restart
            ON connector_restart_ledger (connector, occurred_at);
        CREATE TABLE IF NOT EXISTS conversation_in_flight (
            owner TEXT NOT NULL,
            conversation TEXT NOT NULL,
            grant_id TEXT NOT NULL,
            claimed_at TEXT NOT NULL,
            PRIMARY KEY (owner, conversation)
        );",
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "worker_supervision_tests.rs"]
mod worker_supervision_tests;

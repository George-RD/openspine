// openspine:allow-large-module reason: store lifecycle and transaction wiring share one schema boundary
//! SQLite storage (build plan 4a): task grants, the hash-chained audit log,
//! approvals, selection tokens, and per-task conversation history.
//!
//! Rows store each schema object's own JSON serialization in a `TEXT`
//! column (the schemas crate's `deny_unknown_fields` JSON *is* the
//! validation layer, per D-028 — there is no separate table-per-field
//! mapping to keep in sync). A handful of columns are extracted for
//! indexed lookups (`task_token`, `action_request_id`, …).
//!
//! `rusqlite` is synchronous; [`Store`] serializes access behind a
//! `parking_lot::Mutex` rather than pulling in an async SQLite driver —
//! this kernel serves one owner at a time, so lock contention is not a
//! concern, and every method here does a single small, fast query.
//!
//! No migration mechanism existed prior to AD-139 — `CREATE TABLE IF NOT EXISTS`
//! only ever ran against a fresh file. Every deploy target was dev-only.
//! The day-2 operations contract (AD-139) introduced versioned migrations
//! (`PRAGMA user_version`) with a documented downgrade path, upgrading the
//! ad-hoc lane once a destructive schema change is first needed.

mod pending_draft;
use std::path::Path;
use std::sync::atomic::AtomicBool;
#[cfg(test)]
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::artifact_store::ArtifactStoreError;
use jiff::Timestamp;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use openspine_schemas::grant::TaskGrant;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use ulid::Ulid;

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS task_grants (
    id TEXT PRIMARY KEY,
    -- D-047: sha256:<hex> hash of the bearer token, never the plaintext.
    task_token TEXT NOT NULL UNIQUE,
    expires_at TEXT NOT NULL,
    grant_json TEXT NOT NULL,
    pending_message_digest TEXT NOT NULL,
    bound_chat_id INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS grant_counters (
    grant_id TEXT PRIMARY KEY,
    artifact_puts INTEGER NOT NULL DEFAULT 0,
    model_calls INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS audit_log (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    ts TEXT NOT NULL,
    kind TEXT NOT NULL,
    prev_hash TEXT NOT NULL,
    hash TEXT NOT NULL,
    meta_json TEXT NOT NULL,
    event_json TEXT NOT NULL,
    -- AD-105: per-aggregate bus coordinates (default for brand-new DBs).
    -- Index is created in migrations AFTER add-column for legacy DBs, so
    -- SCHEMA_SQL never references columns an existing table may lack.
    aggregate_id TEXT NOT NULL DEFAULT 'system',
    aggregate_seq INTEGER NOT NULL DEFAULT 0
);
-- consumer_checkpoints also created in migrations for legacy DBs; listed
-- here so brand-new files get the table even if migrations are skipped in tests.
CREATE TABLE IF NOT EXISTS consumer_checkpoints (
    consumer_id TEXT PRIMARY KEY,
    last_acked_global_seq INTEGER NOT NULL DEFAULT 0,
    checkpoint_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS approvals (
    id TEXT PRIMARY KEY,
    action_request_id TEXT NOT NULL,
    approval_json TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_approvals_action_request
    ON approvals (action_request_id);
CREATE TABLE IF NOT EXISTS selection_tokens (
    id TEXT PRIMARY KEY,
    used INTEGER NOT NULL DEFAULT 0,
    token_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS skill_context_selections (
    id TEXT PRIMARY KEY,
    task_grant_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    pack_id TEXT NOT NULL,
    skill_id TEXT NOT NULL,
    skill_version INTEGER NOT NULL,
    task_class TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    used INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_skill_context_grant
    ON skill_context_selections (task_grant_id, used, expires_at);
CREATE TABLE IF NOT EXISTS action_requests (
    id TEXT PRIMARY KEY,
    request_json TEXT NOT NULL,
    used INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS conversation_state (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    task_grant_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content_digest TEXT NOT NULL,
    ts TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_conversation_task_grant
    ON conversation_state (task_grant_id, seq);
CREATE TABLE IF NOT EXISTS kv_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS principals (
    id TEXT PRIMARY KEY,
    identity_id TEXT NOT NULL,
    is_owner INTEGER NOT NULL,
    principal_json TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_principal_owner_singleton
    ON principals (is_owner) WHERE is_owner = 1;
CREATE TABLE IF NOT EXISTS identities (
    id TEXT PRIMARY KEY,
    identity_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS identity_identifiers (
    value_hash TEXT NOT NULL,
    identifier_kind TEXT NOT NULL,
    identity_id TEXT NOT NULL,
    PRIMARY KEY (value_hash, identifier_kind)
);
CREATE INDEX IF NOT EXISTS idx_identity_identifiers_hash
    ON identity_identifiers (value_hash);
"#;

pub(super) fn genesis_digest() -> Digest {
    Digest::parse(format!("sha256:{}", "0".repeat(64)))
        .expect("64 zero hex chars is always a well-formed sha256 digest")
}

#[derive(Clone)]
pub struct Store {
    pub(crate) conn: Arc<Mutex<Connection>>,
    #[cfg(test)]
    activation_tx_failure: Arc<AtomicBool>,
    #[cfg(test)]
    fault_init_tx: Arc<std::sync::Mutex<bool>>,
    #[cfg(test)]
    fail_next_skill_promotion_tx: Arc<AtomicBool>,
    pub(crate) fail_next_owner_reconfirmation: Arc<AtomicBool>,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("stored digest {0} failed to parse")]
    BadDigest(String),
    #[error("stored ulid {0} failed to parse")]
    BadUlid(String),
    #[error("proposed artifact lifecycle error: {0}")]
    ProposedArtifactLifecycle(String),
    #[error("unauthorized owner assertion: {0}")]
    NotOwner(String),
    #[error("inconsistent artifact lineage: {0}")]
    InconsistentLineage(String),
    #[error("timestamp out of representable range: {0}")]
    TimestampRange(String),
    #[error("invalid audit kind: {0}")]
    BadAuditKind(String),
    #[error("bad audit ledger metadata: {0}")]
    BadLedgerMeta(String),
    #[error("consumer checkpoint filter mismatch for {0}")]
    CheckpointFilterMismatch(String),
    #[error("consumer checkpoint regression for {0}")]
    CheckpointRegression(String),
    #[error("clock regression: {0}")]
    ClockRegression(String),
    #[error("numeric ledger value out of SQLite range")]
    NumericRange,
    #[error("task grant not found for escalation: {0}")]
    TaskGrantNotFound(Ulid),
    #[error("mandatory owner notification failed: {0}")]
    OwnerNotificationFailed(String),
    #[error("failure-routing invariant violation: {0}")]
    FailureRouting(String),
    #[error("worker dispatch not found")]
    WorkerDispatchNotFound,
    #[error("worker result already recorded (receipt-keyed idempotency)")]
    WorkerResultAlreadyRecorded,
    #[error("artifact store error during failure surfacing: {0}")]
    ArtifactStore(#[source] ArtifactStoreError),
    #[error("audit ledger chain failed verification")]
    LedgerCorrupted,
    #[error("workflow timer unknown or never scheduled: {0}")]
    WorkflowTimerUnknown(String),
    #[error("unsupported database schema version {current} (latest supported is {latest})")]
    UnsupportedVersion { current: i64, latest: i64 },
    #[error("learned artifact error: {0}")]
    LearnedArtifact(String),
    #[error("invalid task timer schedule: {0}")]
    InvalidTaskTimerSchedule(String),
    #[error("prepopulated task timer id: {0}")]
    PrepopulatedTimerId(String),
    #[error("unsupported task schema version: {0}")]
    UnsupportedTaskSchemaVersion(u32),
    #[error("skill not found: {0}")]
    SkillNotFound(String),
    #[error("skill lifecycle violation: {0}")]
    SkillLifecycle(String),
    #[error("skill review/promotion digest mismatch: {0}")]
    SkillDigestMismatch(String),
    #[error("skill provenance mismatch: {0}")]
    SkillProvenanceMismatch(String),
    #[error("unsupported skill schema version: {0}")]
    UnsupportedSkillSchemaVersion(u32),
    #[error("task not found: {0}")]
    TaskNotFound(Ulid),
}
impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrations::apply_versioned_migrations(&mut conn)?;
        nerve::ensure_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            #[cfg(test)]
            activation_tx_failure: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            fault_init_tx: Arc::new(std::sync::Mutex::new(false)),
            #[cfg(test)]
            fail_next_skill_promotion_tx: Arc::new(AtomicBool::new(false)),
            fail_next_owner_reconfirmation: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        let mut conn = Connection::open_in_memory()?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrations::apply_versioned_migrations(&mut conn)?;
        nerve::ensure_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            #[cfg(test)]
            activation_tx_failure: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            fault_init_tx: Arc::new(std::sync::Mutex::new(false)),
            #[cfg(test)]
            fail_next_skill_promotion_tx: Arc::new(AtomicBool::new(false)),
            fail_next_owner_reconfirmation: Arc::new(AtomicBool::new(false)),
        })
    }
    #[cfg(test)]
    pub(crate) fn fail_next_activation_tx_for_test(&self) {
        self.activation_tx_failure
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Test-only: force the next `promote_skill` transaction to fail after
    /// its AD-110 verdict has already been durably recorded (in a prior,
    /// separately-committed transaction), proving verdict-before-effect: a
    /// failure here must leave the skill `PendingReview`, never `Installed`.
    #[cfg(test)]
    pub(crate) fn fail_next_skill_promotion_tx_for_test(&self) {
        self.fail_next_skill_promotion_tx
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    // ---- task grants ----------------------------------------------------

    /// `pending_message_ref` points at the encrypted, content-addressed
    /// blob (via [`crate::artifact_store::ArtifactStore`]) holding the
    /// owner's original message text for this task — never stored as
    /// plaintext here, and never passed to the shell via argv/env (which a
    /// host operator can read back via `ps`/`docker inspect`); the shell
    /// fetches it in-process over the authenticated `GET /v1/task` call.
    ///
    /// `bound_chat_id` is the Telegram chat this grant's replies must go
    /// to — the reply dispatcher (Step 4's `telegram.reply:owner_channel`
    /// handler) checks every outgoing reply's target chat against this
    /// before ever calling the connector, denying with
    /// `ChannelBindingViolation` on any mismatch (spec.md).
    pub fn insert_task_grant(
        &self,
        grant: &TaskGrant,
        pending_message_ref: &ArtifactRef,
        bound_chat_id: i64,
    ) -> Result<(), StoreError> {
        // D-047: sweep grants that expired well over a day ago before
        // inserting the new one — no separate scheduled job exists yet, so
        // every new grant is itself a sweep trigger.
        self.sweep_expired_grants(Timestamp::now())?;
        // D-047: never persist the plaintext bearer token — the column
        // stores its hash, and the embedded copy inside `grant_json` is
        // blanked so the raw token cannot be recovered from either place.
        let mut redacted = grant.clone();
        redacted.task_token = String::new();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO task_grants (id, task_token, expires_at, grant_json, pending_message_digest, bound_chat_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                grant.id.to_string(),
                budget_support::hash_task_token(&grant.task_token),
                grant.expires_at.to_string(),
                serde_json::to_string(&redacted)?,
                pending_message_ref.digest.as_str(),
                bound_chat_id,
            ],
        )?;
        Ok(())
    }

    pub fn find_task_grant_by_token(
        &self,
        token: &str,
    ) -> Result<Option<(TaskGrant, ArtifactRef, i64)>, StoreError> {
        let conn = self.conn.lock();
        let row: Option<(String, String, i64)> = conn
            .query_row(
                "SELECT grant_json, pending_message_digest, bound_chat_id FROM task_grants WHERE task_token = ?1",
                params![budget_support::hash_task_token(token)],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((grant_json, digest, bound_chat_id)) = row else {
            return Ok(None);
        };
        let grant: TaskGrant = serde_json::from_str(&grant_json)?;
        let digest = Digest::parse(digest)
            .map_err(|_| StoreError::BadDigest("pending_message_digest".into()))?;
        Ok(Some((
            grant,
            ArtifactRef {
                digest,
                schema_version: 1,
            },
            bound_chat_id,
        )))
    }

    /// Backs D-044's approved-draft dispatch: the `callback_query` handler
    /// has a `task_grant_id` (from the persisted [`ActionRequest`]), not a
    /// `task_token` — the shell that originally requested the preview is
    /// long gone by the time the owner taps approve.
    pub fn find_task_grant_by_id(
        &self,
        id: Ulid,
    ) -> Result<Option<(TaskGrant, ArtifactRef, i64)>, StoreError> {
        let conn = self.conn.lock();
        let row: Option<(String, String, i64)> = conn
            .query_row(
                "SELECT grant_json, pending_message_digest, bound_chat_id FROM task_grants WHERE id = ?1",
                params![id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((grant_json, digest, bound_chat_id)) = row else {
            return Ok(None);
        };
        let grant: TaskGrant = serde_json::from_str(&grant_json)?;
        let digest = Digest::parse(digest)
            .map_err(|_| StoreError::BadDigest("pending_message_digest".into()))?;
        Ok(Some((
            grant,
            ArtifactRef {
                digest,
                schema_version: 1,
            },
            bound_chat_id,
        )))
    }

    #[cfg(test)]
    pub fn count_task_grants(&self) -> Result<usize, StoreError> {
        let conn = self.conn.lock();
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM task_grants", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    #[cfg(test)]
    pub fn count_audit_events_of_kind(&self, kind: &str) -> Result<usize, StoreError> {
        let conn = self.conn.lock();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM audit_log WHERE kind = ?1",
            params![kind],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    #[cfg(test)]
    pub fn all_audit_event_jsons(&self) -> Result<Vec<String>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT event_json FROM audit_log ORDER BY seq")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // ---- audit log --------------------------------------------------------

    // ---- conversation state ----------------------------------------------

    pub fn append_conversation_message(
        &self,
        task_grant_id: Ulid,
        role: &str,
        content_digest: &Digest,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO conversation_state (task_grant_id, role, content_digest, ts) VALUES (?1, ?2, ?3, ?4)",
            params![
                task_grant_id.to_string(),
                role,
                content_digest.as_str(),
                Timestamp::now().to_string(),
            ],
        )?;
        Ok(())
    }

    /// Count conversation turns attached to owner-control grants only.
    /// `conversation_state` has no lane column, so provenance comes from
    /// the persisted, verified `TaskGrant.workflow_id` in `task_grants`.
    pub fn count_owner_control_conversation_turns(&self) -> Result<usize, StoreError> {
        let conn = self.conn.lock();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM conversation_state AS c
             JOIN task_grants AS g ON g.id = c.task_grant_id
             WHERE json_extract(g.grant_json, '$.workflow_id') = ?1",
            params!["owner_control_conversation"],
            |row| row.get(0),
        )?;
        usize::try_from(count).map_err(|_| StoreError::NumericRange)
    }

    /// The most recent `limit` messages for `task_grant_id`, oldest first.
    pub fn recent_conversation(
        &self,
        task_grant_id: Ulid,
        limit: usize,
    ) -> Result<Vec<(String, Digest)>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT role, content_digest FROM conversation_state
             WHERE task_grant_id = ?1 ORDER BY seq DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![task_grant_id.to_string(), limit as i64], |row| {
            let role: String = row.get(0)?;
            let digest: String = row.get(1)?;
            Ok((role, digest))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (role, digest) = row?;
            let digest = Digest::parse(digest)
                .map_err(|_| StoreError::BadDigest("content_digest".into()))?;
            out.push((role, digest));
        }
        out.reverse();
        Ok(out)
    }

    // ---- simple key/value (e.g. last Telegram update_id) ----------------

    pub fn get_kv(&self, key: &str) -> Result<Option<String>, StoreError> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row(
                "SELECT value FROM kv_state WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn set_kv(&self, key: &str, value: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO kv_state (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
    pub fn delete_kv(&self, key: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM kv_state WHERE key = ?1", params![key])?;
        Ok(())
    }
    /// First-boot: persist `telegram.bot_id` and migrate the legacy un-namespaced
    /// offset into `last_telegram_update_id.<bot_id>` in one transaction.
    pub fn initialize_telegram_bot_id_and_migrate_offset(
        &self,
        bot_id: i64,
    ) -> Result<(), StoreError> {
        self.reconcile_telegram_bot_id_tx(bot_id, true)
    }

    /// Switch the persisted identity to `bot_id` into a FRESH namespace, never
    /// inheriting a prior bot's offset — startup recovery when the vault token's
    /// actual id differs from the persisted one (mid-rotation crash: vault on B,
    /// SQLite on A; must not poll B under A's offset).
    pub fn reconcile_telegram_bot_id_to_actual(&self, bot_id: i64) -> Result<(), StoreError> {
        self.reconcile_telegram_bot_id_tx(bot_id, false)
    }

    /// Shared transactional body: set `telegram.bot_id`, optionally migrate the
    /// legacy offset into the new namespace (first boot only — `migrate_legacy`
    /// false means a changed identity starts fresh), and always clear any
    /// un-namespaced legacy offset.
    fn reconcile_telegram_bot_id_tx(
        &self,
        bot_id: i64,
        migrate_legacy: bool,
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let namespaced_key = format!("last_telegram_update_id.{bot_id}");
        tx.execute(
            "INSERT INTO kv_state (key, value) VALUES ('telegram.bot_id', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![bot_id.to_string()],
        )?;
        if migrate_legacy {
            let current: Option<String> = tx
                .query_row(
                    "SELECT value FROM kv_state WHERE key = ?1",
                    params![namespaced_key],
                    |row| row.get(0),
                )
                .optional()?;
            if current.is_none() {
                if let Some(legacy) = tx
                    .query_row(
                        "SELECT value FROM kv_state WHERE key = 'last_telegram_update_id'",
                        [],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?
                {
                    tx.execute(
                        "INSERT INTO kv_state (key, value) VALUES (?1, ?2)",
                        params![namespaced_key, legacy],
                    )?;
                }
            }
        } else {
            // Changed identity: clear any stale `last_telegram_update_id.<bot_id>`
            // (e.g. B→A→B) in the same tx so the real bot starts low, not under
            // an old offset.
            tx.execute(
                "DELETE FROM kv_state WHERE key = ?1",
                params![namespaced_key],
            )?;
        }
        #[cfg(test)]
        {
            // Test-only fault: fire once, after the bot-id + (optional)
            // namespaced legacy-offset writes have landed inside the
            // transaction, so a rollback demonstrably discards those partial
            // mutations. Consumed on fire so a retry re-attempts cleanly.
            let mut guard = self.fault_init_tx.lock().expect("fault_init_tx poisoned");
            if *guard {
                *guard = false;
                return Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
            }
        }
        tx.execute(
            "DELETE FROM kv_state WHERE key = 'last_telegram_update_id'",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }
}

pub(crate) mod activation;
mod audit_support;
pub(crate) mod boot_clock;
pub(crate) use boot_clock::BootClockCheck;
pub(crate) mod briefcase_support;
#[cfg(test)]
mod briefcase_support_tests;
mod budget_support;
#[cfg(test)]
mod budget_support_tests;
#[cfg(test)]
mod day2_tests;
mod digest_store;
pub(crate) mod eval_verdict_store;
#[cfg(test)]
mod eval_verdict_store_tests;
pub(crate) mod event_bus;
mod failure_surfacing;
pub(crate) mod failure_surfacing_types;
mod gate_support;
mod identity;
#[cfg(test)]
mod identity_tests;
pub(crate) mod learned_artifacts;
pub(crate) mod learned_reconfirmation;
#[cfg(test)]
mod lineage_tests;
#[cfg(test)]
mod migration_tests;
mod migrations;
pub(crate) mod nerve;
pub(crate) mod nerve_dispatch;
pub(crate) mod nerve_reactions;
pub(crate) mod personality_seed;
pub(crate) mod proposed_artifacts;
pub(crate) mod skill_preview_records;
pub(crate) mod skill_promotion_decisions;
pub(crate) mod skill_read_queries;
pub(crate) mod skill_store;
pub(crate) mod spend;
pub(crate) mod task_board;
pub(crate) mod task_dispatch;
#[cfg(test)]
mod test_hooks;
#[cfg(test)]
mod tests;
pub(crate) mod worker_dispatch;
pub(crate) mod worker_result_relay;
pub(crate) mod workflow_timers;

impl Store {
    /// Test-only: force the next `commit_owner_reconfirmation` call to fail
    /// its durable transaction, exercising the registry-in-unchanged +
    /// request-retryable + no-success-audit contract. No production caller
    /// may ever set this.
    #[cfg(test)]
    pub fn set_fail_next_owner_reconfirmation(&self, fail: bool) {
        self.fail_next_owner_reconfirmation
            .store(fail, Ordering::SeqCst);
    }
    /// Replace the short-lived synthetic reconfirm grant in place on owner
    /// retry. The action request id remains unchanged, preserving its
    /// single-use and digest binding while refreshing only grant authority.
    pub fn refresh_task_grant(&self, grant: &TaskGrant) -> Result<(), StoreError> {
        let mut redacted = grant.clone();
        redacted.task_token.clear();
        let conn = self.conn.lock();
        let changed = conn.execute(
            "UPDATE task_grants SET task_token = ?1, expires_at = ?2, grant_json = ?3 WHERE id = ?4",
            params![
                budget_support::hash_task_token(&grant.task_token),
                grant.expires_at.to_string(),
                serde_json::to_string(&redacted)?,
                grant.id.to_string(),
            ],
        )?;
        if changed != 1 {
            return Err(StoreError::LearnedArtifact(
                "synthetic reconfirm grant disappeared during refresh".into(),
            ));
        }
        Ok(())
    }
}

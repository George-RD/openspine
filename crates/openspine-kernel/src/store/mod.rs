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
//! No migration mechanism (`PRAGMA user_version` + `ALTER TABLE`) exists
//! yet — `CREATE TABLE IF NOT EXISTS` only ever runs against a fresh file.
//! Acceptable while every deploy target is dev-only (D-020); the first
//! schema change that must survive an existing on-disk `data/kernel.db`
//! needs one added before it ships.

use std::path::Path;

use jiff::Timestamp;
use openspine_schemas::action::{ActionId, GateDecision};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::audit::AuditEvent;
use openspine_schemas::digest::{canonical_json, Digest};
use openspine_schemas::grant::TaskGrant;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest as _, Sha256};
use ulid::Ulid;

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS task_grants (
    id TEXT PRIMARY KEY,
    task_token TEXT NOT NULL UNIQUE,
    expires_at TEXT NOT NULL,
    grant_json TEXT NOT NULL,
    pending_message_digest TEXT NOT NULL,
    bound_chat_id INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS audit_log (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    ts TEXT NOT NULL,
    kind TEXT NOT NULL,
    prev_hash TEXT NOT NULL,
    hash TEXT NOT NULL,
    meta_json TEXT NOT NULL,
    event_json TEXT NOT NULL
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
"#;

fn genesis_digest() -> Digest {
    Digest::parse(format!("sha256:{}", "0".repeat(64)))
        .expect("64 zero hex chars is always a well-formed sha256 digest")
}

pub struct Store {
    conn: Mutex<Connection>,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("stored digest {0} failed to parse")]
    BadDigest(String),
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_SQL)?;
        Self::apply_ad_hoc_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA_SQL)?;
        Self::apply_ad_hoc_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Ad-hoc, no-`PRAGMA user_version` migrations for schema changes made
    /// after a `data/kernel.db` may already exist on disk (see the module
    /// doc comment: `CREATE TABLE IF NOT EXISTS` alone only ever helps a
    /// fresh file). Each statement here must be safe to run against both a
    /// brand-new database (where `SCHEMA_SQL` already created the column,
    /// so this is a harmless no-op) and an old one predating the column —
    /// SQLite's "duplicate column name" failure on the former case is
    /// swallowed; any other error still propagates.
    fn apply_ad_hoc_migrations(conn: &Connection) -> Result<(), StoreError> {
        // D-040 follow-up: `action_requests.used` backs
        // `try_consume_action_request`'s single-approval guard, added
        // after this table first shipped.
        match conn.execute(
            "ALTER TABLE action_requests ADD COLUMN used INTEGER NOT NULL DEFAULT 0",
            [],
        ) {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(_, Some(msg)))
                if msg.contains("duplicate column name") =>
            {
                Ok(())
            }
            Err(err) => Err(err.into()),
        }
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
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO task_grants (id, task_token, expires_at, grant_json, pending_message_digest, bound_chat_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                grant.id.to_string(),
                grant.task_token,
                grant.expires_at.to_string(),
                serde_json::to_string(grant)?,
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
                params![token],
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

    fn last_hash(conn: &Connection) -> Result<Digest, StoreError> {
        let hash: Option<String> = conn
            .query_row(
                "SELECT hash FROM audit_log ORDER BY seq DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        match hash {
            Some(h) => Digest::parse(h.clone()).map_err(|_| StoreError::BadDigest(h)),
            None => Ok(genesis_digest()),
        }
    }

    /// Append one audit row, chaining it to the previous hash. Never
    /// mutates or removes an existing row (append-only, PRD §18). `id` and
    /// `ts` are folded into the hashed pre-image (not just stored
    /// alongside it) so neither can be silently rewritten without breaking
    /// [`Self::verify_audit_chain`].
    #[allow(clippy::too_many_arguments)]
    pub fn append_audit(
        &self,
        kind: &str,
        action: Option<&ActionId>,
        decision: Option<&GateDecision>,
        reason: Option<&str>,
        task_grant_id: Option<Ulid>,
        target_refs: &[ArtifactRef],
        payload_refs: &[ArtifactRef],
    ) -> Result<AuditEvent, StoreError> {
        let conn = self.conn.lock();
        let prev_hash = Self::last_hash(&conn)?;

        let id = Ulid::new();
        let ts = Timestamp::now();
        let meta = serde_json::json!({
            "id": id.to_string(),
            "ts": ts.to_string(),
            "kind": kind,
            "action": action,
            "decision": decision,
            "reason": reason,
            "task_grant_id": task_grant_id.map(|u| u.to_string()),
            "target_refs": target_refs,
            "payload_refs": payload_refs,
        });
        let canonical = canonical_json(&meta);

        let mut hasher = Sha256::new();
        hasher.update(prev_hash.as_str().as_bytes());
        hasher.update(canonical.as_bytes());
        let hash_hex: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let hash = Digest::parse(format!("sha256:{hash_hex}"))
            .expect("sha256 hex digest is always well-formed");

        let event = AuditEvent {
            id,
            schema_version: 1,
            ts,
            kind: kind.to_string(),
            action: action.cloned(),
            decision: decision.cloned(),
            reason: reason.map(str::to_string),
            task_grant_id,
            target_refs: target_refs.to_vec(),
            payload_refs: payload_refs.to_vec(),
            prev_hash,
            hash,
        };

        conn.execute(
            "INSERT INTO audit_log (id, ts, kind, prev_hash, hash, meta_json, event_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                event.id.to_string(),
                event.ts.to_string(),
                event.kind,
                event.prev_hash.as_str(),
                event.hash.as_str(),
                canonical,
                serde_json::to_string(&event)?,
            ],
        )?;

        Ok(event)
    }

    /// Walk the chain from genesis, recomputing each hash. Returns `Ok(true)`
    /// if every row's stored hash matches, `Ok(false)` at the first break
    /// (a broken chain is not an I/O error — it's the thing this function
    /// exists to detect).
    pub fn verify_audit_chain(&self) -> Result<bool, StoreError> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT prev_hash, hash, meta_json FROM audit_log ORDER BY seq ASC")?;
        let rows = stmt.query_map([], |row| {
            let prev_hash: String = row.get(0)?;
            let hash: String = row.get(1)?;
            let meta_json: String = row.get(2)?;
            Ok((prev_hash, hash, meta_json))
        })?;

        let mut expected_prev = genesis_digest();
        for row in rows {
            let (prev_hash, hash, meta_json) = row?;
            if prev_hash != expected_prev.as_str() {
                return Ok(false);
            }
            let mut hasher = Sha256::new();
            hasher.update(prev_hash.as_bytes());
            hasher.update(meta_json.as_bytes());
            let recomputed: String = hasher
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if format!("sha256:{recomputed}") != hash {
                return Ok(false);
            }
            expected_prev =
                Digest::parse(hash).map_err(|_| StoreError::BadDigest("hash".into()))?;
        }
        Ok(true)
    }

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
}

mod gate_support;
#[cfg(test)]
mod tests;

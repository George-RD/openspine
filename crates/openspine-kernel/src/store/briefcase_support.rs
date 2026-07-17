//! Persistence for kernel-owned briefcases, blackboard state, and the
//! kernel-owned learned-source pool packing draws relevant preferences and
//! skills from.

use jiff::Timestamp;
use openspine_schemas::action::{ActionId, GateDecision};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::briefcase::{Briefcase, LearnedSource, WorkerVisibility};
use openspine_schemas::grant::TaskGrant;
use rusqlite::{params, OptionalExtension};
use ulid::Ulid;

use super::{Store, StoreError};

pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS briefcases (
           task_grant_id TEXT PRIMARY KEY,
           briefcase_json TEXT NOT NULL,
           primary_worker_id TEXT NOT NULL DEFAULT ''
         );
         CREATE TABLE IF NOT EXISTS briefcase_worker_visibility (
           task_grant_id TEXT NOT NULL,
           worker_id TEXT NOT NULL,
           visibility_json TEXT NOT NULL,
           PRIMARY KEY (task_grant_id, worker_id)
         );
         CREATE TABLE IF NOT EXISTS learned_sources (
           key TEXT PRIMARY KEY,
           source_json TEXT NOT NULL
         );",
    )?;
    Ok(())
}

/// Parameters for an audit row to be chained inside
/// [`Store::mutate_briefcase_and_audit`].
pub(crate) struct BriefcaseAudit {
    pub kind: String,
    pub action: Option<ActionId>,
    pub decision: Option<GateDecision>,
    pub reason: Option<String>,
    pub task_grant_id: Option<Ulid>,
    pub target_refs: Vec<ArtifactRef>,
    pub payload_refs: Vec<ArtifactRef>,
}

impl Store {
    #[allow(dead_code)]
    #[cfg(test)]
    pub(crate) fn install_test_briefcase_insert_failure(&self) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute_batch(
            "CREATE TRIGGER abort_briefcase_test BEFORE INSERT ON briefcases BEGIN \
             SELECT RAISE(ABORT, 'injected briefcase write failure'); END;",
        )?;
        Ok(())
    }
    #[allow(dead_code)]
    pub fn insert_briefcase(
        &self,
        task_grant_id: Ulid,
        briefcase: &Briefcase,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO briefcases (task_grant_id, briefcase_json) VALUES (?1, ?2)",
            params![task_grant_id.to_string(), serde_json::to_string(briefcase)?],
        )?;
        Ok(())
    }

    /// Atomically persist a freshly composed task grant together with its
    /// initial kernel briefcase inside a single `BEGIN IMMEDIATE` transaction.
    ///
    /// Either both rows land or neither does: a failure while writing the
    /// briefcase (or the grant) rolls back the whole transaction, so the
    /// `task_grants` table can never hold an orphan grant for which no
    /// briefcase was created, and a briefcase can never outlive its grant
    /// (D-050). This replaces the previous two-step `insert_task_grant` +
    /// `insert_briefcase` sequence at the Grant→Run boundary.
    pub fn insert_grant_and_briefcase_atomic(
        &self,
        grant: &TaskGrant,
        pending_message_ref: &ArtifactRef,
        bound_chat_id: i64,
        briefcase: &Briefcase,
    ) -> Result<(), StoreError> {
        // D-047: sweep grants that expired well over a day ago before
        // inserting the new one — no separate scheduled job exists yet.
        self.sweep_expired_grants(Timestamp::now())?;
        // D-047: never persist the plaintext bearer token — the column
        // stores its hash, and the embedded copy inside `grant_json` is
        // blanked so the raw token cannot be recovered from either place.
        let mut redacted = grant.clone();
        redacted.task_token = String::new();
        let conn = self.conn.lock();
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(StoreError::from)?;
        let result = (|| {
            conn.execute(
                "INSERT INTO task_grants (id, task_token, expires_at, grant_json, pending_message_digest, bound_chat_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    grant.id.to_string(),
                    super::budget_support::hash_task_token(&grant.task_token),
                    grant.expires_at.to_string(),
                    serde_json::to_string(&redacted)?,
                    pending_message_ref.digest.as_str(),
                    bound_chat_id,
                ],
            )?;
            conn.execute(
                "INSERT INTO briefcases (task_grant_id, briefcase_json) VALUES (?1, ?2)",
                params![grant.id.to_string(), serde_json::to_string(briefcase)?],
            )?;
            Ok::<(), StoreError>(())
        })();
        match result {
            Ok(()) => {
                conn.execute_batch("COMMIT").map_err(StoreError::from)?;
                Ok(())
            }
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(err)
            }
        }
    }

    #[allow(dead_code)]
    pub fn find_briefcase(&self, task_grant_id: Ulid) -> Result<Option<Briefcase>, StoreError> {
        let conn = self.conn.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT briefcase_json FROM briefcases WHERE task_grant_id = ?1",
                params![task_grant_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .transpose()
    }

    /// Return the independently minted worker identity for a grant, creating
    /// one when this legacy row has not been assigned yet.
    pub fn primary_worker_id(&self, task_grant_id: Ulid) -> Result<Ulid, StoreError> {
        let conn = self.conn.lock();
        let current: String = conn.query_row(
            "SELECT primary_worker_id FROM briefcases WHERE task_grant_id = ?1",
            params![task_grant_id.to_string()],
            |row| row.get(0),
        )?;
        if let Ok(id) = Ulid::from_string(&current) {
            return Ok(id);
        }
        let id = Ulid::new();
        conn.execute(
            "UPDATE briefcases SET primary_worker_id = ?1
             WHERE task_grant_id = ?2 AND primary_worker_id = ''",
            params![id.to_string(), task_grant_id.to_string()],
        )?;
        let persisted: String = conn.query_row(
            "SELECT primary_worker_id FROM briefcases WHERE task_grant_id = ?1",
            params![task_grant_id.to_string()],
            |row| row.get(0),
        )?;
        Ulid::from_string(&persisted).map_err(|_| StoreError::BadDigest("primary_worker_id".into()))
    }

    #[allow(dead_code)]
    pub fn update_briefcase(
        &self,
        task_grant_id: Ulid,
        briefcase: &Briefcase,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        let changed = conn.execute(
            "UPDATE briefcases SET briefcase_json = ?1 WHERE task_grant_id = ?2",
            params![serde_json::to_string(briefcase)?, task_grant_id.to_string()],
        )?;
        if changed == 0 {
            return Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
        }
        Ok(())
    }

    /// Atomically load, mutate, and persist a blackboard inside a single
    /// `BEGIN IMMEDIATE` transaction. The write lock is acquired before the
    /// read, so two independent connections contending on the same grant
    /// cannot both pass the top-up replay guard and then overwrite each
    /// other; the second blocks until the first commits, then sees the
    /// updated log and fails the replay check. The UPDATE is committed only
    /// when the closure succeeds.
    pub fn mutate_briefcase<R, E, F>(&self, task_grant_id: Ulid, mutate: F) -> Result<R, E>
    where
        E: From<StoreError>,
        F: FnOnce(&mut Briefcase) -> Result<R, E>,
    {
        let conn = self.conn.lock();
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(StoreError::from)
            .map_err(E::from)?;
        let result = (|| {
            let json: String = conn
                .query_row(
                    "SELECT briefcase_json FROM briefcases WHERE task_grant_id = ?1",
                    params![task_grant_id.to_string()],
                    |row| row.get(0),
                )
                .map_err(StoreError::from)?;
            let mut briefcase: Briefcase = serde_json::from_str(&json).map_err(StoreError::from)?;
            let result = mutate(&mut briefcase)?;
            conn.execute(
                "UPDATE briefcases SET briefcase_json = ?1 WHERE task_grant_id = ?2",
                params![
                    serde_json::to_string(&briefcase).map_err(StoreError::from)?,
                    task_grant_id.to_string()
                ],
            )
            .map_err(StoreError::from)?;
            Ok::<R, E>(result)
        })();
        match result {
            Ok(result) => {
                conn.execute_batch("COMMIT")
                    .map_err(StoreError::from)
                    .map_err(E::from)?;
                Ok(result)
            }
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(err)
            }
        }
    }

    /// Atomically mutate a briefcase AND append the audit row that records the
    /// mutation inside a single `BEGIN IMMEDIATE` transaction.
    ///
    /// The closure receives the *live* briefcase loaded under the transaction's
    /// write lock, applies its change, and returns the value to surface plus the
    /// audit parameters. The briefcase UPDATE and the audit INSERT both run on the
    /// same locked connection, so a mutation failure (or an audit failure) rolls
    /// back the whole transaction — there is never a briefcase update without its
    /// audit row, nor an audit row for an update that did not land.
    pub fn mutate_briefcase_and_audit<R, E, F>(
        &self,
        task_grant_id: Ulid,
        mutate: F,
    ) -> Result<R, E>
    where
        E: From<StoreError>,
        F: FnOnce(&mut Briefcase) -> Result<(R, BriefcaseAudit), E>,
    {
        let conn = self.conn.lock();
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(StoreError::from)
            .map_err(E::from)?;
        let result = (|| {
            let json: String = conn
                .query_row(
                    "SELECT briefcase_json FROM briefcases WHERE task_grant_id = ?1",
                    params![task_grant_id.to_string()],
                    |row| row.get(0),
                )
                .map_err(StoreError::from)?;
            let mut briefcase: Briefcase = serde_json::from_str(&json).map_err(StoreError::from)?;
            let (value, audit) = mutate(&mut briefcase)?;
            conn.execute(
                "UPDATE briefcases SET briefcase_json = ?1 WHERE task_grant_id = ?2",
                params![
                    serde_json::to_string(&briefcase).map_err(StoreError::from)?,
                    task_grant_id.to_string()
                ],
            )
            .map_err(StoreError::from)?;
            Self::append_audit_conn(
                &conn,
                &audit.kind,
                audit.action.as_ref(),
                audit.decision.as_ref(),
                audit.reason.as_deref(),
                audit.task_grant_id,
                &audit.target_refs,
                &audit.payload_refs,
            )?;
            Ok::<R, E>(value)
        })();
        match result {
            Ok(value) => {
                conn.execute_batch("COMMIT")
                    .map_err(StoreError::from)
                    .map_err(E::from)?;
                Ok(value)
            }
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(err)
            }
        }
    }
    #[allow(dead_code)]
    pub fn set_worker_visibility(
        &self,
        task_grant_id: Ulid,
        visibility: &WorkerVisibility,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO briefcase_worker_visibility (task_grant_id, worker_id, visibility_json)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(task_grant_id, worker_id) DO UPDATE SET visibility_json = excluded.visibility_json",
            params![
                task_grant_id.to_string(),
                visibility.worker_id.to_string(),
                serde_json::to_string(visibility)?
            ],
        )?;
        Ok(())
    }
    #[allow(dead_code)]
    pub fn worker_visibility(
        &self,
        task_grant_id: Ulid,
        worker_id: Ulid,
    ) -> Result<Option<WorkerVisibility>, StoreError> {
        let conn = self.conn.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT visibility_json FROM briefcase_worker_visibility
                 WHERE task_grant_id = ?1 AND worker_id = ?2",
                params![task_grant_id.to_string(), worker_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .transpose()
    }
    /// Upsert a kernel-owned learned source. The JSON is validated by the
    /// schema type at the API boundary and remains content-addressable by key.
    #[allow(dead_code)]
    pub fn insert_learned_source(&self, source: &LearnedSource) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO learned_sources (key, source_json) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET source_json = excluded.source_json",
            params![source.key, serde_json::to_string(source)?],
        )?;
        Ok(())
    }

    /// Load the stable source snapshot used by the next kernel pack.
    pub fn list_learned_sources(&self) -> Result<Vec<LearnedSource>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT source_json FROM learned_sources ORDER BY key")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let json = row?;
            serde_json::from_str(&json).map_err(StoreError::from)
        })
        .collect()
    }
}

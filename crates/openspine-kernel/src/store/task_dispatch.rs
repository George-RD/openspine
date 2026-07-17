use super::task_board::{parse_ulid, DependencyWake, TimerDispatchRecord, TimerDispatchState};
use super::{Store, StoreError};
use jiff::Timestamp;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::task::{Task, TaskStatus};
use rusqlite::{params, OptionalExtension};
use ulid::Ulid;
impl Store {
    // ---- durable timer dispatch state ----------------------------------

    /// Current durable dispatch record for a finalization key, if any. Drives
    /// idempotency and crash recovery: a `terminal` row means the fired event
    /// (or dependency wake) is fully handled; a `handed_off` row means a grant
    /// was persisted with a recoverable token ref and only the worker handoff
    /// remains; `pending` is transient and only exists inside a single tx.
    #[allow(clippy::type_complexity)]
    pub fn dispatch_state_for_key(
        &self,
        key: &str,
    ) -> Result<Option<TimerDispatchRecord>, StoreError> {
        let conn = self.conn.lock();
        let row: Option<(
            String,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = conn
            .query_row(
                "SELECT timer_id, task_id, state, grant_id, token_ref, terminal_reason \
                 FROM dispatch_state WHERE event_id = ?1",
                params![key],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .optional()?;
        Ok(row.map(
            |(timer_id, task_id, state, grant_id, token_ref, reason)| TimerDispatchRecord {
                event_id: key.to_string(),
                timer_id,
                task_id: task_id.and_then(|s| Ulid::from_string(&s).ok()),
                state: TimerDispatchState::parse(&state).unwrap_or(TimerDispatchState::Pending),
                grant_id: grant_id.and_then(|s| Ulid::from_string(&s).ok()),
                token_ref: token_ref.and_then(|s| serde_json::from_str(&s).ok()),
                terminal_reason: reason,
            },
        ))
    }
    pub fn dispatch_receipt_exists(&self, key: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock();
        let found: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM dispatch_receipts WHERE dispatch_id = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    /// All non-terminal dispatch rows, for startup/live recovery draining.
    pub fn incomplete_timer_dispatches(&self) -> Result<Vec<TimerDispatchRecord>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT event_id, timer_id, task_id, state, grant_id, token_ref, terminal_reason \
             FROM dispatch_state WHERE state IN ('pending', 'handed_off')",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (event_id, timer_id, task_id, state, grant_id, token_ref, reason) = row?;
            out.push(TimerDispatchRecord {
                event_id,
                timer_id,
                task_id: task_id.and_then(|s| Ulid::from_string(&s).ok()),
                state: TimerDispatchState::parse(&state).unwrap_or(TimerDispatchState::Pending),
                grant_id: grant_id.and_then(|s| Ulid::from_string(&s).ok()),
                token_ref: token_ref.and_then(|s| serde_json::from_str(&s).ok()),
                terminal_reason: reason,
            });
        }
        Ok(out)
    }

    /// Persist a freshly composed worker grant together with its recoverable
    /// token ref, and durably mark the dispatch `handed_off` in the SAME
    /// transaction. Callers MUST `ArtifactStore::put` the token ref BEFORE
    /// this call so a crash between blob-write and this commit never loses the
    /// only copy of the worker token.
    #[allow(clippy::too_many_arguments)]
    pub fn persist_grant_with_handoff(
        &self,
        key: &str,
        grant: &TaskGrant,
        pending_message_ref: &ArtifactRef,
        bound_chat_id: i64,
        token_ref: &ArtifactRef,
        timer_id: &str,
        task_id: Option<Ulid>,
    ) -> Result<(), StoreError> {
        self.sweep_expired_grants(Timestamp::now())?;
        let mut redacted = grant.clone();
        redacted.task_token = String::new();
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO task_grants (id, task_token, expires_at, grant_json, pending_message_digest, bound_chat_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                grant.id.to_string(),
                super::budget_support::hash_task_token(&grant.task_token),
                grant.expires_at.to_string(),
                serde_json::to_string(&redacted)?,
                pending_message_ref.digest.as_str(),
                bound_chat_id,
            ],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO dispatch_state \
             (event_id, timer_id, task_id, state, grant_id, token_ref, created_at, updated_at) \
             VALUES (?1, ?2, ?3, 'handed_off', ?4, ?5, ?6, ?6)",
            params![
                key,
                timer_id,
                task_id.map(|u| u.to_string()),
                grant.id.to_string(),
                serde_json::to_string(token_ref)?,
                Timestamp::now().to_string(),
            ],
        )?;
        Store::append_audit_conn(
            &tx,
            "authority.granted",
            None,
            None,
            None,
            Some(grant.id),
            &[],
            std::slice::from_ref(pending_message_ref),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Transition a handed-off dispatch to `terminal` after the worker
    /// handoff succeeded, recording the idempotency marker. `reason` is
    /// `"handed_off"` for a delivered grant or `"handoff_failed"` if the
    /// worker spawn could not be confirmed.
    pub fn complete_timer_dispatch(
        &self,
        key: &str,
        reason: &str,
        grant_id: &str,
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE dispatch_state SET state='terminal', terminal_reason=?2, updated_at=?3 \
             WHERE event_id=?1",
            params![key, reason, Timestamp::now().to_string()],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO processed_timer_events (event_id, grant_id, recorded_at) \
             VALUES (?1, ?2, ?3)",
            params![key, grant_id, Timestamp::now().to_string()],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO dispatch_receipts (dispatch_id, grant_id, completed_at)
             VALUES (?1, ?2, ?3)",
            params![key, grant_id, Timestamp::now().to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Finalize a non-actionable (or awaiting-dependency) dispatch as
    /// `terminal` without a grant, consuming the fired event so it is never
    /// replayed. `related_id` populates `processed_timer_events.grant_id` and
    /// is the task/event id for skip cases.
    pub fn mark_dispatch_terminal(
        &self,
        key: &str,
        timer_id: &str,
        task_id: Option<Ulid>,
        reason: &str,
        related_id: &str,
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO dispatch_state \
             (event_id, timer_id, task_id, state, terminal_reason, created_at, updated_at) \
             VALUES (?1, ?2, ?3, 'terminal', ?4, ?5, ?5)",
            params![
                key,
                timer_id,
                task_id.map(|u| u.to_string()),
                reason,
                Timestamp::now().to_string(),
            ],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO processed_timer_events (event_id, grant_id, recorded_at) \
             VALUES (?1, ?2, ?3)",
            params![key, related_id, Timestamp::now().to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }

    // ---- dependency waiters ----------------------------------------------

    /// Record a durable `waiting` dependency waiter so a fired timer whose
    /// task has an unmet dependency can be re-enqueued once the dependency
    /// completes. Upserted so a re-fired timer does not duplicate the row.
    pub fn insert_dependency_waiter(
        &self,
        task_id: Ulid,
        owner_principal_id: Ulid,
        dependency_id: Ulid,
        timer_id: &str,
        event_id: &str,
    ) -> Result<(), StoreError> {
        let now = Timestamp::now().to_string();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR REPLACE INTO task_dependency_waiters \
             (task_id, owner_principal_id, dependency_id, timer_id, event_id, state, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, 'waiting', ?6, ?6)",
            params![
                task_id.to_string(),
                owner_principal_id.to_string(),
                dependency_id.to_string(),
                timer_id,
                event_id,
                now,
            ],
        )?;
        Ok(())
    }

    /// Reset all previously-claimed waiters for a `(task_id, timer_id)` back
    /// to `waiting` so a later dependency completion can re-poll them (used
    /// when a wake's dependencies are not yet fully resolved).
    pub fn reset_dependency_waiter(&self, task_id: Ulid, timer_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE task_dependency_waiters SET state='waiting', updated_at=?3 \
             WHERE task_id=?1 AND timer_id=?2",
            params![task_id.to_string(), timer_id, Timestamp::now().to_string()],
        )?;
        Ok(())
    }

    /// Mark every waiter for a `(task_id, timer_id)` `consumed` once its wake
    /// has been delivered (or found permanently non-actionable). Task+timer
    /// scoped so multi-dependency tasks do not leave orphan `waiting` rows
    /// that would regenerate an already-terminal wake.
    pub fn consume_dependency_waiter(
        &self,
        task_id: Ulid,
        timer_id: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE task_dependency_waiters SET state='consumed', updated_at=?3 \
             WHERE task_id=?1 AND timer_id=?2",
            params![task_id.to_string(), timer_id, Timestamp::now().to_string()],
        )?;
        Ok(())
    }

    /// Poll `waiting` dependency rows for a completed dependency, revalidating
    /// that the dependent task's owner still exists and that EVERY one of its
    /// dependencies is `Done`. Returns one wake per waiter that is now fully
    /// unblocked and marks it `ready`; waiters whose task still has open
    /// dependencies (or a missing owner/task) are left `waiting`/`consumed`.
    pub fn poll_dependency_waits(
        &self,
        completed_dependency_id: Ulid,
    ) -> Result<Vec<DependencyWake>, StoreError> {
        let mut conn = self.conn.lock();
        let candidates = {
            let mut stmt = conn.prepare(
                "SELECT task_id, dependency_id, timer_id, owner_principal_id, event_id \
                 FROM task_dependency_waiters WHERE dependency_id = ?1 AND state = 'waiting'",
            )?;
            let rows = stmt.query_map(params![completed_dependency_id.to_string()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let tx = conn.transaction()?;
        let now = Timestamp::now().to_string();
        let mut wakes = Vec::new();
        for (task_id_s, _dep_s, timer_id, owner_s, _event_id) in candidates {
            let task_id = parse_ulid(&task_id_s)?;
            let owner_exists: Option<i64> = tx
                .query_row(
                    "SELECT 1 FROM principals WHERE id = ?1",
                    params![owner_s],
                    |r| r.get(0),
                )
                .optional()?;
            if owner_exists.is_none() {
                tx.execute(
                    "UPDATE task_dependency_waiters SET state='consumed', updated_at=?2 \
                     WHERE task_id=?1 AND timer_id=?3",
                    params![task_id_s, now, timer_id],
                )?;
                continue;
            }
            let task_json: Option<String> = tx
                .query_row(
                    "SELECT task_json FROM task_board WHERE id = ?1",
                    params![task_id_s],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(task_json) = task_json else {
                tx.execute(
                    "UPDATE task_dependency_waiters SET state='consumed', updated_at=?2 \
                     WHERE task_id=?1 AND timer_id=?3",
                    params![task_id_s, now, timer_id],
                )?;
                continue;
            };
            let mut task: Task = serde_json::from_str(&task_json)?;
            if matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled) {
                tx.execute(
                    "UPDATE task_dependency_waiters SET state='consumed', updated_at=?2 \
                     WHERE task_id=?1 AND timer_id=?3",
                    params![task_id_s, now, timer_id],
                )?;
                continue;
            }
            let mut all_done = true;
            for dep in &task.dependencies {
                let dep_json: Option<String> = tx
                    .query_row(
                        "SELECT task_json FROM task_board WHERE id = ?1",
                        params![dep.to_string()],
                        |r| r.get(0),
                    )
                    .optional()?;
                match dep_json.and_then(|j| serde_json::from_str::<Task>(&j).ok()) {
                    Some(t) if t.status == TaskStatus::Done => {}
                    _ => {
                        all_done = false;
                        break;
                    }
                }
            }
            if !all_done {
                continue;
            }
            if task.status == TaskStatus::Blocked {
                task.status = TaskStatus::Open;
                let status_str = serde_json::to_value(task.status)?
                    .as_str()
                    .unwrap()
                    .to_string();
                tx.execute(
                    "UPDATE task_board SET status = ?1, task_json = ?2 WHERE id = ?3",
                    params![status_str, serde_json::to_string(&task)?, task_id_s],
                )?;
            }
            let wake_key = format!("wake:{task_id}:{timer_id}");
            tx.execute(
                "UPDATE task_dependency_waiters SET state='ready', updated_at=?2 \
                 WHERE task_id=?1 AND timer_id=?3",
                params![task_id_s, now, timer_id],
            )?;
            wakes.push(DependencyWake {
                task_id,
                timer_id: timer_id.clone(),
                dependency_id: completed_dependency_id,
                wake_key,
            });
        }
        tx.commit()?;
        Ok(wakes)
    }
    /// Periodically poll every distinct waiting dependency. This keeps
    /// dependency wake durable even when task completion was committed by a
    /// caller that predates `mark_task_done_and_poll`.
    pub fn poll_all_dependency_waits(&self) -> Result<Vec<DependencyWake>, StoreError> {
        let dependency_ids: Vec<Ulid> = {
            let conn = self.conn.lock();
            let mut stmt = conn.prepare(
                "SELECT DISTINCT dependency_id FROM task_dependency_waiters WHERE state = 'waiting'",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            let mut ids = Vec::new();
            for row in rows {
                ids.push(parse_ulid(&row?)?);
            }
            ids
        };
        let mut wakes = Vec::new();
        for dependency_id in dependency_ids {
            wakes.extend(self.poll_dependency_waits(dependency_id)?);
        }
        Ok(wakes)
    }

    /// Wake rows already marked `ready` (crashed before dispatch) for recovery
    /// re-drive.
    pub fn take_ready_wakes(&self) -> Result<Vec<DependencyWake>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT task_id, MIN(dependency_id), timer_id \
             FROM task_dependency_waiters WHERE state = 'ready' \
             GROUP BY task_id, timer_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut wakes = Vec::new();
        for row in rows {
            let (task_id_s, dep_s, timer_id) = row?;
            wakes.push(DependencyWake {
                task_id: parse_ulid(&task_id_s)?,
                timer_id: timer_id.clone(),
                dependency_id: parse_ulid(&dep_s)?,
                wake_key: format!("wake:{task_id_s}:{timer_id}"),
            });
        }
        Ok(wakes)
    }

    /// Mark a task `Done` (if currently open/blocked) and poll any dependency
    /// waiters that were blocked on it, returning the wakes to dispatch.
    #[allow(dead_code)]
    pub fn mark_task_done_and_poll(
        &self,
        task_id: Ulid,
    ) -> Result<Vec<DependencyWake>, StoreError> {
        {
            let mut conn = self.conn.lock();
            let tx = conn.transaction()?;
            let json: Option<String> = tx
                .query_row(
                    "SELECT task_json FROM task_board WHERE id = ?1",
                    params![task_id.to_string()],
                    |r| r.get(0),
                )
                .optional()?;
            let json = json.ok_or_else(|| StoreError::TaskNotFound(task_id))?;
            let mut task: Task = serde_json::from_str(&json)?;
            if !matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled) {
                task.status = TaskStatus::Done;
                let status_str = serde_json::to_value(task.status)?
                    .as_str()
                    .unwrap()
                    .to_string();
                tx.execute(
                    "UPDATE task_board SET status = ?1, task_json = ?2 WHERE id = ?3",
                    params![
                        status_str,
                        serde_json::to_string(&task)?,
                        task_id.to_string()
                    ],
                )?;
                tx.commit()?;
            }
        }
        self.poll_dependency_waits(task_id)
    }
}

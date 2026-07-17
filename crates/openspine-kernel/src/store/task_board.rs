use super::{Store, StoreError};
use jiff::Timestamp;
use openspine_schemas::task::{
    Task, TaskSlice, TaskStatus, TaskTimerKind, CURRENT_TASK_SCHEMA_VERSION,
};
use rusqlite::{params, OptionalExtension};
use ulid::Ulid;
pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_board (
            id TEXT PRIMARY KEY,
            owner_principal_id TEXT NOT NULL,
            status TEXT NOT NULL,
            due_at INTEGER,
            reminder_at INTEGER,
            created_at INTEGER NOT NULL,
            title_ref TEXT NOT NULL,
            provenance_json TEXT NOT NULL,
            due_timer_id TEXT,
            reminder_timer_id TEXT,
            task_json TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS task_timer_links (
            timer_id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            UNIQUE(task_id, kind),
            FOREIGN KEY(task_id) REFERENCES task_board(id) ON DELETE CASCADE,
            CHECK(kind IN ('deadline', 'reminder'))
         );
         CREATE TABLE IF NOT EXISTS processed_timer_events (
            event_id TEXT PRIMARY KEY,
            grant_id TEXT NOT NULL,
            recorded_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS dispatch_state (
            event_id TEXT PRIMARY KEY,
            timer_id TEXT NOT NULL,
            task_id TEXT,
            state TEXT NOT NULL CHECK(state IN ('pending', 'handed_off', 'terminal')),
            grant_id TEXT,
            token_ref TEXT,
            terminal_reason TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS dispatch_receipts (
            dispatch_id TEXT PRIMARY KEY,
            grant_id TEXT NOT NULL,
            completed_at TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_dispatch_state_pending
            ON dispatch_state (state, updated_at);
         CREATE TABLE IF NOT EXISTS task_dependency_waiters (
            task_id TEXT NOT NULL,
            owner_principal_id TEXT NOT NULL,
            dependency_id TEXT NOT NULL,
            timer_id TEXT NOT NULL,
            event_id TEXT NOT NULL,
            state TEXT NOT NULL CHECK(state IN ('waiting', 'ready', 'consumed')),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY(task_id, dependency_id, timer_id),
            FOREIGN KEY(task_id) REFERENCES task_board(id) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS idx_task_dependency_waiters_state
            ON task_dependency_waiters (state, updated_at);
         CREATE INDEX IF NOT EXISTS idx_task_board_due
            ON task_board (status, due_at);
         CREATE INDEX IF NOT EXISTS idx_task_board_blocked
            ON task_board (status, created_at);
         CREATE INDEX IF NOT EXISTS idx_task_board_owner
            ON task_board (owner_principal_id);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_task_board_due_timer
            ON task_board (due_timer_id) WHERE due_timer_id IS NOT NULL;
         CREATE UNIQUE INDEX IF NOT EXISTS idx_task_board_reminder_timer
            ON task_board (reminder_timer_id) WHERE reminder_timer_id IS NOT NULL;",
    )?;
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerDispatchState {
    Pending,
    HandedOff,
    Terminal,
}
impl TimerDispatchState {
    pub(super) fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "pending" => Ok(Self::Pending),
            "handed_off" => Ok(Self::HandedOff),
            "terminal" => Ok(Self::Terminal),
            other => Err(StoreError::FailureRouting(format!(
                "invalid dispatch state {other:?}"
            ))),
        }
    }
}
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TimerDispatchRecord {
    pub event_id: String,
    pub timer_id: String,
    pub task_id: Option<Ulid>,
    pub state: TimerDispatchState,
    pub grant_id: Option<Ulid>,
    pub token_ref: Option<openspine_schemas::artifact::ArtifactRef>,
    pub terminal_reason: Option<String>,
}
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DependencyWake {
    pub task_id: Ulid,
    pub timer_id: String,
    pub dependency_id: Ulid,
    pub wake_key: String,
}
fn timestamp_to_epoch_nanos(timestamp: Timestamp) -> Result<i64, StoreError> {
    i64::try_from(timestamp.as_nanosecond()).map_err(|_| {
        StoreError::TimestampRange("epoch nanoseconds out of SQLite range".to_string())
    })
}
pub(super) fn epoch_nanos_to_timestamp(nanos: i64) -> Result<Timestamp, StoreError> {
    Timestamp::from_nanosecond(i128::from(nanos)).map_err(|err| {
        StoreError::TimestampRange(format!("invalid epoch nanoseconds {nanos}: {err}"))
    })
}
pub(super) fn parse_ulid(s: &str) -> Result<Ulid, StoreError> {
    Ulid::from_string(s).map_err(|_| StoreError::BadDigest(s.to_string()))
}
impl Store {
    #[allow(dead_code)]
    pub fn schedule_task_timer(
        &self,
        timer_id: &str,
        task_id: &str,
        kind: TaskTimerKind,
        fires_at: Timestamp,
    ) -> Result<openspine_schemas::audit::AuditEvent, StoreError> {
        ulid::Ulid::from_string(timer_id)
            .map_err(|_| StoreError::InvalidTaskTimerSchedule("timer_id must be a ULID".into()))?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let json_str: String = tx.query_row(
            "SELECT task_json FROM task_board WHERE id = ?1",
            params![task_id],
            |r| r.get(0),
        )?;
        let mut task: Task = serde_json::from_str(&json_str)?;
        if matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled) {
            return Err(StoreError::InvalidTaskTimerSchedule(format!(
                "task {} is in terminal status {:?}",
                task.id, task.status
            )));
        }
        let kind_str = match kind {
            TaskTimerKind::Deadline => {
                if task.due_at != Some(fires_at) {
                    return Err(StoreError::InvalidTaskTimerSchedule(format!(
                        "fires_at {:?} does not match task due_at {:?}",
                        fires_at, task.due_at
                    )));
                }
                task.due_timer_id = Some(timer_id.to_string());
                tx.execute(
                    "UPDATE task_board SET due_timer_id = ?1, task_json = ?2 WHERE id = ?3",
                    params![timer_id, serde_json::to_string(&task)?, task_id],
                )?;
                "deadline"
            }
            TaskTimerKind::Reminder => {
                if task.reminder_at != Some(fires_at) {
                    return Err(StoreError::InvalidTaskTimerSchedule(format!(
                        "fires_at {:?} does not match task reminder_at {:?}",
                        fires_at, task.reminder_at
                    )));
                }
                task.reminder_timer_id = Some(timer_id.to_string());
                tx.execute(
                    "UPDATE task_board SET reminder_timer_id = ?1, task_json = ?2 WHERE id = ?3",
                    params![timer_id, serde_json::to_string(&task)?, task_id],
                )?;
                "reminder"
            }
        };
        tx.execute(
            "INSERT INTO task_timer_links (timer_id, task_id, kind) VALUES (?1, ?2, ?3)",
            params![timer_id, task_id, kind_str],
        )?;
        let run_id = format!(
            "task:{task_id}:{}",
            serde_json::to_value(kind).unwrap().as_str().unwrap()
        );
        let payload = serde_json::json!({
            "timer_id": timer_id,
            "task_id": task_id,
            "kind": kind,
            "fires_at": fires_at.to_string(),
        });
        let event = Self::append_audit_conn_with_options(
            &tx,
            "workflow.timer_scheduled",
            None,
            None,
            None,
            None,
            &[],
            &[],
            Some(&run_id),
            Some(&serde_json::to_string(&payload)?),
        )?;
        tx.execute(
            "INSERT INTO workflow_timers (timer_id, run_id, fires_at, status, fired_event_id)
             VALUES (?1, ?2, ?3, 'pending', NULL)",
            params![timer_id, run_id, timestamp_to_epoch_nanos(fires_at)?],
        )?;
        tx.commit()?;
        Ok(event)
    }
    #[allow(dead_code)]
    pub fn insert_task(&self, task: &Task) -> Result<(), StoreError> {
        if task.schema_version != CURRENT_TASK_SCHEMA_VERSION {
            return Err(StoreError::UnsupportedTaskSchemaVersion(
                task.schema_version,
            ));
        }
        if task.due_timer_id.is_some() || task.reminder_timer_id.is_some() {
            return Err(StoreError::PrepopulatedTimerId(format!(
                "task {} must not be inserted with pre-populated timer IDs",
                task.id
            )));
        }
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO task_board \
             (id, owner_principal_id, status, due_at, reminder_at, created_at, \
              title_ref, provenance_json, due_timer_id, reminder_timer_id, task_json) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                task.id.to_string(),
                task.owner_principal_id.to_string(),
                serde_json::to_value(task.status)
                    .unwrap()
                    .as_str()
                    .unwrap_or("open"),
                task.due_at.map(timestamp_to_epoch_nanos).transpose()?,
                task.reminder_at.map(timestamp_to_epoch_nanos).transpose()?,
                timestamp_to_epoch_nanos(task.created_at)?,
                serde_json::to_string(&task.title_ref)?,
                serde_json::to_string(&task.provenance)?,
                task.due_timer_id,
                task.reminder_timer_id,
                serde_json::to_string(task)?,
            ],
        )?;
        Ok(())
    }
    #[allow(dead_code)]
    pub fn get_task(&self, id: Ulid) -> Result<Option<Task>, StoreError> {
        let conn = self.conn.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT task_json FROM task_board WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        match json {
            Some(j) => Ok(Some(serde_json::from_str(&j)?)),
            None => Ok(None),
        }
    }
    pub fn task_by_timer_id(&self, timer_id: &str) -> Result<Option<Task>, StoreError> {
        let conn = self.conn.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT t.task_json FROM task_board t \
                 JOIN task_timer_links l ON t.id = l.task_id \
                 WHERE l.timer_id = ?1",
                params![timer_id],
                |row| row.get(0),
            )
            .optional()?;
        match json {
            Some(j) => Ok(Some(serde_json::from_str(&j)?)),
            None => Ok(None),
        }
    }
    pub fn tasks_due_now(
        &self,
        at: Timestamp,
        owner_principal_id: Ulid,
        limit: usize,
    ) -> Result<Vec<TaskSlice>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, status, due_at, title_ref FROM task_board \
             WHERE status = 'open' AND due_at IS NOT NULL AND due_at <= ?1 \
             AND owner_principal_id = ?2 \
             ORDER BY due_at ASC, id ASC LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            params![
                timestamp_to_epoch_nanos(at)?,
                owner_principal_id.to_string(),
                limit as i64
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?;
        rows.map(|r| map_slice_row(r?)).collect()
    }
    pub fn blocked_tasks(
        &self,
        owner_principal_id: Ulid,
        limit: usize,
    ) -> Result<Vec<TaskSlice>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, status, due_at, title_ref FROM task_board \
             WHERE status = 'blocked' AND owner_principal_id = ?1 \
             ORDER BY created_at ASC, id ASC LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            params![owner_principal_id.to_string(), limit as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?;
        rows.map(|r| map_slice_row(r?)).collect()
    }
    pub fn asked_about_tasks(
        &self,
        owner_principal_id: Ulid,
        limit: usize,
    ) -> Result<Vec<TaskSlice>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, status, due_at, title_ref FROM task_board \
             WHERE provenance_json LIKE '{\"kind\":\"asked_about\"%' \
             AND status NOT IN ('done', 'cancelled') \
             AND owner_principal_id = ?1 \
             ORDER BY created_at ASC, id ASC LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            params![owner_principal_id.to_string(), limit as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?;
        rows.map(|r| map_slice_row(r?)).collect()
    }
    #[allow(dead_code)]
    pub fn master_slice(
        &self,
        at: Timestamp,
        owner_principal_id: Ulid,
        limit: usize,
    ) -> Result<Vec<TaskSlice>, StoreError> {
        let mut combined: Vec<TaskSlice> = Vec::with_capacity(limit);
        let mut seen = std::collections::HashSet::new();
        for slice in self
            .tasks_due_now(at, owner_principal_id, limit)?
            .into_iter()
            .chain(self.blocked_tasks(owner_principal_id, limit)?)
            .chain(self.asked_about_tasks(owner_principal_id, limit)?)
        {
            if seen.insert(slice.id) {
                combined.push(slice);
            }
            if combined.len() >= limit {
                break;
            }
        }
        Ok(combined)
    }
    #[allow(dead_code)]
    pub fn timer_event_already_processed(&self, event_id: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock();
        let exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM processed_timer_events WHERE event_id = ?1",
                params![event_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(exists.is_some())
    }
    pub fn master_slice_for_task(
        &self,
        task_id: Ulid,
        now: Timestamp,
        limit: usize,
    ) -> Result<Vec<TaskSlice>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let task = self
            .get_task(task_id)?
            .ok_or_else(|| StoreError::TaskNotFound(task_id))?;
        let owner = task.owner_principal_id;
        let mut combined: Vec<TaskSlice> = Vec::with_capacity(limit);
        let mut seen = std::collections::HashSet::new();
        combined.push(TaskSlice {
            schema_version: 1,
            id: task.id,
            status: task.status,
            due_at: task.due_at,
            title_ref: task.title_ref.clone(),
        });
        seen.insert(task.id);
        if limit == 1 {
            return Ok(combined);
        }
        for slice in self
            .tasks_due_now(now, owner, limit)?
            .into_iter()
            .chain(self.blocked_tasks(owner, limit)?)
            .chain(self.asked_about_tasks(owner, limit)?)
        {
            if seen.insert(slice.id) {
                combined.push(slice);
                if combined.len() >= limit {
                    break;
                }
            }
        }
        Ok(combined)
    }
    pub fn mark_task_blocked(&self, id: Ulid) -> Result<(), StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let json: Option<String> = tx
            .query_row(
                "SELECT task_json FROM task_board WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let json = json.ok_or_else(|| StoreError::TaskNotFound(id))?;
        let mut task: Task = serde_json::from_str(&json)?;
        if task.status == TaskStatus::Blocked {
            return Ok(());
        }
        task.status = TaskStatus::Blocked;
        let status_str = serde_json::to_value(task.status)?
            .as_str()
            .unwrap()
            .to_string();
        tx.execute(
            "UPDATE task_board SET status = ?1, task_json = ?2 WHERE id = ?3",
            params![status_str, serde_json::to_string(&task)?, id.to_string()],
        )?;
        Store::append_audit_conn(
            &tx,
            "task.blocked",
            None,
            None,
            Some(&format!("unmet dependency at timer fire: {id}")),
            None,
            &[],
            &[],
        )?;
        tx.commit()?;
        Ok(())
    }
}
fn map_slice_row(r: (String, String, Option<i64>, String)) -> Result<TaskSlice, StoreError> {
    let (id, status, due_at, title_ref) = r;
    Ok(TaskSlice {
        schema_version: 1,
        id: Ulid::from_string(&id).map_err(|_| StoreError::BadDigest(id))?,
        status: serde_json::from_str(&format!("\"{status}\""))
            .map_err(|_| StoreError::BadDigest(status))?,
        due_at: due_at.map(epoch_nanos_to_timestamp).transpose()?,
        title_ref: serde_json::from_str(&title_ref)?,
    })
}
#[cfg(test)]
#[path = "task_board_recovery_tests.rs"]
mod recovery_tests;
#[cfg(test)]
#[path = "task_board_tests.rs"]
mod tests;

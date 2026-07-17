//! Kernel task-board domain types (AD-090 / AD-131).
//!
//! Tasks and commitments are first-class kernel objects. These structs are
//! the validation layer (D-028): every top-level object uses
//! `deny_unknown_fields`, so an unknown field fails deserialization rather
//! than being silently dropped. The store persists the canonical JSON of a
//! [`Task`] and extracts a handful of columns (status, due time, owner,
//! created time, bounded title ref, provenance kind) for indexed lookups.
//!
//! Task detail NEVER leaves the store into master context: the master agent
//! receives only the bounded [`TaskSlice`] (AD-131).
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::artifact::ArtifactRef;

/// The latest task-record schema version this build understands. Stored
/// `task_json` carrying a newer version is rejected at the validation
/// boundary (D-028) rather than silently misinterpreted.
pub const CURRENT_TASK_SCHEMA_VERSION: u32 = 1;

/// Serde field-level guard: a `schema_version` greater than the current
/// version fails deserialization, so a future-shaped record never loads as a
/// silently-truncated [`Task`].
fn deserialize_schema_version<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = u32::deserialize(deserializer)?;
    if v != CURRENT_TASK_SCHEMA_VERSION {
        return Err(serde::de::Error::custom(format!(
            "unsupported task schema_version {v} (expected {CURRENT_TASK_SCHEMA_VERSION})"
        )));
    }
    Ok(v)
}

/// Validation error for [`WorkerId`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WorkerIdError {
    #[error("worker id is empty")]
    Empty,
    #[error("worker id contains an invalid character")]
    InvalidCharacter,
}

/// Validated identifier of the worker/agent that owns a task (AD-090).
///
/// Mirrors the agent naming (`main_assistant_agent`, `email_reply_drafter`):
/// non-empty, lowercase ASCII alphanumeric plus underscore. Constructed via
/// [`WorkerId::new`] so an arbitrary `String` can never become a worker id.
/// Serde is transparent so canonical JSON remains a plain string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct WorkerId(String);

impl WorkerId {
    /// Construct a worker id. Empty strings or any character outside
    /// `[a-z0-9_]` are rejected.
    pub fn new(s: impl Into<String>) -> Result<Self, WorkerIdError> {
        let s = s.into();
        if s.is_empty() {
            return Err(WorkerIdError::Empty);
        }
        if !s
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(WorkerIdError::InvalidCharacter);
        }
        Ok(WorkerId(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for WorkerId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        WorkerId::new(s).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for WorkerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for WorkerId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for WorkerId {
    type Error = WorkerIdError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        WorkerId::new(s)
    }
}

/// Lifecycle state of a tracked task/commitment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Open,
    Blocked,
    Done,
    Cancelled,
}

/// Which kind of timer a `workflow_timers` row represents for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskTimerKind {
    Deadline,
    Reminder,
}

/// Non-sensitive provenance reference for a task (AD-090). Carries only a
/// content-addressed [`ArtifactRef`] plus a coarse kind (D-012) — never
/// plaintext secrets or sensitive text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskProvenance {
    /// "You asked Tuesday" — the owner requested this.
    AskedAbout {
        reference: ArtifactRef,
        asked_at: Timestamp,
    },
    /// "Promised supplier reply by Friday" — a commitment we made.
    Promised { reference: ArtifactRef },
    /// An external counterparty obligation surfaced through normal routing.
    External { reference: ArtifactRef },
}

/// One tracked task/commitment (AD-090). Serialized canonically and stored
/// as `task_json` in the kernel store; a handful of columns are extracted
/// for indexed queries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    #[serde(deserialize_with = "deserialize_schema_version")]
    pub schema_version: u32,
    pub id: Ulid,
    pub owner_principal_id: Ulid,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owning_worker: Option<WorkerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owning_grant_id: Option<Ulid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reminder_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_timer_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reminder_timer_id: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<Ulid>,
    pub provenance: TaskProvenance,
    /// Bounded title reference (content-addressed ref), never the full task text.
    pub title_ref: ArtifactRef,
    pub created_at: Timestamp,
}

/// A bounded projection row returned to the master agent (AD-131).
///
/// Deliberately carries only enough to triage: identity, status, due time,
/// and a bounded title reference. It NEVER includes dependencies, provenance
/// detail, or the owning grant — the full [`Task`] stays in the store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskSlice {
    #[serde(deserialize_with = "deserialize_schema_version")]
    pub schema_version: u32,
    pub id: Ulid,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_at: Option<Timestamp>,
    pub title_ref: ArtifactRef,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::ArtifactRef;
    use crate::digest::Digest;

    fn ref_of(s: &str) -> ArtifactRef {
        ArtifactRef {
            digest: Digest::parse(format!("sha256:{}", s.repeat(64))).unwrap(),
            schema_version: 1,
        }
    }

    fn task() -> Task {
        Task {
            schema_version: 1,
            id: Ulid::new(),
            owner_principal_id: Ulid::new(),
            status: TaskStatus::Open,
            owning_worker: Some(WorkerId::new("main_assistant_agent").unwrap()),
            owning_grant_id: Some(Ulid::new()),
            due_at: Some(Timestamp::from_second(1_700_000_000).unwrap()),
            reminder_at: Some(Timestamp::from_second(1_700_000_100).unwrap()),
            due_timer_id: Some(Ulid::new().to_string()),
            reminder_timer_id: Some(Ulid::new().to_string()),
            dependencies: vec![],
            provenance: TaskProvenance::AskedAbout {
                reference: ref_of("a"),
                asked_at: Timestamp::from_second(1_690_000_000).unwrap(),
            },
            title_ref: ref_of("b"),
            created_at: Timestamp::from_second(1_690_000_000).unwrap(),
        }
    }

    #[test]
    fn task_round_trips_through_serde() {
        let t = task();
        let json = serde_json::to_string(&t).unwrap();
        let back: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
        assert_eq!(back.owning_grant_id, t.owning_grant_id);
        assert_eq!(back.due_timer_id, t.due_timer_id);
        assert_eq!(back.reminder_timer_id, t.reminder_timer_id);
    }

    #[test]
    fn task_requires_schema_version_and_rejects_unknown_fields() {
        let json = serde_json::to_string(&task()).unwrap();
        assert!(json.contains("schema_version"));
        let mut value = serde_json::from_str::<serde_json::Value>(&json).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("secret".into(), serde_json::json!("nope"));
        assert!(serde_json::from_value::<Task>(value).is_err());
    }

    #[test]
    fn task_status_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_value(TaskStatus::Blocked).unwrap(),
            serde_json::json!("blocked")
        );
        assert_eq!(
            serde_json::to_value(TaskStatus::Cancelled).unwrap(),
            serde_json::json!("cancelled")
        );
    }

    #[test]
    fn provenance_round_trips_with_tag() {
        let p = TaskProvenance::Promised {
            reference: ref_of("c"),
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["kind"], "promised");
        let back: TaskProvenance = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn slice_round_trips_and_rejects_unknown_fields() {
        let s = TaskSlice {
            schema_version: 1,
            id: Ulid::new(),
            status: TaskStatus::Blocked,
            due_at: Some(Timestamp::from_second(1_700_000_000).unwrap()),
            title_ref: ref_of("d"),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: TaskSlice = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        let mut value = serde_json::from_str::<serde_json::Value>(&json).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("owning_grant_id".into(), serde_json::json!("x"));
        assert!(serde_json::from_value::<TaskSlice>(value).is_err());
    }

    #[test]
    fn worker_id_validation_rejects_empty_and_invalid_chars() {
        assert_eq!(WorkerId::new(""), Err(WorkerIdError::Empty));
        assert_eq!(WorkerId::new("Main"), Err(WorkerIdError::InvalidCharacter));
        assert_eq!(
            WorkerId::new("main-assistant"),
            Err(WorkerIdError::InvalidCharacter)
        );
        assert_eq!(
            WorkerId::new("main_assistant_agent").unwrap().as_str(),
            "main_assistant_agent"
        );
    }

    #[test]
    fn task_provenance_rejects_unknown_fields() {
        let json = serde_json::json!({
            "kind": "promised",
            "reference": ref_of("c"),
            "unknown_field": "error"
        });
        assert!(serde_json::from_value::<TaskProvenance>(json).is_err());
    }

    #[test]
    fn task_rejects_plaintext_title_and_provenance_references() {
        let mut value = serde_json::to_value(task()).unwrap();
        let valid_title = value["title_ref"].clone();
        value["title_ref"] = serde_json::json!("sensitive plaintext");
        assert!(serde_json::from_value::<Task>(value.clone()).is_err());
        value["title_ref"] = valid_title;
        value["provenance"]["reference"] = serde_json::json!("sensitive plaintext");
        assert!(serde_json::from_value::<Task>(value).is_err());
    }
    #[test]
    fn task_rejects_unsupported_schema_version() {
        let mut t = task();
        t.schema_version = 2;
        let json = serde_json::to_string(&t).unwrap();
        assert!(serde_json::from_str::<Task>(&json).is_err());

        t.schema_version = 0;
        let json = serde_json::to_string(&t).unwrap();
        assert!(serde_json::from_str::<Task>(&json).is_err());
    }
}

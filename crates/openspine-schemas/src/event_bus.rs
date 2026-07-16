//! Event-bus subscription types (AD-105).
//!
//! The bus **is** the audit ledger: these types describe typed filters and
//! consumer checkpoints over [`crate::audit::AuditEvent`] rows. No parallel
//! event envelope.

use serde::{Deserialize, Serialize};

use crate::audit::AuditKind;

/// Typed filter for ordered ledger replay.
///
/// - `kinds = None` matches every audit kind; `Some(list)` matches any listed
///   [`AuditKind`] (OR).
/// - `aggregate_id = None` matches every aggregate; `Some(id)` matches that
///   aggregate only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventSubscriptionFilter {
    pub schema_version: u32,
    pub kinds: Option<Vec<AuditKind>>,
    pub aggregate_id: Option<String>,
}

impl Default for EventSubscriptionFilter {
    fn default() -> Self {
        Self {
            schema_version: 1,
            kinds: None,
            aggregate_id: None,
        }
    }
}

impl EventSubscriptionFilter {
    /// Unconstrained filter — every ledger row matches.
    pub fn all() -> Self {
        Self::default()
    }

    pub fn kinds(kinds: impl IntoIterator<Item = AuditKind>) -> Self {
        Self {
            schema_version: 1,
            kinds: Some(kinds.into_iter().collect()),
            aggregate_id: None,
        }
    }

    pub fn aggregate(aggregate_id: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            kinds: None,
            aggregate_id: Some(aggregate_id.into()),
        }
    }

    /// True when `kind` / `aggregate_id` satisfy this filter.
    pub fn matches(&self, kind: &AuditKind, aggregate_id: &str) -> bool {
        if let Some(ref kinds) = self.kinds {
            if !kinds.iter().any(|k| k == kind) {
                return false;
            }
        }
        if let Some(ref want) = self.aggregate_id {
            if want != aggregate_id {
                return false;
            }
        }
        true
    }
}

/// Last successfully handled global ledger sequence for a named consumer.
///
/// `last_acked_global_seq = 0` means nothing has been acked yet. The checkpoint
/// advances only after the consumer handler returns success for an event —
/// never at append/publish time (AD-105 idempotent-consumer contract).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumerCheckpoint {
    pub schema_version: u32,
    pub last_acked_global_seq: u64,
}

impl Default for ConsumerCheckpoint {
    fn default() -> Self {
        Self {
            schema_version: 1,
            last_acked_global_seq: 0,
        }
    }
}

impl ConsumerCheckpoint {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_round_trips_through_serde() {
        let filter = EventSubscriptionFilter {
            schema_version: 1,
            kinds: Some(vec![
                AuditKind::from_static("action.gated"),
                AuditKind::from_static("authority.granted"),
            ]),
            aggregate_id: Some("system".into()),
        };
        let json = serde_json::to_string(&filter).unwrap();
        let back: EventSubscriptionFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(filter, back);
        // Transparent kinds serialize as plain strings inside the array.
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["kinds"][0], "action.gated");
    }

    #[test]
    fn filter_defaults_match_all() {
        let err = serde_json::from_str::<EventSubscriptionFilter>("{}").unwrap_err();
        assert!(err.to_string().contains("schema_version"));
        let filter = EventSubscriptionFilter::default();
        assert!(filter.matches(&AuditKind::from_static("anything"), "any-agg"));
    }

    #[test]
    fn filter_kinds_and_aggregate() {
        let filter = EventSubscriptionFilter {
            schema_version: 1,
            kinds: Some(vec![AuditKind::from_static("action.gated")]),
            aggregate_id: Some("system".into()),
        };
        assert!(filter.matches(&AuditKind::from_static("action.gated"), "system"));
        assert!(!filter.matches(&AuditKind::from_static("authority.granted"), "system"));
        assert!(!filter.matches(&AuditKind::from_static("action.gated"), "task_grant:x"));
    }

    #[test]
    fn checkpoint_round_trips_and_defaults() {
        let cp = ConsumerCheckpoint {
            schema_version: 1,
            last_acked_global_seq: 7,
        };
        let json = serde_json::to_string(&cp).unwrap();
        let back: ConsumerCheckpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(cp, back);
        assert_eq!(ConsumerCheckpoint::default().last_acked_global_seq, 0);
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = serde_json::from_str::<EventSubscriptionFilter>(
            r#"{"schema_version":1,"kinds":null,"sneaky":true}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown field"));
        let err = serde_json::from_str::<ConsumerCheckpoint>(
            r#"{"schema_version":1,"last_acked_global_seq":1,"extra":0}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }
}

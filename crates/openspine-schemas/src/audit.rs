//! Audit events (PRD §18) — private payloads are referenced by encrypted
//! artifact ref or digest, never stored as raw plaintext audit text.
//!
//! This is the *persisted* audit row shape (it carries the hash-chain
//! fields per D-025); `openspine-gate`'s in-process `AuditMeta` is a
//! lighter, storage-free struct the kernel converts into an `AuditEvent`
//! when it appends to the chain.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::action::{ActionId, GateDecision};
use crate::artifact::ArtifactRef;
use crate::digest::Digest;

/// Nominal audit-kind identifier (AD-105 / D-013).
///
/// The vocabulary is open — new kinds are added without a schema change —
/// but the type is not a bare `String`: construction rejects empty values,
/// and subscription filters take `AuditKind` rather than free text. Serde
/// is transparent so on-disk/audit JSON remains a plain string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct AuditKind(String);

impl AuditKind {
    /// Construct a kind. Empty strings are rejected.
    pub fn new(s: impl Into<String>) -> Result<Self, AuditKindError> {
        let s = s.into();
        if s.is_empty() {
            return Err(AuditKindError::Empty);
        }
        Ok(AuditKind(s))
    }

    /// Infallible constructor for known non-empty literals.
    pub fn from_static(s: &'static str) -> Self {
        debug_assert!(!s.is_empty(), "AuditKind must be non-empty");
        AuditKind(s.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for AuditKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        AuditKind::new(s).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for AuditKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for AuditKind {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for AuditKind {
    type Error = AuditKindError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        AuditKind::new(s)
    }
}

impl TryFrom<String> for AuditKind {
    type Error = AuditKindError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        AuditKind::new(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuditKindError {
    #[error("audit kind must be non-empty")]
    Empty,
}

/// Default aggregate for grant-less / legacy audit rows.
pub fn default_aggregate_id() -> String {
    "system".to_string()
}

/// One append-only, hash-chained audit log row.
///
/// `kind` is an open vocabulary (e.g. `action.gate_decision`,
/// `telegram.owner.message.ignored`, `artifact.activated`) — new audit kinds
/// are added without a schema change, matching D-013. The field is a nominal
/// [`AuditKind`] so subscription filters are typed, not free-text.
///
/// `aggregate_id` + `aggregate_seq` are the AD-105 bus coordinates: unique
/// event IDs already live in `id`; per-aggregate sequences let idempotent
/// consumers order and deduplicate a single logical stream without a parallel
/// event store.
///
/// **Legacy rows:** pre-AD-105 `event_json` lacks these fields. Serde defaults
/// them to `aggregate_id = "system"` and `aggregate_seq = 0`. Sequence `0` is a
/// **legacy sentinel**, not a newly assigned sequence — the positive,
/// gap-free per-aggregate guarantee applies only to rows written by the
/// post-AD-105 append path (`aggregate_seq >= 1`). Hash-chained historical
/// payloads are never rewritten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditEvent {
    pub id: Ulid,
    pub schema_version: u32,
    pub ts: jiff::Timestamp,
    pub kind: AuditKind,
    pub action: Option<ActionId>,
    pub decision: Option<GateDecision>,
    pub reason: Option<String>,
    pub task_grant_id: Option<Ulid>,
    #[serde(default)]
    pub target_refs: Vec<ArtifactRef>,
    #[serde(default)]
    pub payload_refs: Vec<ArtifactRef>,
    /// Logical stream key for per-aggregate sequencing (AD-105).
    #[serde(default = "default_aggregate_id")]
    pub aggregate_id: String,
    /// Monotonic sequence within `aggregate_id`. `0` = legacy/unknown;
    /// new appends always assign `>= 1`.
    #[serde(default)]
    pub aggregate_seq: u64,
    #[serde(default)]
    pub payload_json: Option<String>,
    pub prev_hash: Digest,
    pub hash: Digest,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::DenialReason;
    use jiff::Timestamp;

    fn genesis_hash() -> Digest {
        Digest::parse(format!("sha256:{}", "0".repeat(64))).unwrap()
    }

    #[test]
    fn round_trips_through_serde() {
        let event = AuditEvent {
            id: Ulid::new(),
            schema_version: 1,
            ts: Timestamp::now(),
            kind: AuditKind::from_static("action.gate_decision"),
            action: Some(ActionId::new("email.send")),
            decision: Some(GateDecision::Deny {
                reason: DenialReason::ExplicitDeny,
            }),
            reason: Some("email.send is hard-denied".to_string()),
            task_grant_id: Some(Ulid::new()),
            target_refs: vec![],
            payload_refs: vec![],
            aggregate_id: "task_grant:test".to_string(),
            aggregate_seq: 1,
            payload_json: Some(r#"{"value":42}"#.to_string()),
            prev_hash: genesis_hash(),
            hash: Digest::parse(format!("sha256:{}", "1".repeat(64))).unwrap(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: AuditEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
        // Transparent kind serializes as a plain string.
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["kind"], "action.gate_decision");
    }

    #[test]
    fn target_and_payload_refs_default_to_empty_when_omitted() {
        let value = serde_json::json!({
            "id": Ulid::new().to_string(),
            "schema_version": 1,
            "ts": Timestamp::now().to_string(),
            "kind": "artifact.activated",
            "action": null,
            "decision": null,
            "reason": null,
            "task_grant_id": null,
            "prev_hash": genesis_hash().as_str(),
            "hash": format!("sha256:{}", "2".repeat(64)),
        });
        let event: AuditEvent = serde_json::from_value(value).unwrap();
        assert!(event.target_refs.is_empty() && event.payload_refs.is_empty());
        // Legacy rows: system aggregate + seq 0 sentinel.
        assert_eq!(event.aggregate_id, "system");
        assert_eq!(event.aggregate_seq, 0);
        assert_eq!(event.payload_json, None);
    }

    #[test]
    fn rejects_unknown_fields() {
        let mut value = serde_json::json!({
            "id": Ulid::new().to_string(),
            "schema_version": 1,
            "ts": Timestamp::now().to_string(),
            "kind": "artifact.activated",
            "action": null,
            "decision": null,
            "reason": null,
            "task_grant_id": null,
            "prev_hash": genesis_hash().as_str(),
            "hash": format!("sha256:{}", "2".repeat(64)),
        });
        value
            .as_object_mut()
            .unwrap()
            .insert("raw_email_body".into(), serde_json::json!("plaintext leak"));
        assert!(serde_json::from_value::<AuditEvent>(value).is_err());
    }

    #[test]
    fn audit_kind_rejects_empty() {
        assert_eq!(AuditKind::new(""), Err(AuditKindError::Empty));
        assert!(AuditKind::new("action.gated").is_ok());
    }

    #[test]
    fn audit_kind_rejects_empty_on_deserialize() {
        let err = serde_json::from_str::<AuditKind>(r#""""#).unwrap_err();
        assert!(err.to_string().contains("non-empty") || err.to_string().contains("empty"));
    }
}

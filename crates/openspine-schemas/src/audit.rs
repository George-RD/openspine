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

/// One append-only, hash-chained audit log row.
///
/// `kind` is an open vocabulary (e.g. `action.gate_decision`,
/// `telegram.owner.message.ignored`, `artifact.activated`) — new audit kinds
/// are added without a schema change, matching D-013.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditEvent {
    pub id: Ulid,
    pub schema_version: u32,
    pub ts: jiff::Timestamp,
    pub kind: String,
    pub action: Option<ActionId>,
    pub decision: Option<GateDecision>,
    pub reason: Option<String>,
    pub task_grant_id: Option<Ulid>,
    #[serde(default)]
    pub target_refs: Vec<ArtifactRef>,
    #[serde(default)]
    pub payload_refs: Vec<ArtifactRef>,
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
            kind: "action.gate_decision".to_string(),
            action: Some(ActionId::new("email.send")),
            decision: Some(GateDecision::Deny {
                reason: DenialReason::ExplicitDeny,
            }),
            reason: Some("email.send is hard-denied".to_string()),
            task_grant_id: Some(Ulid::new()),
            target_refs: vec![],
            payload_refs: vec![],
            prev_hash: genesis_hash(),
            hash: Digest::parse(format!("sha256:{}", "1".repeat(64))).unwrap(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: AuditEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
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
}

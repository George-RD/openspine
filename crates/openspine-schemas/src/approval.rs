//! Digest-bound approvals (PRD §17, D-011).
//!
//! Approval applies to exact payloads and targets. If body, recipient,
//! target, thread, connector, or account role changes, the approval is
//! invalid — enforced by `gate()` matching both digests exactly
//! (`openspine-gate`, Step 6 of the build plan).

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::digest::Digest;

/// PRD §17 `decision`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approved,
    Rejected,
    Edited,
}

/// PRD §17 `timeout_behavior`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutBehavior {
    DoNothing,
}

/// A digest-bound approval record (PRD §17).
///
/// `approval_channel` is not in the PRD's literal example — it records
/// *how* the owner approved (e.g. `telegram_inline`), needed once Step 6
/// implements the inline-button approval flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRecord {
    pub id: Ulid,
    pub schema_version: u32,
    pub action_request_id: Ulid,
    pub approved_by: String,
    pub approved_at: jiff::Timestamp,
    pub approved_payload_digest: Digest,
    pub approved_target_digest: Digest,
    pub expires_at: jiff::Timestamp,
    pub decision: ApprovalDecision,
    pub timeout_behavior: TimeoutBehavior,
    pub approval_channel: String,
}

impl ApprovalRecord {
    /// Does this approval authorize exactly this payload/target pair, right now?
    pub fn matches(
        &self,
        payload_digest: &Digest,
        target_digest: &Digest,
        now: jiff::Timestamp,
    ) -> bool {
        self.decision == ApprovalDecision::Approved
            && &self.approved_payload_digest == payload_digest
            && &self.approved_target_digest == target_digest
            && now < self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::Timestamp;

    fn sample_approval() -> ApprovalRecord {
        let now = Timestamp::now();
        ApprovalRecord {
            id: Ulid::new(),
            schema_version: 1,
            action_request_id: Ulid::new(),
            approved_by: "owner".to_string(),
            approved_at: now,
            approved_payload_digest: Digest::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
            approved_target_digest: Digest::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
            expires_at: now + std::time::Duration::from_secs(900),
            decision: ApprovalDecision::Approved,
            timeout_behavior: TimeoutBehavior::DoNothing,
            approval_channel: "telegram_inline".to_string(),
        }
    }

    #[test]
    fn round_trips_through_serde() {
        let approval = sample_approval();
        let json = serde_json::to_string(&approval).unwrap();
        let back: ApprovalRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(approval, back);
    }

    #[test]
    fn matches_requires_both_digests_and_approved_decision() {
        let approval = sample_approval();
        let now = approval.approved_at;
        assert!(approval.matches(
            &approval.approved_payload_digest,
            &approval.approved_target_digest,
            now
        ));

        let other_digest = Digest::parse(format!("sha256:{}", "c".repeat(64))).unwrap();
        assert!(!approval.matches(&other_digest, &approval.approved_target_digest, now));
        assert!(!approval.matches(&approval.approved_payload_digest, &other_digest, now));
    }

    #[test]
    fn matches_rejects_expired_approval() {
        let approval = sample_approval();
        assert!(!approval.matches(
            &approval.approved_payload_digest,
            &approval.approved_target_digest,
            approval.expires_at
        ));
    }

    #[test]
    fn matches_rejects_non_approved_decisions() {
        let mut approval = sample_approval();
        approval.decision = ApprovalDecision::Rejected;
        let now = approval.approved_at;
        assert!(!approval.matches(
            &approval.approved_payload_digest,
            &approval.approved_target_digest,
            now
        ));
    }
}

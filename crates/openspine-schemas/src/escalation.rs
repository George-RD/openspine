//! Escalation and refusal surfaces (AD-133, AD-151).
//!
//! The pure chokepoint function [`surface_denial`] separates the
//! worker/counterparty-facing deferral text from the owner-only
//! [`EscalationNotice`]. Callers MUST route the two halves on separate
//! channels: the notice never rides on a worker HTTP response.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::action::{ActionId, DenialReason, GateDecision};
use crate::grant::TaskGrant;

/// The ONE canonical policy-free refusal a counterparty ever sees (AD-151).
///
/// Phrasing is learnable presentation (AD-135); the invariant that this is
/// the only human-facing refusal text is kernel.
pub const CANONICAL_DEFERRAL: &str = "I need to check on that — I'll get back to you";

/// Owner-only escalation record. Carries the real [`DenialReason`] so the
/// owner can act. MUST NOT be serialized into a worker-facing response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EscalationNotice {
    pub task_grant_id: Ulid,
    pub denied_action: ActionId,
    pub reason: DenialReason,
    /// The complete typed gate outcome, retained for owner audit only.
    pub decision: GateDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counterparty: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub escalated_at: Timestamp,
    pub schema_version: u32,
}

/// Producer-specific owner escalation payload. The tagged enum makes invalid
/// combinations unrepresentable: worker-confidence events cannot carry a
/// fabricated gate reason, and gate-denial events cannot omit their decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EscalationPayload {
    GateDenial {
        action: ActionId,
        decision: GateDecision,
        reason: DenialReason,
        /// Owner-only summary. Never copied into the worker-facing response.
        summary: String,
    },
    WorkerConfidence {
        /// Owner-only summary. Never copied into the worker-facing response.
        summary: String,
    },
}

/// The owner-routed escalation envelope shared by all producers. The worker
/// runtime can route `WorkerConfidence` without inventing gate-denial fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EscalationEvent {
    pub task_grant_id: Ulid,
    pub payload: EscalationPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub occurred_at: Timestamp,
    pub schema_version: u32,
}

impl EscalationEvent {
    pub fn from_denial(notice: &EscalationNotice) -> Self {
        Self {
            task_grant_id: notice.task_grant_id,
            payload: EscalationPayload::GateDenial {
                action: notice.denied_action.clone(),
                decision: notice.decision.clone(),
                reason: notice.reason,
                summary: format!(
                    "denied `{}` ({})",
                    notice.denied_action,
                    denial_reason_code(notice.reason)
                ),
            },
            thread_id: notice.thread_id.clone(),
            occurred_at: notice.escalated_at,
            schema_version: 1,
        }
    }
}

/// Worker/counterparty-facing deferral. Always the canonical constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerFacingDeferral {
    pub text: &'static str,
}

impl WorkerFacingDeferral {
    pub const fn canonical() -> Self {
        Self {
            text: CANONICAL_DEFERRAL,
        }
    }
}

/// Map a gate decision into the worker-facing deferral and owner-only
/// escalation. Returns `None` for Allow / EffectSuppressed.
///
/// The two halves of the pair are deliberately separate types so a caller
/// cannot accidentally put the owner-only notice on the worker channel.
pub fn surface_denial(
    grant: &TaskGrant,
    action: &ActionId,
    decision: &GateDecision,
    counterparty: Option<&str>,
    now: Timestamp,
) -> Option<(WorkerFacingDeferral, EscalationNotice)> {
    let reason = match decision {
        GateDecision::Deny { reason } => *reason,
        GateDecision::ApprovalRequired { .. } => DenialReason::ApprovalMissing,
        GateDecision::Allow | GateDecision::EffectSuppressed => return None,
    };

    let notice = EscalationNotice {
        task_grant_id: grant.id,
        denied_action: action.clone(),
        reason,
        decision: decision.clone(),
        counterparty: counterparty.map(str::to_string),
        thread_id: grant.thread_id.clone(),
        escalated_at: now,
        schema_version: 1,
    };
    Some((WorkerFacingDeferral::canonical(), notice))
}

/// Snake-case machine outcome for audit `reason` fields. Not free-form
/// policy prose — just the enum code as a stable string.
pub fn denial_reason_code(reason: DenialReason) -> &'static str {
    match reason {
        DenialReason::NotGranted => "not_granted",
        DenialReason::ExplicitDeny => "explicit_deny",
        DenialReason::GrantExpired => "grant_expired",
        DenialReason::ApprovalMissing => "approval_missing",
        DenialReason::ApprovalDigestMismatch => "approval_digest_mismatch",
        DenialReason::ApprovalExpired => "approval_expired",
        DenialReason::SelectionTokenInvalid => "selection_token_invalid",
        DenialReason::KernelOriginNotTrusted => "kernel_origin_not_trusted",
        DenialReason::ChannelBindingViolation => "channel_binding_violation",
        DenialReason::LimitExceeded => "limit_exceeded",
        DenialReason::UnknownAction => "unknown_action",
        DenialReason::CaveatWidening => "caveat_widening",
        DenialReason::EgressClassNotGranted => "egress_class_not_granted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::Lifecycle;
    use crate::grant::{GrantLimits, GrantMode};
    use crate::grant_chain::TEST_GRANT_HMAC_KEY;

    fn sample_grant() -> TaskGrant {
        let now = Timestamp::now();
        let id = Ulid::new();
        let mut g = TaskGrant {
            id,
            schema_version: 1,
            lifecycle_state: Lifecycle::Active,
            user: "owner".into(),
            purpose: "test".into(),
            issued_by: "kernel".into(),
            issued_at: now,
            expires_at: now + std::time::Duration::from_secs(60),
            event_id: Ulid::new(),
            route_id: "r".into(),
            agent_id: "a".into(),
            workflow_id: "w".into(),
            capability_pack_id: "p".into(),
            authority_sources: vec![],
            selection_tokens: vec![],
            allowed_actions: vec![],
            approval_required_actions: vec![],
            denied_actions: vec![],
            output_channels: vec![],
            limits: GrantLimits {
                max_model_calls: 1,
                max_artifacts: 1,
                max_runtime_seconds: 60,
            },
            task_token: "a".repeat(64),
            root_grant_id: id,
            parent_grant_id: None,
            mode: GrantMode::Live,
            chain: vec![],
            caveat_mac: String::new(),
            thread_id: None,
            allowed_egress_classes: vec![],
        };
        g.seal_root(TEST_GRANT_HMAC_KEY);
        g
    }

    const ALL_REASONS: &[DenialReason] = &[
        DenialReason::NotGranted,
        DenialReason::ExplicitDeny,
        DenialReason::GrantExpired,
        DenialReason::ApprovalMissing,
        DenialReason::ApprovalDigestMismatch,
        DenialReason::ApprovalExpired,
        DenialReason::SelectionTokenInvalid,
        DenialReason::KernelOriginNotTrusted,
        DenialReason::ChannelBindingViolation,
        DenialReason::LimitExceeded,
        DenialReason::UnknownAction,
        DenialReason::CaveatWidening,
        DenialReason::EgressClassNotGranted,
    ];

    #[test]
    fn deny_surfaces_canonical_deferral_and_escalation() {
        let grant = sample_grant();
        let action = ActionId::new("email.send");
        let decision = GateDecision::Deny {
            reason: DenialReason::NotGranted,
        };
        let now = Timestamp::now();
        let (deferral, notice) =
            surface_denial(&grant, &action, &decision, Some("alice@example.com"), now)
                .expect("deny must surface");
        assert_eq!(deferral.text, CANONICAL_DEFERRAL);
        assert_eq!(notice.task_grant_id, grant.id);
        assert_eq!(notice.denied_action, action);
        assert_eq!(notice.reason, DenialReason::NotGranted);
        assert_eq!(notice.counterparty.as_deref(), Some("alice@example.com"));
        assert_eq!(notice.escalated_at, now);
    }

    #[test]
    fn approval_required_surfaces_as_deferral_with_approval_missing() {
        let grant = sample_grant();
        let action = ActionId::new("email.create_draft");
        let decision = GateDecision::ApprovalRequired {
            approval_type: "email.create_draft".into(),
        };
        let (deferral, notice) = surface_denial(&grant, &action, &decision, None, Timestamp::now())
            .expect("approval-required must surface");
        assert_eq!(deferral.text, CANONICAL_DEFERRAL);
        assert_eq!(notice.reason, DenialReason::ApprovalMissing);
    }

    #[test]
    fn allow_and_suppressed_do_not_surface() {
        let grant = sample_grant();
        let action = ActionId::new("openspine.status.read");
        assert!(surface_denial(
            &grant,
            &action,
            &GateDecision::Allow,
            None,
            Timestamp::now()
        )
        .is_none());
        assert!(surface_denial(
            &grant,
            &action,
            &GateDecision::EffectSuppressed,
            None,
            Timestamp::now()
        )
        .is_none());
    }

    #[test]
    fn every_denial_reason_yields_same_counterparty_text() {
        let grant = sample_grant();
        let action = ActionId::new("email.send");
        let now = Timestamp::now();
        for reason in ALL_REASONS {
            let decision = GateDecision::Deny { reason: *reason };
            let (deferral, notice) =
                surface_denial(&grant, &action, &decision, None, now).expect("deny must surface");
            assert_eq!(deferral.text, CANONICAL_DEFERRAL);
            assert_eq!(notice.reason, *reason);
            // No-leak: the deferral text must not contain the reason code.
            let code = denial_reason_code(*reason);
            assert!(
                !deferral.text.contains(code),
                "deferral leaked reason code {code}"
            );
            assert!(!deferral.text.to_lowercase().contains("not allowed"));
            assert!(!deferral.text.to_lowercase().contains("policy"));
            assert!(!deferral.text.to_lowercase().contains("denied"));
        }
    }

    #[test]
    fn escalation_notice_round_trips() {
        let grant = sample_grant();
        let action = ActionId::new("email.send");
        let decision = GateDecision::Deny {
            reason: DenialReason::ExplicitDeny,
        };
        let (_, notice) =
            surface_denial(&grant, &action, &decision, Some("c"), Timestamp::now()).unwrap();
        let json = serde_json::to_string(&notice).unwrap();
        let back: EscalationNotice = serde_json::from_str(&json).unwrap();
        assert_eq!(notice, back);
    }

    #[test]
    fn generic_worker_confidence_payload_has_no_gate_fields() {
        let event = EscalationEvent {
            task_grant_id: Ulid::new(),
            payload: EscalationPayload::WorkerConfidence {
                summary: "confidence below threshold".to_string(),
            },
            thread_id: None,
            occurred_at: Timestamp::now(),
            schema_version: 1,
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["payload"]["kind"], "worker_confidence");
        assert!(value["payload"].get("reason").is_none());
        assert!(value["payload"].get("decision").is_none());
        let back: EscalationEvent = serde_json::from_value(value).unwrap();
        assert_eq!(event, back);
    }

    #[test]
    fn canonical_deferral_is_stable() {
        assert_eq!(
            CANONICAL_DEFERRAL,
            "I need to check on that — I'll get back to you"
        );
        assert_eq!(WorkerFacingDeferral::canonical().text, CANONICAL_DEFERRAL);
    }
}

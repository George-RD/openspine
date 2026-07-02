//! `gate()` — the single mediation point every effectful action passes
//! through (design.md, PRD §8.3, spec.md).
//!
//! Pure decision logic: no storage, no I/O. Approval and selection-token
//! *lookups* are supplied by the caller (the kernel, Step 4/5) through
//! [`GateContext`] so this crate never touches SQLite directly.

use jiff::Timestamp;
use ulid::Ulid;

use openspine_schemas::action::{ActionId, ActionRequest, DenialReason, GateDecision};
use openspine_schemas::approval::{ApprovalDecision, ApprovalRecord};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use openspine_schemas::event::TargetRef;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::selection::SelectionToken;

/// Everything a caller must be able to look up for `gate()` to resolve one
/// [`ActionRequest`] without doing storage I/O itself.
pub trait GateContext {
    /// The approval decision recorded against this exact `action_request_id`,
    /// if the owner has already decided one way or another (approved,
    /// rejected, or edited). `None` means "never asked" — the only path
    /// that leads back to [`GateDecision::ApprovalRequired`]. Once a
    /// decision exists, a request whose payload/target digest no longer
    /// matches it is denied outright (D-011), never re-asked.
    fn approval_for_request(&self, action_request_id: Ulid) -> Option<ApprovalRecord>;

    /// Look up a selection token by id. Not called by this change's own
    /// `gate()` body — declared now so the trait boundary is stable before
    /// Step 5 wires selection-token validation into connector dispatch.
    fn find_selection_token(&self, id: Ulid) -> Option<SelectionToken>;
}

/// Audit-sufficient metadata for one gate decision (spec.md "Gate decisions
/// MUST be auditable"). Private payloads are represented by refs/digests
/// only — [`ArtifactRef`] carries a digest and lifecycle state, never
/// plaintext (PRD §18); `target_ref`/`target_digest` mirror whatever the
/// request carried, so a denial can be traced back to exactly what was
/// being acted on without ever recording raw content.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditMeta {
    pub action: ActionId,
    pub task_grant_id: Ulid,
    pub target_ref: Option<TargetRef>,
    pub target_digest: Option<Digest>,
    pub payload_refs: Vec<ArtifactRef>,
}

/// The full outcome of mediating one [`ActionRequest`]: the decision plus
/// enough metadata for the caller to persist an audit event. `gate()` never
/// writes the audit event itself (no I/O) — it returns what the write needs.
#[derive(Debug, Clone, PartialEq)]
pub struct GateOutcome {
    pub decision: GateDecision,
    pub audit: AuditMeta,
}

/// Mediate one action request against its task grant (PRD §8.3).
///
/// Precedence: explicit deny > approval-required > allow > unspecified deny.
/// A grant that has already expired is denied before any list is consulted
/// — an expired grant authorizes nothing, no matter what its lists say.
pub fn gate(
    grant: &TaskGrant,
    req: &ActionRequest,
    ctx: &dyn GateContext,
    now: Timestamp,
) -> GateOutcome {
    let decision = resolve(grant, req, ctx, now);
    GateOutcome {
        decision,
        audit: AuditMeta {
            action: req.action.clone(),
            task_grant_id: grant.id,
            target_ref: req.target_ref.clone(),
            target_digest: req.target_digest.clone(),
            payload_refs: req.payload_ref.iter().cloned().collect(),
        },
    }
}

fn resolve(
    grant: &TaskGrant,
    req: &ActionRequest,
    ctx: &dyn GateContext,
    now: Timestamp,
) -> GateDecision {
    if grant.is_expired(now) {
        return GateDecision::Deny {
            reason: DenialReason::GrantExpired,
        };
    }

    if grant.denied_actions.contains(&req.action) {
        return GateDecision::Deny {
            reason: DenialReason::ExplicitDeny,
        };
    }

    if grant.approval_required_actions.contains(&req.action) {
        return resolve_approval_required(req, ctx, now);
    }

    if grant.allowed_actions.contains(&req.action) {
        return GateDecision::Allow;
    }

    GateDecision::Deny {
        reason: DenialReason::NotGranted,
    }
}

fn resolve_approval_required(
    req: &ActionRequest,
    ctx: &dyn GateContext,
    now: Timestamp,
) -> GateDecision {
    let Some(approval) = ctx.approval_for_request(req.id) else {
        return GateDecision::ApprovalRequired {
            approval_type: req.action.as_str().to_string(),
        };
    };

    let payload_digest: Option<&Digest> = req.payload_ref.as_ref().map(|r| &r.digest);
    let target_digest: Option<&Digest> = req.target_digest.as_ref();

    let currently_matches = match (payload_digest, target_digest) {
        (Some(pd), Some(td)) => approval.matches(pd, td, now),
        // Nothing to bind the approval to — it can never authorize this
        // request, digest-bound or not.
        _ => false,
    };

    if currently_matches {
        return GateDecision::Allow;
    }

    // An approval decision exists for this exact action request but does
    // not currently authorize it. This is a denial, never a re-ask: an
    // agent that mutates a payload/target after approval must not be able
    // to walk gate() back into ApprovalRequired and try again unreviewed.
    let reason = if now >= approval.expires_at {
        DenialReason::ApprovalExpired
    } else if approval.decision != ApprovalDecision::Approved {
        DenialReason::ApprovalMissing
    } else {
        DenialReason::ApprovalDigestMismatch
    };
    GateDecision::Deny { reason }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use openspine_schemas::artifact::Lifecycle;
    use openspine_schemas::digest::Digest;
    use openspine_schemas::event::{TargetRef, TargetRefKind};
    use openspine_schemas::grant::GrantLimits;

    use super::*;

    fn digest(byte: char) -> Digest {
        Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).unwrap()
    }

    fn grant_with(allowed: &[&str], approval_required: &[&str], denied: &[&str]) -> TaskGrant {
        let issued_at = Timestamp::now();
        TaskGrant {
            id: Ulid::new(),
            schema_version: 1,
            lifecycle_state: Lifecycle::Active,
            user: "owner".to_string(),
            purpose: "test".to_string(),
            issued_by: "kernel".to_string(),
            issued_at,
            expires_at: issued_at + std::time::Duration::from_secs(120),
            event_id: Ulid::new(),
            route_id: "owner_telegram_main_assistant".to_string(),
            agent_id: "main_assistant_agent".to_string(),
            workflow_id: "owner_control_conversation".to_string(),
            capability_pack_id: "owner_control_basic_pack".to_string(),
            authority_sources: vec!["global_policy:v1".to_string()],
            selection_tokens: vec![],
            allowed_actions: allowed.iter().map(|a| ActionId::new(*a)).collect(),
            approval_required_actions: approval_required
                .iter()
                .map(|a| ActionId::new(*a))
                .collect(),
            denied_actions: denied.iter().map(|a| ActionId::new(*a)).collect(),
            output_channels: vec![],
            limits: GrantLimits {
                max_model_calls: 8,
                max_artifacts: 20,
                max_runtime_seconds: 120,
            },
            task_token: "a".repeat(64),
        }
    }

    fn request_for(action: &str) -> ActionRequest {
        ActionRequest {
            id: Ulid::new(),
            task_grant_id: Ulid::new(),
            action: ActionId::new(action),
            target_ref: Some(TargetRef {
                kind: TargetRefKind::EmailThread,
                id: Some("thread-1".to_string()),
            }),
            payload_ref: Some(ArtifactRef {
                digest: digest('a'),
                schema_version: 1,
            }),
            target_digest: Some(digest('b')),
            requested_at: Timestamp::now(),
            schema_version: 1,
        }
    }

    /// Test double for [`GateContext`]: a fixed table of approval records
    /// keyed by the `action_request_id` they decide.
    #[derive(Default)]
    struct MockContext {
        approvals: HashMap<Ulid, ApprovalRecord>,
    }

    impl GateContext for MockContext {
        fn approval_for_request(&self, action_request_id: Ulid) -> Option<ApprovalRecord> {
            self.approvals.get(&action_request_id).cloned()
        }

        fn find_selection_token(&self, _id: Ulid) -> Option<SelectionToken> {
            None
        }
    }

    fn approval_for(
        req: &ActionRequest,
        decision: ApprovalDecision,
        ttl_secs: i64,
    ) -> ApprovalRecord {
        let now = Timestamp::now();
        ApprovalRecord {
            id: Ulid::new(),
            schema_version: 1,
            action_request_id: req.id,
            approved_by: "owner".to_string(),
            approved_at: now,
            approved_payload_digest: req.payload_ref.as_ref().unwrap().digest.clone(),
            approved_target_digest: req.target_digest.clone().unwrap(),
            expires_at: now + std::time::Duration::from_secs(ttl_secs.max(0) as u64),
            decision,
            timeout_behavior: openspine_schemas::approval::TimeoutBehavior::DoNothing,
            approval_channel: "telegram_inline".to_string(),
        }
    }

    #[test]
    fn allowed_action_returns_allow() {
        let grant = grant_with(&["openspine.status.read"], &[], &[]);
        let req = request_for("openspine.status.read");
        let ctx = MockContext::default();
        let outcome = gate(&grant, &req, &ctx, Timestamp::now());
        assert_eq!(outcome.decision, GateDecision::Allow);
    }

    #[test]
    fn denied_action_returns_deny() {
        let grant = grant_with(&[], &[], &["email.read_inbox"]);
        let req = request_for("email.read_inbox");
        let ctx = MockContext::default();
        let outcome = gate(&grant, &req, &ctx, Timestamp::now());
        assert_eq!(
            outcome.decision,
            GateDecision::Deny {
                reason: DenialReason::ExplicitDeny
            }
        );
    }

    #[test]
    fn approval_required_action_returns_approval_required() {
        let grant = grant_with(&[], &["email.create_draft"], &[]);
        let req = request_for("email.create_draft");
        let ctx = MockContext::default();
        let outcome = gate(&grant, &req, &ctx, Timestamp::now());
        assert_eq!(
            outcome.decision,
            GateDecision::ApprovalRequired {
                approval_type: "email.create_draft".to_string()
            }
        );
    }

    #[test]
    fn approval_required_action_does_not_execute() {
        // "Does not execute" for a pure decision function means: the
        // outcome is never `Allow` until a matching approval exists.
        let grant = grant_with(&[], &["email.create_draft"], &[]);
        let req = request_for("email.create_draft");
        let ctx = MockContext::default();
        let outcome = gate(&grant, &req, &ctx, Timestamp::now());
        assert_ne!(outcome.decision, GateDecision::Allow);
    }

    #[test]
    fn allowed_plus_denied_returns_deny() {
        let grant = grant_with(&["email.send"], &[], &["email.send"]);
        let req = request_for("email.send");
        let ctx = MockContext::default();
        let outcome = gate(&grant, &req, &ctx, Timestamp::now());
        assert_eq!(
            outcome.decision,
            GateDecision::Deny {
                reason: DenialReason::ExplicitDeny
            }
        );
    }

    #[test]
    fn allowed_plus_approval_required_returns_approval_required() {
        let grant = grant_with(&["email.create_draft"], &["email.create_draft"], &[]);
        let req = request_for("email.create_draft");
        let ctx = MockContext::default();
        let outcome = gate(&grant, &req, &ctx, Timestamp::now());
        assert!(matches!(
            outcome.decision,
            GateDecision::ApprovalRequired { .. }
        ));
    }

    #[test]
    fn unspecified_action_returns_deny() {
        let grant = grant_with(&["openspine.status.read"], &[], &[]);
        let req = request_for("network.raw_egress");
        let ctx = MockContext::default();
        let outcome = gate(&grant, &req, &ctx, Timestamp::now());
        assert_eq!(
            outcome.decision,
            GateDecision::Deny {
                reason: DenialReason::NotGranted
            }
        );
    }

    #[test]
    fn expired_grant_denies_even_an_allowed_action() {
        let grant = grant_with(&["openspine.status.read"], &[], &[]);
        let req = request_for("openspine.status.read");
        let ctx = MockContext::default();
        let outcome = gate(&grant, &req, &ctx, grant.expires_at);
        assert_eq!(
            outcome.decision,
            GateDecision::Deny {
                reason: DenialReason::GrantExpired
            }
        );
    }

    #[test]
    fn matching_approval_allows_the_exact_request() {
        let grant = grant_with(&[], &["email.create_draft"], &[]);
        let req = request_for("email.create_draft");
        let approval = approval_for(&req, ApprovalDecision::Approved, 900);
        let ctx = MockContext {
            approvals: HashMap::from([(req.id, approval)]),
        };
        let outcome = gate(&grant, &req, &ctx, Timestamp::now());
        assert_eq!(outcome.decision, GateDecision::Allow);
    }

    #[test]
    fn approved_but_payload_changed_since_is_denied_not_reasked() {
        let grant = grant_with(&[], &["email.create_draft"], &[]);
        let mut req = request_for("email.create_draft");
        let approval = approval_for(&req, ApprovalDecision::Approved, 900);
        // The agent mutates the payload after approval (edited body, new digest).
        req.payload_ref = Some(ArtifactRef {
            digest: digest('f'),
            schema_version: 1,
        });
        let ctx = MockContext {
            approvals: HashMap::from([(req.id, approval)]),
        };
        let outcome = gate(&grant, &req, &ctx, Timestamp::now());
        assert_eq!(
            outcome.decision,
            GateDecision::Deny {
                reason: DenialReason::ApprovalDigestMismatch
            }
        );
    }

    #[test]
    fn expired_approval_is_denied_not_reasked() {
        let grant = grant_with(&[], &["email.create_draft"], &[]);
        let req = request_for("email.create_draft");
        let approval = approval_for(&req, ApprovalDecision::Approved, 1);
        let expired_at = approval.expires_at;
        let ctx = MockContext {
            approvals: HashMap::from([(req.id, approval)]),
        };
        let outcome = gate(&grant, &req, &ctx, expired_at);
        assert_eq!(
            outcome.decision,
            GateDecision::Deny {
                reason: DenialReason::ApprovalExpired
            }
        );
    }

    #[test]
    fn rejected_approval_is_denied_not_reasked() {
        let grant = grant_with(&[], &["email.create_draft"], &[]);
        let req = request_for("email.create_draft");
        let approval = approval_for(&req, ApprovalDecision::Rejected, 900);
        let ctx = MockContext {
            approvals: HashMap::from([(req.id, approval)]),
        };
        let outcome = gate(&grant, &req, &ctx, Timestamp::now());
        assert_eq!(
            outcome.decision,
            GateDecision::Deny {
                reason: DenialReason::ApprovalMissing
            }
        );
    }

    #[test]
    fn audit_metadata_records_action_grant_and_refs_not_plaintext() {
        let grant = grant_with(&["openspine.status.read"], &[], &[]);
        let req = request_for("openspine.status.read");
        let ctx = MockContext::default();
        let outcome = gate(&grant, &req, &ctx, Timestamp::now());
        assert_eq!(outcome.audit.action, req.action);
        assert_eq!(outcome.audit.task_grant_id, grant.id);
        assert_eq!(outcome.audit.target_ref, req.target_ref);
        assert_eq!(outcome.audit.target_digest, req.target_digest);
        assert_eq!(
            outcome.audit.payload_refs,
            vec![req.payload_ref.clone().unwrap()]
        );
    }

    #[test]
    fn denial_audit_metadata_still_carries_refs() {
        let grant = grant_with(&[], &[], &["email.send"]);
        let req = request_for("email.send");
        let ctx = MockContext::default();
        let outcome = gate(&grant, &req, &ctx, Timestamp::now());
        assert!(matches!(outcome.decision, GateDecision::Deny { .. }));
        assert_eq!(outcome.audit.payload_refs.len(), 1);
    }
}

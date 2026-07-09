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

fn approval_for(req: &ActionRequest, decision: ApprovalDecision, ttl_secs: i64) -> ApprovalRecord {
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
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), Timestamp::now());
    assert_eq!(outcome.decision, GateDecision::Allow);
}

#[test]
fn denied_action_returns_deny() {
    let grant = grant_with(&[], &[], &["email.read_inbox"]);
    let req = request_for("email.read_inbox");
    let ctx = MockContext::default();
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), Timestamp::now());
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
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), Timestamp::now());
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
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), Timestamp::now());
    assert_ne!(outcome.decision, GateDecision::Allow);
}

#[test]
fn allowed_plus_denied_returns_deny() {
    let grant = grant_with(&["email.send"], &[], &["email.send"]);
    let req = request_for("email.send");
    let ctx = MockContext::default();
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), Timestamp::now());
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
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), Timestamp::now());
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
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), Timestamp::now());
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
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), grant.expires_at);
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
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), Timestamp::now());
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
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), Timestamp::now());
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
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), expired_at);
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
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), Timestamp::now());
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
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), Timestamp::now());
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
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), Timestamp::now());
    assert!(matches!(outcome.decision, GateDecision::Deny { .. }));
    assert_eq!(outcome.audit.payload_refs.len(), 1);
}

/// Test fixture catalog: the canonical set of action ids the kernel
/// recognizes. Mirrors `openspine_kernel::action_catalog::canonical_catalog`
/// so gate tests can exercise fail-fast behavior without depending on the
/// kernel crate.
fn test_catalog() -> ActionCatalog {
    let ids = [
        "openspine.status.read",
        "workflow.invoke:approved",
        "artifact.propose",
        "setup.workflow.start",
        "memory.read:owner_preferences_limited",
        "model.generate:approved_provider",
        "lyra.ui.preview",
        "telegram.reply:owner_channel",
        "connector.enable",
        "route.activate",
        "capability_pack.change",
        "workflow.activate",
        "policy.change_proposal",
        "email.read_inbox",
        "email.read_thread:unselected",
        "email.send",
        "email.read_attachment",
        "network.raw_egress",
        "vault.secret_read",
        "policy.modify_direct",
        "filesystem.host_read",
        "filesystem.host_write",
        "coolify.deploy",
        "coolify.rollback",
        "coolify.secret_modify",
        "email.read_thread:selected_no_attachments",
        "memory.read:writing_preferences_scoped",
        "artifact.write:task_scratch",
        "email.create_draft",
        "artifact.activate",
        "coolify.delete_resource",
    ];
    ActionCatalog::new(ids.iter().map(|s| ActionId::new(*s)))
}

/// D-053: a request whose action id is not in the catalog is denied with
/// `DenialReason::UnknownAction` — fail-fast on unknown ids. The denial is
/// still audited.
#[test]
fn gate_denies_catalog_unknown_id_with_unknown_action_reason() {
    let grant = grant_with(&["openspine.status.read"], &[], &[]);
    let req = request_for("totally.unknown.action");
    let ctx = MockContext::default();
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), Timestamp::now());
    assert!(
        matches!(
            outcome.decision,
            GateDecision::Deny {
                reason: DenialReason::UnknownAction
            }
        ),
        "expected UnknownAction denial, got {:?}",
        outcome.decision
    );
    assert_eq!(outcome.audit.action, req.action);
}

/// D-053: a request whose action is *known* to the catalog but was never
/// granted keeps the pre-existing `NotGranted` denial verbatim. The catalog
/// check must not shadow the grant-membership check.
#[test]
fn gate_keeps_not_granted_for_known_ungranted_id() {
    let grant = grant_with(&["email.read_inbox"], &[], &[]);
    let req = request_for("email.send");
    let ctx = MockContext::default();
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), Timestamp::now());
    assert!(
        matches!(
            outcome.decision,
            GateDecision::Deny {
                reason: DenialReason::NotGranted
            }
        ),
        "expected NotGranted for a known-but-ungranted id, got {:?}",
        outcome.decision
    );
}

/// D-053: a grant persisted before the catalog existed could carry an
/// out-of-catalog id in its `allowed_actions`. The gate is the last line of
/// defense — such an id must resolve to `UnknownAction`, never to a
/// list-derived `Allow`.
#[test]
fn gate_denies_stale_granted_but_catalog_unknown_id() {
    let grant = grant_with(&["totally.unknown.action"], &[], &[]);
    let req = request_for("totally.unknown.action");
    let ctx = MockContext::default();
    let outcome = gate(&grant, &req, &ctx, &test_catalog(), Timestamp::now());
    assert!(
        matches!(
            outcome.decision,
            GateDecision::Deny {
                reason: DenialReason::UnknownAction
            }
        ),
        "expected UnknownAction for a stale-granted out-of-catalog id, got {:?}",
        outcome.decision
    );
}

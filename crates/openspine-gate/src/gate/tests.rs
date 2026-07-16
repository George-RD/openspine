use std::collections::HashMap;

use openspine_schemas::approval::{ApprovalDecision, ApprovalRecord};
use openspine_schemas::artifact::{ArtifactRef, Lifecycle};
use openspine_schemas::digest::Digest;
use openspine_schemas::event::{TargetRef, TargetRefKind};
use openspine_schemas::grant::GrantLimits;
use openspine_schemas::grant::GrantMode;

use super::token_tests::test_catalog;
use super::*;

pub(crate) fn digest(byte: char) -> Digest {
    Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).unwrap()
}

pub(crate) fn grant_with(
    allowed: &[&str],
    approval_required: &[&str],
    denied: &[&str],
) -> TaskGrant {
    let issued_at = Timestamp::now();
    let mut grant = TaskGrant {
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
        root_grant_id: Ulid::nil(),
        parent_grant_id: None,
        mode: GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
    };
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    grant
}

pub(crate) fn request_for(action: &str) -> ActionRequest {
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
        selection_token_id: None,
        requested_at: Timestamp::now(),
        schema_version: 1,
    }
}

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
    fn grant_hmac_key(&self) -> Option<Vec<u8>> {
        Some(b"openspine-test-grant-hmac-key-v1".to_vec())
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
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        Timestamp::now(),
    );
    assert_eq!(outcome.decision, GateDecision::Allow);
}

#[test]
fn denied_action_returns_deny() {
    let grant = grant_with(&[], &[], &["email.read_inbox"]);
    let req = request_for("email.read_inbox");
    let ctx = MockContext::default();
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        Timestamp::now(),
    );
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
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        Timestamp::now(),
    );
    assert_eq!(
        outcome.decision,
        GateDecision::ApprovalRequired {
            approval_type: "email.create_draft".to_string()
        }
    );
}

#[test]
fn approval_required_action_does_not_execute() {
    let grant = grant_with(&[], &["email.create_draft"], &[]);
    let req = request_for("email.create_draft");
    let ctx = MockContext::default();
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        Timestamp::now(),
    );
    assert_ne!(outcome.decision, GateDecision::Allow);
}

#[test]
fn allowed_plus_denied_returns_deny() {
    let grant = grant_with(&["email.send"], &[], &["email.send"]);
    let req = request_for("email.send");
    let ctx = MockContext::default();
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        Timestamp::now(),
    );
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
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        Timestamp::now(),
    );
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
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        Timestamp::now(),
    );
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
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        grant.expires_at,
    );
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
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        Timestamp::now(),
    );
    assert_eq!(outcome.decision, GateDecision::Allow);
}

#[test]
fn approved_but_payload_changed_since_is_denied_not_reasked() {
    let grant = grant_with(&[], &["email.create_draft"], &[]);
    let mut req = request_for("email.create_draft");
    let approval = approval_for(&req, ApprovalDecision::Approved, 900);
    req.payload_ref = Some(ArtifactRef {
        digest: digest('f'),
        schema_version: 1,
    });
    let ctx = MockContext {
        approvals: HashMap::from([(req.id, approval)]),
    };
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        Timestamp::now(),
    );
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
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        expired_at,
    );
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
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        Timestamp::now(),
    );
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
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        Timestamp::now(),
    );
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
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        Timestamp::now(),
    );
    assert!(matches!(outcome.decision, GateDecision::Deny { .. }));
    assert_eq!(outcome.audit.payload_refs.len(), 1);
}

#[test]
fn shadow_allow_is_non_executable_effect_suppressed() {
    let mut grant = grant_with(&["openspine.status.read"], &[], &[]);
    grant.mode = GrantMode::Shadow;
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    let req = request_for("openspine.status.read");
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &MockContext::default(),
        &test_catalog(),
        Timestamp::now(),
    );
    assert_eq!(outcome.decision, GateDecision::EffectSuppressed);
}

#[test]
fn shadow_deny_remains_deny() {
    let mut grant = grant_with(&[], &[], &[]);
    grant.mode = GrantMode::Shadow;
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    let outcome = gate(
        &grant,
        &request_for("openspine.status.read"),
        ActionOrigin::Shell,
        &MockContext::default(),
        &test_catalog(),
        Timestamp::now(),
    );
    assert!(matches!(outcome.decision, GateDecision::Deny { .. }));
}

#[test]
fn bound_parameter_conflict_is_caveat_widening() {
    // Valid MAC over a chain that contains conflicting AD-036 bindings, so
    // the failure comes from bindings_valid — not a short-circuit MAC miss.
    let key = b"openspine-test-grant-hmac-key-v1";
    let mut grant = grant_with(&["openspine.status.read"], &[], &[]);
    grant.chain = vec![openspine_schemas::grant::GrantChainStep {
        grant_id: grant.id,
        parent_grant_id: None,
        mode: GrantMode::Live,
        selection_tokens: grant.selection_tokens.clone(),
        added_caveats: vec![
            openspine_schemas::grant::GrantCaveat::BoundParameter {
                name: "recipient".into(),
                value: "a@example.com".into(),
            },
            openspine_schemas::grant::GrantCaveat::BoundParameter {
                name: "recipient".into(),
                value: "b@example.com".into(),
            },
        ],
    }];
    grant.root_grant_id = grant.id;
    let root = openspine_schemas::grant_chain::RootAuthority::from_grant(&grant);
    grant.caveat_mac = openspine_schemas::grant_chain::compute_mac_hex(key, &root, &grant.chain);
    assert!(
        grant.verify_mac(key),
        "precondition: MAC must be valid so bindings_valid is the deny path"
    );
    assert!(!openspine_schemas::grant_chain::bindings_valid(&grant));
    let outcome = gate(
        &grant,
        &request_for("openspine.status.read"),
        ActionOrigin::Shell,
        &MockContext::default(),
        &test_catalog(),
        Timestamp::now(),
    );
    assert_eq!(
        outcome.decision,
        GateDecision::Deny {
            reason: DenialReason::CaveatWidening
        }
    );
}

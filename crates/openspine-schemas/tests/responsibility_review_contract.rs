use std::collections::BTreeSet;

use openspine_schemas::action::{
    ActionDescriptor, ActionImplementationDescriptor, ActionImplementationId, ActionSemantics,
    BudgetWindowBounds, DarkWindowPolicy, DataDestination, DelegationDefaults,
    DelegationPolicyBounds, DelegationProposalMode, EffectKind, EffectReversibility,
    ReviewedScopeDimension,
};
use openspine_schemas::briefcase::CounterpartyRef;
use openspine_schemas::delegation_evidence::{
    DelegationEvidence, DelegationEvidenceError, OwnerApprovalEvidence,
};
use openspine_schemas::digest::Digest;
use openspine_schemas::event::{AccountRole, TargetRef, TargetRefKind};
use openspine_schemas::identity::RelationshipKind;
use openspine_schemas::owner_review::{
    BoundaryBehavior, OwnerReviewDecision, OwnerReviewRequest, OwnerReviewRequestInput,
    ProposalKind, ProposalProvenance, ResponsibilityLifecycleControl, ReviewFallbackBehavior,
    ReviewLimits,
};
use openspine_schemas::resolved_context::{
    ResolvedActionContext, ResolvedActionContextError, ResolvedActionContextInput,
};
use openspine_schemas::responsibility::{
    ResponsibilityAssessment, ResponsibilityCompatibilityBinding, ResponsibilityDriftReason,
    ResponsibilityManifest, ResponsibilityStatus,
};
use openspine_schemas::reviewed_scope::{ReviewedActionScope, ScopeComparison};
use openspine_schemas::standing_rule::BudgetWindow;
use ulid::Ulid;

fn digest(c: char) -> Digest {
    Digest::parse(format!("sha256:{}", c.to_string().repeat(64))).unwrap()
}

fn descriptor() -> ActionDescriptor {
    ActionDescriptor {
        schema_version: 1,
        descriptor_version: 7,
        action_id: "message.create_draft".into(),
        semantics: ActionSemantics {
            owner_verb: "create".into(),
            owner_object: "draft".into(),
            owner_target: "selected conversation".into(),
            effect_kind: EffectKind::OwnerAccountWrite,
            reversibility: EffectReversibility::Reversible,
            destination: DataDestination::OwnerCloudAccount,
        },
        reusable_delegation: true,
        required_scope_dimensions: BTreeSet::from([
            ReviewedScopeDimension::ConnectorImplementation,
            ReviewedScopeDimension::ConnectorInstance,
            ReviewedScopeDimension::AccountRole,
            ReviewedScopeDimension::AccountIdentity,
            ReviewedScopeDimension::Target,
            ReviewedScopeDimension::Counterparty,
            ReviewedScopeDimension::RelationshipTier,
            ReviewedScopeDimension::EffectDestination,
            ReviewedScopeDimension::Workflow,
            ReviewedScopeDimension::TaskShape,
        ]),
        delegation_policy: Some(DelegationPolicyBounds {
            schema_version: 1,
            policy_version: 4,
            quota: BudgetWindowBounds {
                minimum_max: 1,
                maximum_max: 20,
                minimum_window_secs: 60,
                maximum_window_secs: 30 * 24 * 3600,
            },
            rate: BudgetWindowBounds {
                minimum_max: 1,
                maximum_max: 5,
                minimum_window_secs: 60,
                maximum_window_secs: 24 * 3600,
            },
            maximum_lapse_secs: 90 * 24 * 3600,
            proposal_mode: DelegationProposalMode::DefaultsPermitted,
            defaults: Some(DelegationDefaults {
                quota: BudgetWindow {
                    max: 5,
                    window_secs: 7 * 24 * 3600,
                },
                rate: BudgetWindow {
                    max: 1,
                    window_secs: 3600,
                },
                expires_after_secs: 90 * 24 * 3600,
            }),
            dark_window_policy: DarkWindowPolicy::Prohibited,
            fresh_target_selection_required: true,
        }),
    }
}

fn implementation() -> ActionImplementationDescriptor {
    ActionImplementationDescriptor {
        schema_version: 1,
        implementation_version: 9,
        action_id: "message.create_draft".into(),
        implementation_id: ActionImplementationId::new("matrix.message.create_draft"),
        connector_kind: "matrix".into(),
        executor_id: "matrix.draft.executor".into(),
        executor_version: 2,
        resolver_id: "matrix.draft.resolver".into(),
        resolver_version: 3,
    }
}

fn context_input() -> ResolvedActionContextInput {
    ResolvedActionContextInput {
        connector_instance_id: "matrix-primary".into(),
        account_role: Some(AccountRole::OwnerMailbox),
        account_identity_digest: Some(digest('a')),
        target_refs: vec![TargetRef {
            kind: TargetRefKind::Conversation,
            id: Some("conversation-42".into()),
        }],
        counterparty: Some(CounterpartyRef::Bound {
            identity_id: Ulid::from(11_u128),
            relationship: RelationshipKind::Client,
        }),
        bound_parameters: Default::default(),
        target_digest: Some(digest('b')),
        payload_digest: Some(digest('c')),
        workflow_id: Some("reply_workflow".into()),
        task_shape_digest: Some(digest('d')),
    }
}

fn resolved() -> ResolvedActionContext {
    ResolvedActionContext::try_new(&descriptor(), &implementation(), context_input()).unwrap()
}

fn evidence() -> DelegationEvidence {
    let owner = Ulid::from(7_u128);
    let approvals = [101_u128, 102_u128]
        .into_iter()
        .map(|id| OwnerApprovalEvidence {
            decision_event_id: Ulid::from(id),
            owner_principal_id: owner,
            request_digest: digest('1'),
            target_digest: digest('2'),
            payload_digest: digest(if id == 101 { '3' } else { '4' }),
        })
        .collect();
    DelegationEvidence::repeated_approvals(digest('5'), approvals).unwrap()
}

fn limits() -> ReviewLimits {
    ReviewLimits {
        quota: BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        rate: BudgetWindow {
            max: 1,
            window_secs: 3600,
        },
        expires_after_secs: 90 * 24 * 3600,
    }
}

#[test]
fn resolved_context_fails_closed_when_required_scope_is_missing() {
    let mut missing_account = context_input();
    missing_account.account_identity_digest = None;
    assert_eq!(
        ResolvedActionContext::try_new(&descriptor(), &implementation(), missing_account),
        Err(ResolvedActionContextError::MissingScopeDimension {
            dimension: ReviewedScopeDimension::AccountIdentity,
        })
    );

    let mut unresolved_counterparty = context_input();
    unresolved_counterparty.counterparty = Some(CounterpartyRef::Unresolved {
        channel: "matrix".into(),
        identifier: "@someone:example.test".into(),
    });
    assert_eq!(
        ResolvedActionContext::try_new(&descriptor(), &implementation(), unresolved_counterparty,),
        Err(ResolvedActionContextError::CounterpartyMustBeBound)
    );
}

#[test]
fn repeated_approval_evidence_rejects_weak_or_recursive_sets() {
    let owner = Ulid::from(7_u128);
    let one = vec![OwnerApprovalEvidence {
        decision_event_id: Ulid::from(101_u128),
        owner_principal_id: owner,
        request_digest: digest('1'),
        target_digest: digest('2'),
        payload_digest: digest('3'),
    }];
    assert_eq!(
        DelegationEvidence::repeated_approvals(digest('5'), one),
        Err(DelegationEvidenceError::TooFewApprovals { count: 1 })
    );

    let duplicate = OwnerApprovalEvidence {
        decision_event_id: Ulid::from(101_u128),
        owner_principal_id: owner,
        request_digest: digest('1'),
        target_digest: digest('2'),
        payload_digest: digest('3'),
    };
    assert_eq!(
        DelegationEvidence::repeated_approvals(digest('5'), vec![duplicate.clone(), duplicate]),
        Err(DelegationEvidenceError::DuplicateDecisionEvent {
            event_id: Ulid::from(101_u128),
        })
    );

    let valid = evidence();
    assert!(valid.integrity_is_valid());
    let mut tampered = serde_json::to_value(&valid).unwrap();
    tampered["approval_count"] = serde_json::json!(3);
    let tampered: DelegationEvidence = serde_json::from_value(tampered).unwrap();
    assert!(!tampered.integrity_is_valid());
}

#[test]
fn owner_review_is_digest_bound_serializable_and_channel_neutral() {
    let scope = ReviewedActionScope::derive(&descriptor(), &resolved()).unwrap();
    let evidence = evidence();
    let provenance = ProposalProvenance::from_evidence(
        &evidence,
        "Two matching owner approvals for this reviewed context".into(),
    );
    let review = OwnerReviewRequest::try_new(OwnerReviewRequestInput {
        id: Ulid::from(200_u128),
        schema_version: 1,
        review_version: 1,
        proposal_kind: ProposalKind::Responsibility,
        provenance,
        title: "Prepare replies for this client".into(),
        description: "Create drafts in the reviewed mailbox and relationship scope.".into(),
        reviewed_scope: scope,
        automatic_effects: vec!["Create a draft in the owner account".into()],
        remaining_boundaries: vec!["Sending remains denied".into()],
        limits: limits(),
        fallback_behavior: ReviewFallbackBehavior {
            scope_mismatch: BoundaryBehavior::RequireApproval,
            compatibility_drift: BoundaryBehavior::RequireApproval,
            budget_exhaustion: BoundaryBehavior::RequireApproval,
            timeout: BoundaryBehavior::Deny,
        },
        proposal_digest: digest('6'),
        compatibility_digest: resolved().compatibility_digest().clone(),
        available_decisions: BTreeSet::from([
            OwnerReviewDecision::Approve,
            OwnerReviewDecision::Reject,
            OwnerReviewDecision::Narrow,
            OwnerReviewDecision::Edit,
        ]),
        lifecycle_controls: BTreeSet::from([
            ResponsibilityLifecycleControl::Pause,
            ResponsibilityLifecycleControl::Resume,
            ResponsibilityLifecycleControl::Expire,
            ResponsibilityLifecycleControl::Revoke,
        ]),
    })
    .unwrap();

    let json = serde_json::to_string(&review).unwrap();
    assert!(!json.contains("telegram"));
    assert!(!json.contains("chat_id"));
    assert!(!json.contains("terminal"));
    assert_eq!(
        serde_json::from_str::<OwnerReviewRequest>(&json).unwrap(),
        review
    );
    assert!(review.binding_is_valid());
    assert_ne!(review.binding_digest(), &digest('6'));

    let mut tampered = serde_json::to_value(&review).unwrap();
    tampered["title"] = serde_json::json!("Wider responsibility");
    let tampered: OwnerReviewRequest = serde_json::from_value(tampered).unwrap();
    assert!(!tampered.binding_is_valid());
}

#[test]
fn responsibility_is_a_reference_view_and_drift_requires_review() {
    let descriptor = descriptor();
    let context = resolved();
    let scope = ReviewedActionScope::derive(&descriptor, &context).unwrap();
    assert!(scope.binding_is_valid());
    let mut tampered_scope = serde_json::to_value(&scope).unwrap();
    tampered_scope["context_class_digest"] = serde_json::json!(digest('0'));
    let tampered_scope: ReviewedActionScope = serde_json::from_value(tampered_scope).unwrap();
    assert_eq!(
        tampered_scope.compare(&context),
        ScopeComparison::InvalidReviewedScope
    );

    let manifest = ResponsibilityManifest {
        id: "client_reply_drafts".into(),
        schema_version: 1,
        version: 1,
        status: ResponsibilityStatus::Active,
        workflow_id: "reply_workflow".into(),
        standing_rule_id: "client_reply_draft_rule".into(),
        reviewed_scope: scope,
        limits: limits(),
        compatibility: ResponsibilityCompatibilityBinding {
            schema_version: 1,
            descriptor_version: descriptor.descriptor_version,
            implementation_version: context.implementation_version(),
            delegation_policy_version: descriptor
                .delegation_policy
                .as_ref()
                .unwrap()
                .policy_version,
            workflow_version: 3,
            owner_review_binding_digest: digest('7'),
        },
        controls: BTreeSet::from([
            ResponsibilityLifecycleControl::Pause,
            ResponsibilityLifecycleControl::Resume,
            ResponsibilityLifecycleControl::Expire,
            ResponsibilityLifecycleControl::Revoke,
        ]),
        provenance_digest: evidence().evidence_set_digest().unwrap().clone(),
    };

    assert_eq!(
        manifest.assess(Some(&context), 4, 3),
        ResponsibilityAssessment::Compatible
    );
    assert_eq!(
        manifest.assess(None, 4, 3),
        ResponsibilityAssessment::NeedsReview {
            reasons: BTreeSet::from([ResponsibilityDriftReason::ResolvedContextUnavailable]),
        }
    );
    assert_eq!(
        manifest.assess(Some(&context), 5, 4),
        ResponsibilityAssessment::NeedsReview {
            reasons: BTreeSet::from([
                ResponsibilityDriftReason::DelegationPolicyVersionChanged,
                ResponsibilityDriftReason::WorkflowVersionChanged,
            ]),
        }
    );

    let json = serde_json::to_string(&manifest).unwrap();
    assert!(!json.contains("task_grant"));
    assert!(!json.contains("allowed_actions"));
    assert_eq!(
        serde_json::from_str::<ResponsibilityManifest>(&json).unwrap(),
        manifest
    );
}

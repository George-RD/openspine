use std::collections::BTreeSet;

use openspine_schemas::action::{
    ActionCatalog, ActionDescriptor, ActionEgressDeclaration, ActionImplementationDescriptor,
    ActionImplementationId, ActionSemantics, BudgetWindowBounds, DarkWindowPolicy, DataDestination,
    DelegationDefaults, DelegationPolicyBounds, DelegationProposalMode, EffectKind,
    EffectReversibility, ReviewedScopeDimension,
};
use openspine_schemas::briefcase::CounterpartyRef;
use openspine_schemas::delegation_evidence::{DelegationEvidence, OwnerApprovalEvidence};
use openspine_schemas::digest::Digest;
use openspine_schemas::event::{AccountRole, TargetRef, TargetRefKind};
use openspine_schemas::identity::RelationshipKind;
use openspine_schemas::owner_review::{
    BoundaryBehavior, OwnerReviewDecision, OwnerReviewRequestInput, ProposalKind,
    ResponsibilityLifecycleControl, ReviewFallbackBehavior, ReviewLimits,
};
use openspine_schemas::resolved_context::{ResolvedActionContext, ResolvedActionContextInput};
use openspine_schemas::responsibility::{
    ResponsibilityCompatibilityBinding, ResponsibilityManifest, ResponsibilityStatus,
};
use openspine_schemas::reviewed_scope::ReviewedActionScope;
use openspine_schemas::standing_rule::BudgetWindow;
use ulid::Ulid;

pub fn digest(c: char) -> Digest {
    Digest::parse(format!("sha256:{}", c.to_string().repeat(64))).unwrap()
}

pub fn descriptor() -> ActionDescriptor {
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

pub fn implementation() -> ActionImplementationDescriptor {
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

pub fn policy() -> DelegationPolicyBounds {
    descriptor().delegation_policy.unwrap()
}

pub fn context_input() -> ResolvedActionContextInput {
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

pub fn resolved_with(input: ResolvedActionContextInput) -> ResolvedActionContext {
    let descriptor = descriptor();
    let implementation = implementation();
    let action_id = descriptor.action_id.clone();
    let implementation_id = implementation.implementation_id.clone();
    let catalog = ActionCatalog::new([action_id.clone()])
        .with_egress_declarations([(action_id.clone(), ActionEgressDeclaration::default())])
        .with_delegation_descriptors([descriptor])
        .with_implementation_descriptors([implementation]);
    ResolvedActionContext::try_new(&catalog, &action_id, &implementation_id, input).unwrap()
}

pub fn resolved() -> ResolvedActionContext {
    resolved_with(context_input())
}

pub fn repeated_evidence(context_class_digest: Digest) -> DelegationEvidence {
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
    DelegationEvidence::repeated_approvals(context_class_digest, approvals).unwrap()
}

pub fn evidence_for(scope: &ReviewedActionScope) -> DelegationEvidence {
    repeated_evidence(scope.context_class_digest().clone())
}

pub fn explicit_owner_request_evidence() -> DelegationEvidence {
    DelegationEvidence::ExplicitOwnerRequest {
        schema_version: 1,
        decision_event_id: Ulid::from(103_u128),
        owner_principal_id: Ulid::from(7_u128),
        request_digest: digest('8'),
    }
}

pub fn limits() -> ReviewLimits {
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

pub fn review_input(
    reviewed_scope: ReviewedActionScope,
    evidence: DelegationEvidence,
) -> OwnerReviewRequestInput {
    OwnerReviewRequestInput {
        id: Ulid::from(200_u128),
        schema_version: 1,
        review_version: 1,
        proposal_kind: ProposalKind::Responsibility,
        evidence,
        title: "Prepare replies for this client".into(),
        description: "Create drafts in the reviewed mailbox and relationship scope.".into(),
        reviewed_scope,
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
        evaluation_binding: None,
    }
}

pub fn manifest(status: ResponsibilityStatus) -> ResponsibilityManifest {
    let context = resolved();
    let reviewed_scope = ReviewedActionScope::derive(&context).unwrap();
    let provenance_digest = evidence_for(&reviewed_scope)
        .evidence_set_digest()
        .unwrap()
        .clone();
    ResponsibilityManifest {
        id: "client_reply_drafts".into(),
        schema_version: 1,
        version: 1,
        status,
        workflow_id: "reply_workflow".into(),
        standing_rule_id: "client_reply_draft_rule".into(),
        reviewed_scope,
        limits: limits(),
        compatibility: ResponsibilityCompatibilityBinding {
            schema_version: 1,
            descriptor_version: context.descriptor_version(),
            implementation_version: context.implementation_version(),
            delegation_policy_version: context.delegation_policy_version(),
            workflow_version: 3,
            owner_review_binding_digest: digest('7'),
        },
        controls: BTreeSet::from([
            ResponsibilityLifecycleControl::Pause,
            ResponsibilityLifecycleControl::Resume,
            ResponsibilityLifecycleControl::Expire,
            ResponsibilityLifecycleControl::Revoke,
        ]),
        provenance_digest,
    }
}

use std::collections::{BTreeMap, BTreeSet};

use openspine_schemas::action::{
    validate_delegation_contract, ActionCatalog, ActionDescriptor, ActionEgressDeclaration,
    ActionId, ActionImplementationDescriptor, ActionImplementationId, ActionSemantics,
    BudgetWindowBounds, DarkWindowPolicy, DataDestination, DelegationDefaults,
    DelegationEligibilityError, DelegationPolicyBounds, DelegationProposalMode, EffectKind,
    EffectReversibility, ReviewedScopeDimension,
};
use openspine_schemas::briefcase::CounterpartyRef;
use openspine_schemas::delegation_evidence::{DelegationEvidence, OwnerApprovalEvidence};
use openspine_schemas::digest::Digest;
use openspine_schemas::egress::EgressClass;
use openspine_schemas::event::{AccountRole, TargetRef, TargetRefKind};
use openspine_schemas::resolved_context::{
    ResolvedActionContext, ResolvedActionContextError, ResolvedActionContextInput,
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
        descriptor_version: 2,
        action_id: ActionId::new("message.create_draft"),
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
            policy_version: 3,
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
        implementation_version: 4,
        action_id: ActionId::new("message.create_draft"),
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
            relationship: openspine_schemas::identity::RelationshipKind::Client,
        }),
        bound_parameters: BTreeMap::new(),
        target_digest: Some(digest('b')),
        payload_digest: Some(digest('c')),
        workflow_id: Some("reply_workflow".into()),
        task_shape_digest: Some(digest('d')),
    }
}

fn catalog_with(
    descriptor: ActionDescriptor,
    implementation: ActionImplementationDescriptor,
    egress: ActionEgressDeclaration,
) -> ActionCatalog {
    let action_id = descriptor.action_id.clone();
    ActionCatalog::new([action_id.clone()])
        .with_egress_declarations([(action_id, egress)])
        .with_delegation_descriptors([descriptor])
        .with_implementation_descriptors([implementation])
}

fn resolved(input: ResolvedActionContextInput) -> ResolvedActionContext {
    let descriptor = descriptor();
    let implementation = implementation();
    let action_id = descriptor.action_id.clone();
    let implementation_id = implementation.implementation_id.clone();
    let catalog = catalog_with(
        descriptor,
        implementation,
        ActionEgressDeclaration::default(),
    );
    ResolvedActionContext::try_new(&catalog, &action_id, &implementation_id, input).unwrap()
}

#[test]
fn incomplete_descriptor_is_not_delegatable() {
    let mut descriptor = descriptor();
    descriptor.semantics.owner_verb.clear();
    assert_eq!(
        validate_delegation_contract(&descriptor, &implementation()),
        Err(DelegationEligibilityError::IncompleteDescriptor {
            field: "owner_verb"
        })
    );
}

#[test]
fn communication_delegation_rejects_action_only_scope_and_allow_defaults() {
    let mut action_only = descriptor();
    action_only.required_scope_dimensions.clear();
    assert_eq!(
        validate_delegation_contract(&action_only, &implementation()),
        Err(DelegationEligibilityError::CommunicationScopeTooBroad)
    );

    let mut dark_allow = descriptor();
    dark_allow
        .delegation_policy
        .as_mut()
        .unwrap()
        .dark_window_policy = DarkWindowPolicy::BoundedAllow {
        maximum_timeout_secs: 600,
        maximum_outstanding: 1,
    };
    assert_eq!(
        validate_delegation_contract(&dark_allow, &implementation()),
        Err(DelegationEligibilityError::CommunicationDarkWindowAllowForbidden)
    );
}

#[test]
fn communication_scope_floor_covers_external_services_and_counterparties() {
    let mut external_service = descriptor();
    external_service.semantics.effect_kind = EffectKind::ReadOnly;
    external_service.semantics.destination = DataDestination::ExternalService;
    external_service
        .delegation_policy
        .as_mut()
        .unwrap()
        .dark_window_policy = DarkWindowPolicy::BoundedAllow {
        maximum_timeout_secs: 600,
        maximum_outstanding: 1,
    };
    assert_eq!(
        validate_delegation_contract(&external_service, &implementation()),
        Err(DelegationEligibilityError::CommunicationDarkWindowAllowForbidden)
    );

    let mut counterparty_communication = descriptor();
    counterparty_communication.semantics.effect_kind = EffectKind::CounterpartyCommunication;
    counterparty_communication
        .required_scope_dimensions
        .remove(&ReviewedScopeDimension::Counterparty);
    assert_eq!(
        validate_delegation_contract(&counterparty_communication, &implementation()),
        Err(DelegationEligibilityError::CommunicationScopeTooBroad)
    );
}

#[test]
fn invalid_policy_bounds_and_defaults_fail_closed() {
    let mut inverted_quota = descriptor();
    let policy = inverted_quota.delegation_policy.as_mut().unwrap();
    policy.quota.minimum_max = 21;
    policy.quota.maximum_max = 20;
    assert_eq!(
        validate_delegation_contract(&inverted_quota, &implementation()),
        Err(DelegationEligibilityError::InvalidPolicyBounds { field: "quota" })
    );

    let mut zero_policy_version = descriptor();
    zero_policy_version
        .delegation_policy
        .as_mut()
        .unwrap()
        .policy_version = 0;
    assert_eq!(
        validate_delegation_contract(&zero_policy_version, &implementation()),
        Err(DelegationEligibilityError::InvalidPolicyBounds {
            field: "policy_version"
        })
    );

    let mut quota_default_too_large = descriptor();
    quota_default_too_large
        .delegation_policy
        .as_mut()
        .unwrap()
        .defaults
        .as_mut()
        .unwrap()
        .quota
        .max = 21;
    assert_eq!(
        validate_delegation_contract(&quota_default_too_large, &implementation()),
        Err(DelegationEligibilityError::DefaultOutOfBounds { field: "quota" })
    );

    let mut lapse_default_too_large = descriptor();
    let policy = lapse_default_too_large.delegation_policy.as_mut().unwrap();
    policy.defaults.as_mut().unwrap().expires_after_secs = policy.maximum_lapse_secs + 1;
    assert_eq!(
        validate_delegation_contract(&lapse_default_too_large, &implementation()),
        Err(DelegationEligibilityError::DefaultOutOfBounds {
            field: "expires_after_secs"
        })
    );

    let mut explicit_limits_with_defaults = descriptor();
    explicit_limits_with_defaults
        .delegation_policy
        .as_mut()
        .unwrap()
        .proposal_mode = DelegationProposalMode::ExplicitLimitsRequired;
    assert_eq!(
        validate_delegation_contract(&explicit_limits_with_defaults, &implementation()),
        Err(DelegationEligibilityError::InvalidPolicyBounds { field: "defaults" })
    );
}

#[test]
fn synthetic_non_gmail_scope_matches_without_protocol_branches() {
    let context = resolved(context_input());
    let scope = ReviewedActionScope::derive(&context).unwrap();
    assert_eq!(scope.compare(&context), ScopeComparison::Matches);
}

#[test]
fn resolved_classifications_come_from_the_action_catalog() {
    let mut descriptor = descriptor();
    descriptor.required_scope_dimensions.extend([
        ReviewedScopeDimension::EgressClass,
        ReviewedScopeDimension::OutputChannel,
    ]);
    let implementation = implementation();
    let action_id = descriptor.action_id.clone();
    let implementation_id = implementation.implementation_id.clone();
    let catalog = catalog_with(
        descriptor,
        implementation,
        ActionEgressDeclaration {
            output_channels: Some(vec!["matrix.owner.draft".into()]),
            egress_class: Some(EgressClass::WebFormPost),
        },
    );

    let context =
        ResolvedActionContext::try_new(&catalog, &action_id, &implementation_id, context_input())
            .unwrap();
    assert_eq!(context.egress_class(), Some(EgressClass::WebFormPost));
    assert_eq!(
        context.output_channels(),
        &BTreeSet::from(["matrix.owner.draft".to_string()])
    );
    let scope = ReviewedActionScope::derive(&context).unwrap();
    assert!(scope
        .dimensions()
        .contains_key(&ReviewedScopeDimension::EgressClass));
    assert!(scope
        .dimensions()
        .contains_key(&ReviewedScopeDimension::OutputChannel));
}

#[test]
fn resolved_context_fails_closed_without_catalog_classifications() {
    let descriptor = descriptor();
    let implementation = implementation();
    let action_id = descriptor.action_id.clone();
    let implementation_id = implementation.implementation_id.clone();
    let catalog = ActionCatalog::new([action_id.clone()])
        .with_delegation_descriptors([descriptor])
        .with_implementation_descriptors([implementation]);
    assert_eq!(
        ResolvedActionContext::try_new(&catalog, &action_id, &implementation_id, context_input(),),
        Err(ResolvedActionContextError::MissingCatalogEgressDeclaration { action_id })
    );

    let mut descriptor = descriptor();
    descriptor
        .required_scope_dimensions
        .insert(ReviewedScopeDimension::DisclosureClass);
    let implementation = implementation();
    let action_id = descriptor.action_id.clone();
    let implementation_id = implementation.implementation_id.clone();
    let catalog = catalog_with(
        descriptor,
        implementation,
        ActionEgressDeclaration::default(),
    );
    assert_eq!(
        ResolvedActionContext::try_new(&catalog, &action_id, &implementation_id, context_input(),),
        Err(ResolvedActionContextError::MissingScopeDimension {
            dimension: ReviewedScopeDimension::DisclosureClass,
        })
    );
}

#[test]
fn scope_mismatch_names_exact_changed_dimensions() {
    let original = resolved(context_input());
    let scope = ReviewedActionScope::derive(&original).unwrap();

    let mut changed = context_input();
    changed.connector_instance_id = "matrix-secondary".into();
    changed.account_identity_digest = Some(digest('e'));
    changed.target_refs[0].id = Some("conversation-99".into());
    changed.workflow_id = Some("other_workflow".into());
    let changed = resolved(changed);
    let ScopeComparison::Mismatch { dimensions } = scope.compare(&changed) else {
        panic!("scope must mismatch");
    };
    assert_eq!(
        dimensions,
        BTreeSet::from([
            ReviewedScopeDimension::ConnectorInstance,
            ReviewedScopeDimension::AccountIdentity,
            ReviewedScopeDimension::Target,
            ReviewedScopeDimension::Workflow,
        ])
    );
}

#[test]
fn repeated_approval_evidence_binds_context_and_set_digest() {
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
    let evidence = DelegationEvidence::repeated_approvals(digest('5'), approvals).unwrap();
    assert!(evidence.supports_pattern_claim());
    assert_eq!(evidence.approval_count(), Some(2));
    assert!(evidence.evidence_set_digest().is_some());
}

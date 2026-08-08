//! Tests for owner-review resume revalidation and decision handling
//! (add-channel-neutral-responsibility-review, #129).

use std::collections::BTreeSet;

use jiff::Timestamp;
use openspine_schemas::action::{ActionId, ActionImplementationId};
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::briefcase::CounterpartyRef;
use openspine_schemas::delegation_evidence::DelegationEvidence;
use openspine_schemas::digest::Digest;
use openspine_schemas::event::{AccountRole, TargetRef, TargetRefKind};
use openspine_schemas::identity::RelationshipKind;
use openspine_schemas::owner_review::{
    BoundaryBehavior, OwnerReviewDecision, OwnerReviewRequest, OwnerReviewRequestInput,
    ProposalKind, ResponsibilityLifecycleControl, ReviewFallbackBehavior, ReviewLimits,
};
use openspine_schemas::resolved_context::{ResolvedActionContext, ResolvedActionContextInput};
use openspine_schemas::reviewed_scope::ReviewedActionScope;
use openspine_schemas::standing_rule::{BudgetWindow, DarkWindowConfig, StandingRuleManifest};
use ulid::Ulid;

use crate::gmail::GmailConnector;
use crate::pipeline::owner_review::resume_standing_rule_revalidated;
use crate::test_support::fixtures::{test_state, test_state_with_gmail};

fn digest(c: char) -> Digest {
    Digest::parse(format!("sha256:{}", c.to_string().repeat(64))).unwrap()
}

/// A `ResolvedActionContext` for `email.create_draft` resolved from the
/// canonical catalog, bound to a concrete instance.
fn email_context() -> ResolvedActionContext {
    email_context_with_target_count(1)
}

pub(super) fn email_context_with_target_count(target_count: usize) -> ResolvedActionContext {
    let catalog = crate::action_catalog::canonical_catalog();
    let action = ActionId::new("email.create_draft");
    let implementation = ActionImplementationId::new("gmail.draft.v1");
    let input = ResolvedActionContextInput {
        connector_instance_id: "gmail-primary".into(),
        account_role: Some(AccountRole::OwnerMailbox),
        account_identity_digest: Some(digest('a')),
        target_refs: (1..=target_count)
            .map(|index| TargetRef {
                kind: TargetRefKind::EmailThread,
                id: Some(format!("thread-{index}")),
            })
            .collect(),
        counterparty: Some(CounterpartyRef::Bound {
            identity_id: Ulid::from(11_u128),
            relationship: RelationshipKind::Client,
        }),
        bound_parameters: Default::default(),
        target_digest: Some(digest('b')),
        payload_digest: Some(digest('c')),
        workflow_id: Some("draft_reply_workflow".into()),
        task_shape_digest: Some(digest('d')),
    };
    ResolvedActionContext::try_new(&catalog, &action, &implementation, input).unwrap()
}

pub(super) fn owner_review(id: Ulid, principal: Ulid, description: String) -> OwnerReviewRequest {
    owner_review_with_target_count(id, principal, description, 1)
}

/// As [`owner_review_with_target_count`], but the stored review additionally
/// offers `extra` decisions — used to reach refusal arms that only fire once
/// an intent has passed the membership check.
pub(super) fn owner_review_offering(
    id: Ulid,
    principal: Ulid,
    description: String,
    extra: &[OwnerReviewDecision],
) -> OwnerReviewRequest {
    owner_review_inner(id, principal, description, 1, extra)
}

pub(super) fn owner_review_with_target_count(
    id: Ulid,
    principal: Ulid,
    description: String,
    target_count: usize,
) -> OwnerReviewRequest {
    owner_review_inner(id, principal, description, target_count, &[])
}

fn owner_review_inner(
    id: Ulid,
    principal: Ulid,
    description: String,
    target_count: usize,
    extra_decisions: &[OwnerReviewDecision],
) -> OwnerReviewRequest {
    let context = email_context_with_target_count(target_count);
    let scope = ReviewedActionScope::derive(&context).unwrap();
    let catalog = crate::action_catalog::canonical_catalog();
    let descriptor = catalog
        .delegation_descriptor_for(&ActionId::new("email.create_draft"))
        .unwrap();
    let policy = descriptor.delegation_policy.as_ref().unwrap();
    OwnerReviewRequest::try_new(
        OwnerReviewRequestInput {
            id,
            schema_version: 1,
            review_version: 1,
            proposal_kind: ProposalKind::Responsibility,
            evidence: DelegationEvidence::ExplicitOwnerRequest {
                schema_version: 1,
                decision_event_id: Ulid::new(),
                owner_principal_id: principal,
                request_digest: digest('8'),
            },
            title: "Prepare client replies".into(),
            description,
            reviewed_scope: scope,
            automatic_effects: vec!["Create a Gmail draft".into()],
            remaining_boundaries: vec!["Sending remains denied".into()],
            limits: ReviewLimits {
                quota: BudgetWindow {
                    max: 5,
                    window_secs: 7 * 24 * 3600,
                },
                rate: BudgetWindow {
                    max: 1,
                    window_secs: 3600,
                },
                expires_after_secs: 7 * 24 * 3600,
            },
            fallback_behavior: ReviewFallbackBehavior {
                scope_mismatch: BoundaryBehavior::RequireApproval,
                compatibility_drift: BoundaryBehavior::RequireApproval,
                budget_exhaustion: BoundaryBehavior::RequireApproval,
                timeout: BoundaryBehavior::Deny,
            },
            proposal_digest: digest('6'),
            compatibility_digest: context.compatibility_digest().clone(),
            available_decisions: BTreeSet::from([
                OwnerReviewDecision::Approve,
                OwnerReviewDecision::Reject,
                OwnerReviewDecision::Narrow,
            ])
            .into_iter()
            .chain(extra_decisions.iter().copied())
            .collect(),
            lifecycle_controls: BTreeSet::from([
                ResponsibilityLifecycleControl::Pause,
                ResponsibilityLifecycleControl::Resume,
                ResponsibilityLifecycleControl::Expire,
                ResponsibilityLifecycleControl::Revoke,
            ]),
        },
        policy,
    )
    .unwrap()
}

/// A scoped `StandingRuleManifest` for `email.create_draft` bound to a
/// specific context via its reviewed scope and compatibility epoch.
pub(super) fn scoped_manifest(id: &str, context: &ResolvedActionContext) -> StandingRuleManifest {
    let scope = ReviewedActionScope::derive(context).unwrap();
    let binding = openspine_schemas::standing_rule::ReviewedScopeBinding::derive_from(
        scope,
        context.compatibility_digest().clone(),
    );
    let mut m = manifest(
        id,
        "email.create_draft",
        7 * 24 * 3600,
        BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 1,
            window_secs: 3600,
        },
        None,
    );
    m.reviewed_scope = Some(binding);
    m
}

fn manifest(
    id: &str,
    action: &str,
    expires_after_secs: i64,
    quota: BudgetWindow,
    rate: BudgetWindow,
    dark_window: Option<DarkWindowConfig>,
) -> StandingRuleManifest {
    StandingRuleManifest {
        id: id.to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        action_id: ActionId::new(action),
        description: format!("standing rule {id} for {action}"),
        quota,
        rate,
        expires_after_secs,
        dark_window,
        reviewed_scope: None,
    }
}

fn budget() -> BudgetWindow {
    BudgetWindow {
        max: 2,
        window_secs: 3600,
    }
}

#[test]
fn resume_of_an_already_active_exact_version_is_a_safe_noop() {
    let state = test_state();
    let now = Timestamp::from_second(2_000_000).unwrap();
    let rule = scoped_manifest("resume-not-paused", &email_context());
    state
        .store
        .activate_standing_rule(&rule, None, now)
        .unwrap();
    // The exact version is already active: treat a duplicate/concurrent
    // resume as handled without emitting a refusal disposition.
    let resumed = resume_standing_rule_revalidated(&state, "resume-not-paused", 1, now).unwrap();
    assert!(!resumed);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("standing_rule.resume_refused_not_paused")
            .unwrap(),
        0
    );
}

#[test]
fn resume_refuses_an_expired_rule() {
    let state = test_state();
    let now = Timestamp::from_second(2_000_000).unwrap();
    let mut rule = scoped_manifest("resume-expired", &email_context());
    rule.expires_after_secs = 10; // expires 10s after activation
    state
        .store
        .activate_standing_rule(&rule, None, now)
        .unwrap();
    state
        .store
        .pause_standing_rule("resume-expired", Timestamp::now())
        .unwrap();
    // Advance past expiry.
    let later = Timestamp::from_second(2_000_100).unwrap();
    let resumed = resume_standing_rule_revalidated(&state, "resume-expired", 1, later).unwrap();
    assert!(!resumed);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("standing_rule.resume_refused_expired")
            .unwrap(),
        1
    );
}

#[test]
fn resume_refuses_a_superseded_rule() {
    let state = test_state();
    let now = Timestamp::from_second(2_000_000).unwrap();
    let v1 = scoped_manifest("resume-superseded", &email_context());
    state.store.activate_standing_rule(&v1, None, now).unwrap();
    state
        .store
        .pause_standing_rule("resume-superseded", Timestamp::now())
        .unwrap();
    // A v2 activation supersedes the paused v1 to revoked.
    let mut v2 = v1.clone();
    v2.version = 2;
    state.store.activate_standing_rule(&v2, None, now).unwrap();
    // Resume of v1 must refuse (it is no longer paused at v1).
    let resumed = resume_standing_rule_revalidated(&state, "resume-superseded", 1, now).unwrap();
    assert!(!resumed);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("standing_rule.resume_refused_superseded")
            .unwrap(),
        1
    );
}

#[test]
fn resume_refuses_a_rule_with_no_reviewed_scope() {
    let state = test_state();
    let now = Timestamp::from_second(2_000_000).unwrap();
    // A legacy unbounded rule (no reviewed scope) has nothing to revalidate,
    // so resume must refuse it as an invalid scope.
    let rule = manifest(
        "resume-no-scope",
        "email.send",
        3600,
        budget(),
        budget(),
        None,
    );
    state
        .store
        .activate_standing_rule(&rule, None, now)
        .unwrap();
    state
        .store
        .pause_standing_rule("resume-no-scope", Timestamp::now())
        .unwrap();
    let resumed = resume_standing_rule_revalidated(&state, "resume-no-scope", 1, now).unwrap();
    assert!(!resumed);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("standing_rule.resume_refused_invalid_scope")
            .unwrap(),
        1
    );
}

#[test]
fn resume_refuses_when_connector_is_unavailable() {
    let state = test_state_with_gmail(GmailConnector::new(
        "client-id".into(),
        "client-secret".into(),
        "refresh-token".into(),
        "owner@example.com".into(),
    ));
    let now = Timestamp::from_second(2_000_000).unwrap();
    let rule = scoped_manifest("resume-unavailable", &email_context());
    state
        .store
        .activate_standing_rule(&rule, None, now)
        .unwrap();
    state
        .store
        .pause_standing_rule("resume-unavailable", Timestamp::now())
        .unwrap();
    for _ in 0..10 {
        state.connectors.record_connector_outcome("gmail", false);
    }
    let resumed = resume_standing_rule_revalidated(&state, "resume-unavailable", 1, now).unwrap();
    assert!(!resumed);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("standing_rule.resume_refused_unavailable")
            .unwrap(),
        1
    );
}

#[test]
fn resume_reactivates_a_still_current_rule() {
    let state = test_state();
    let now = Timestamp::from_second(2_000_000).unwrap();
    let rule = scoped_manifest("resume-ok", &email_context());
    state
        .store
        .activate_standing_rule(&rule, None, now)
        .unwrap();
    state
        .store
        .pause_standing_rule("resume-ok", Timestamp::now())
        .unwrap();
    assert!(!state
        .store
        .standing_rule_is_current("resume-ok", 1)
        .unwrap());
    let resumed = resume_standing_rule_revalidated(&state, "resume-ok", 1, now).unwrap();
    assert!(resumed);
    assert!(state
        .store
        .standing_rule_is_current("resume-ok", 1)
        .unwrap());
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("standing_rule.resumed")
            .unwrap(),
        1
    );
}

/// The drift arm of resume: a rule whose bound compatibility epoch no longer
/// equals the catalog's current declaration axes MUST NOT reactivate. Bound to
/// a deliberately foreign epoch, so the refusal is about drift and not about a
/// malformed binding (the invalid-scope arm below covers that separately).
#[test]
fn resume_refuses_a_rule_whose_compatibility_epoch_drifted() {
    let state = test_state();
    let now = Timestamp::from_second(2_000_000).unwrap();
    let context = email_context();
    let scope = ReviewedActionScope::derive(&context).unwrap();
    let stale_epoch = digest('9');
    let current = state
        .action_catalog
        .compatibility_digest_for(&ActionId::new("email.create_draft"))
        .expect("the reviewed action declares a compatibility epoch");
    assert_ne!(
        &stale_epoch, &current,
        "the fixture epoch must actually differ from the catalog's"
    );
    let mut rule = manifest(
        "resume-drifted",
        "email.create_draft",
        7 * 24 * 3600,
        budget(),
        budget(),
        None,
    );
    rule.reviewed_scope = Some(
        openspine_schemas::standing_rule::ReviewedScopeBinding::derive_from(scope, stale_epoch),
    );
    state
        .store
        .activate_standing_rule(&rule, None, now)
        .unwrap();
    state
        .store
        .pause_standing_rule("resume-drifted", Timestamp::now())
        .unwrap();

    let resumed = resume_standing_rule_revalidated(&state, "resume-drifted", 1, now).unwrap();
    if resumed {
        panic!("a drifted epoch must not resume");
    }
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("standing_rule.resume_refused_scope_drift")
            .unwrap(),
        1
    );
    if state
        .store
        .standing_rule_is_current("resume-drifted", 1)
        .unwrap()
    {
        panic!("a refused resume must leave the rule out of live consultation");
    }
}

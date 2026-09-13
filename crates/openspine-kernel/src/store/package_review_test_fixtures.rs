use crate::pipeline::AppState;
use openspine_schemas::action::{ActionId, ActionImplementationId, ActionRequest};
use openspine_schemas::artifact::{ArtifactRef, Lifecycle};
use openspine_schemas::briefcase::CounterpartyRef;
use openspine_schemas::delegation_evidence::DelegationEvidence;
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::event::{AccountRole, TargetRef, TargetRefKind};
use openspine_schemas::identity::RelationshipKind;
use openspine_schemas::owner_review::*;
use openspine_schemas::resolved_context::{ResolvedActionContext, ResolvedActionContextInput};
use openspine_schemas::reviewed_scope::ReviewedActionScope;
use openspine_schemas::standing_rule::{BudgetWindow, ReviewedScopeBinding, StandingRuleManifest};
use std::collections::BTreeSet;
use std::time::Duration;
use ulid::Ulid;

fn at() -> Timestamp {
    "2099-01-01T12:00:00Z".parse().unwrap()
}

struct ReviewHarness {
    state: AppState,
    request: ActionRequest,
    review: OwnerReviewRequest,
    review_ref: ArtifactRef,
}

fn fixture(expires_at: Timestamp) -> ReviewHarness {
    let state = crate::test_support::fixtures::test_state();
    let principal = state.owner.principal_id.as_ulid();
    let action = ActionId::new("email.create_draft");
    let context = ResolvedActionContext::try_new(
        &state.action_catalog,
        &action,
        &ActionImplementationId::new("gmail.draft.v1"),
        ResolvedActionContextInput {
            connector_instance_id: "gmail-primary".into(),
            account_role: Some(AccountRole::OwnerMailbox),
            account_identity_digest: Some(digest_of_bytes(b"account")),
            target_refs: vec![TargetRef {
                kind: TargetRefKind::EmailThread,
                id: Some("census-thread".into()),
            }],
            counterparty: Some(CounterpartyRef::Bound {
                identity_id: Ulid::from(11_u128),
                relationship: RelationshipKind::Client,
            }),
            bound_parameters: Default::default(),
            target_digest: Some(digest_of_bytes(b"target")),
            payload_digest: Some(digest_of_bytes(b"draft")),
            workflow_id: Some("draft_reply_workflow".into()),
            task_shape_digest: Some(digest_of_bytes(b"shape")),
        },
    )
    .unwrap();
    let scope = ReviewedActionScope::derive(&context).unwrap();
    let limits = ReviewLimits {
        quota: BudgetWindow { max: 5, window_secs: 604800 },
        rate: BudgetWindow { max: 1, window_secs: 3600 },
        expires_after_secs: 604800,
    };
    let manifest = StandingRuleManifest {
        id: "census-rule".into(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Proposed,
        action_id: action.clone(),
        description: "Prepare replies within this scope".into(),
        quota: limits.quota,
        rate: limits.rate,
        expires_after_secs: limits.expires_after_secs,
        dark_window: None,
        reviewed_scope: Some(ReviewedScopeBinding::derive_from(
            scope.clone(), context.compatibility_digest().clone(),
        )),
    };
    let payload = state.artifacts.put(serde_yaml::to_string(&manifest).unwrap().as_bytes()).unwrap();
    let mut grant = crate::store::tests::sample_grant("census-review-grant");
    grant.user = state.owner.principal_id;
    grant.issued_at = at() - Duration::from_secs(120);
    grant.expires_at = at() - Duration::from_secs(60);
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    state.store.insert_task_grant(&grant, &payload, &state.owner_surface).unwrap();
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: "artifact.activate".into(),
        target_ref: None,
        payload_ref: Some(payload.clone()),
        target_digest: Some(digest_of_bytes(b"census-rule-v1")),
        selection_token_id: None,
        params: Default::default(),
        skill_attribution: None,
        requested_at: at() - Duration::from_secs(120),
        schema_version: 1,
    };
    state.store.insert_action_request(&request).unwrap();
    // Seed the result of evaluation; production rejection/expiry is exercised
    // below through the actual owner-decision handler, not a simulated outcome.
    state.store.with_conn_for_test(|conn| {
        conn.execute(
            "INSERT INTO proposed_artifacts
             (id, kind, artifact_id, version, state, yaml_digest, task_grant_id, action_request_id, proposed_at)
             VALUES (?1, 'standing_rule', 'census-rule', 1, 'review_required', ?2, ?3, ?4, ?5)",
            rusqlite::params![Ulid::new().to_string(), payload.digest.as_str(), grant.id.to_string(), request.id.to_string(), at().to_string()],
        ).unwrap();
    });
    let review = OwnerReviewRequest::try_new(
        OwnerReviewRequestInput {
            id: Ulid::new(), schema_version: 1, review_version: 1,
            proposal_kind: ProposalKind::StandingRule,
            evidence: DelegationEvidence::ExplicitOwnerRequest {
                schema_version: 1, decision_event_id: Ulid::new(),
                owner_principal_id: principal, request_digest: digest_of_bytes(b"owner request"),
            },
            title: "Prepare replies".into(), description: manifest.description,
            reviewed_scope: scope,
            automatic_effects: vec!["Create a draft".into()],
            remaining_boundaries: vec!["Sending stays blocked".into()],
            limits,
            fallback_behavior: ReviewFallbackBehavior {
                scope_mismatch: BoundaryBehavior::RequireApproval,
                compatibility_drift: BoundaryBehavior::RequireApproval,
                budget_exhaustion: BoundaryBehavior::RequireApproval,
                timeout: BoundaryBehavior::Deny,
            },
            proposal_digest: payload.digest.clone(),
            compatibility_digest: context.compatibility_digest().clone(),
            available_decisions: BTreeSet::from([OwnerReviewDecision::Approve, OwnerReviewDecision::Reject, OwnerReviewDecision::Narrow]),
            lifecycle_controls: BTreeSet::from([ResponsibilityLifecycleControl::Pause, ResponsibilityLifecycleControl::Revoke]),
            evaluation_binding: Some(OwnerReviewEvaluationBinding {
                artifact_kind: "standing_rule".into(), artifact_id: "census-rule".into(),
                artifact_version: 1, proposal_digest: payload.digest.clone(),
                action_request_id: request.id, replay_verdict_id: Ulid::new(), judge_verdict_id: Ulid::new(),
                epochs: OwnerReviewEvaluationEpochs {
                    proposal_digest: Some(payload.digest.clone()), compatibility_digest: None,
                    reviewed_scope_digest: None, evidence_set_digest: None,
                    descriptor_version: None, implementation_version: None, policy_version: None,
                },
            }),
        },
        state.action_catalog.delegation_descriptor_for(&action).unwrap().delegation_policy.as_ref().unwrap(),
    ).unwrap();
    let review_ref = persist(&state, &review, expires_at);
    ReviewHarness { state, request, review, review_ref }
}

fn persist(state: &AppState, review: &OwnerReviewRequest, expiry: Timestamp) -> ArtifactRef {
    let artifact = state.artifacts.put(&serde_json::to_vec(review).unwrap()).unwrap();
    state.store.insert_owner_review(review.id, &artifact, state.owner.principal_id.as_ulid(), expiry, at() - Duration::from_secs(120)).unwrap();
    artifact
}

fn decide(h: &ReviewHarness, intent: DecisionIntent, now: Timestamp) -> Result<crate::pipeline::owner_review_decision::OwnerReviewDecisionOutcome, crate::pipeline::owner_review_decision::OwnerReviewDecisionError> {
    crate::pipeline::owner_review_decision::submit_owner_review_decision(
        &h.state, &h.state.owner_surface, h.review.id, h.review.binding_digest(), intent, None, now,
    )
}

fn proposal_counts(h: &ReviewHarness, now: Timestamp) -> OutstandingWorkCounts {
    let before = h.state.store.all_audit_event_jsons().unwrap();
    let request_before = h.state.store.find_action_request(h.request.id).unwrap();
    let result = h.state.store.package_outstanding_work_with_reviews(now, &h.state.artifacts).unwrap();
    assert_eq!(h.state.store.all_audit_event_jsons().unwrap(), before);
    assert_eq!(h.state.store.find_action_request(h.request.id).unwrap(), request_before);
    assert_eq!(h.state.store.find_proposed_artifact("standing_rule", "census-rule", 1).unwrap().unwrap().state, Lifecycle::ReviewRequired);
    result.source(OutstandingWorkSource::ProposedArtifacts)
}

fn expected(outstanding: u64, terminal: u64, unknown: u64) -> OutstandingWorkCounts {
    OutstandingWorkCounts { outstanding, terminal, unknown }
}

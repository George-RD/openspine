//! Unit tests for the channel-neutral owner review contract. Extracted to a
//! sibling so `owner_review.rs` is not parked on the 500-line cap.

use super::*;

#[test]
fn legal_transition_table_is_enforced() {
    use OwnerReviewState::*;
    // The documented legal edges.
    assert!(can_transition(Pending, Approved));
    assert!(can_transition(Pending, Rejected));
    assert!(can_transition(Pending, Narrowed));
    assert!(can_transition(Pending, Expired));
    assert!(can_transition(Narrowed, Approved));
    assert!(can_transition(Narrowed, Rejected));
    assert!(can_transition(Narrowed, Narrowed));
    assert!(can_transition(Narrowed, Expired));
    assert!(can_transition(Approved, Expired));
    assert!(can_transition(Rejected, Expired));
    assert!(can_transition(Revoked, Expired));
    // Revoked is reachable from any non-terminal state.
    assert!(can_transition(Pending, Revoked));
    assert!(can_transition(Narrowed, Revoked));
    assert!(can_transition(Approved, Revoked));
    assert!(can_transition(Rejected, Revoked));
    // Expired is terminal.
    assert!(!can_transition(Expired, Pending));
    assert!(!can_transition(Expired, Approved));
    assert!(!can_transition(Expired, Revoked));
    // No illegal edges.
    assert!(!can_transition(Approved, Pending));
    assert!(!can_transition(Rejected, Approved));
    assert!(!can_transition(Approved, Narrowed));
    assert!(!can_transition(Rejected, Narrowed));
}

#[test]
fn decision_intent_maps_totally_onto_the_two_sets() {
    use DecisionIntent::*;
    // Every decision intent maps onto OwnerReviewDecision.
    assert_eq!(Approve.as_decision(), Some(OwnerReviewDecision::Approve));
    assert_eq!(Reject.as_decision(), Some(OwnerReviewDecision::Reject));
    assert_eq!(Narrow.as_decision(), Some(OwnerReviewDecision::Narrow));
    assert_eq!(Edit.as_decision(), Some(OwnerReviewDecision::Edit));
    assert_eq!(
        Pause.as_control(),
        Some(ResponsibilityLifecycleControl::Pause)
    );
    assert_eq!(
        Resume.as_control(),
        Some(ResponsibilityLifecycleControl::Resume)
    );
    assert_eq!(
        Expire.as_control(),
        Some(ResponsibilityLifecycleControl::Expire)
    );
    assert_eq!(
        Revoke.as_control(),
        Some(ResponsibilityLifecycleControl::Revoke)
    );
    assert!(Inspect.is_read_only());
    assert_eq!(Inspect.as_decision(), None);
    assert_eq!(Inspect.as_control(), None);
    assert!(!Approve.is_read_only());
}

#[test]
fn membership_check_gates_every_intent_except_inspect() {
    use DecisionIntent::*;
    let decisions: BTreeSet<OwnerReviewDecision> =
        [OwnerReviewDecision::Approve, OwnerReviewDecision::Reject]
            .into_iter()
            .collect();
    let controls: BTreeSet<ResponsibilityLifecycleControl> =
        [ResponsibilityLifecycleControl::Pause]
            .into_iter()
            .collect();
    assert!(Approve.is_permitted(&decisions, &controls));
    assert!(Reject.is_permitted(&decisions, &controls));
    assert!(Pause.is_permitted(&decisions, &controls));
    assert!(!Narrow.is_permitted(&decisions, &controls));
    assert!(!Edit.is_permitted(&decisions, &controls));
    assert!(!Resume.is_permitted(&decisions, &controls));
    assert!(!Expire.is_permitted(&decisions, &controls));
    assert!(!Revoke.is_permitted(&decisions, &controls));
    assert!(Inspect.is_permitted(&decisions, &controls));
    assert!(Inspect.is_permitted(&BTreeSet::new(), &BTreeSet::new()));
}
#[test]
fn legacy_owner_review_binding_digest_is_stable_for_schema_additions() {
    let review = legacy_owner_review_fixture();
    let expected =
        Digest::parse("sha256:85265a2739be2544180b28e8d6b06f082ba1a071fadb30d2ca7588a564ef9b78")
            .unwrap();
    assert_eq!(review.calculate_binding_digest(), expected);
    let encoded = serde_json::to_vec(&review).unwrap();
    assert!(!String::from_utf8(encoded.clone())
        .unwrap()
        .contains("evaluation_binding"));
    let decoded: OwnerReviewRequest = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded.calculate_binding_digest(), expected);
}

fn legacy_owner_review_fixture() -> OwnerReviewRequest {
    OwnerReviewRequest {
        id: Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
        schema_version: 1,
        review_version: 1,
        proposal_kind: ProposalKind::Responsibility,
        provenance: ProposalProvenance {
            schema_version: 1,
            kind: DelegationEvidenceKind::RepeatedApprovals,
            summary: "2 matching owner approvals".into(),
            evidence_digest: Digest::parse(
                "sha256:1111111111111111111111111111111111111111111111111111111111111111",
            )
            .unwrap(),
            evidence_count: 2,
        },
        title: "Legacy owner review".into(),
        description: "A review persisted before evaluation binding.".into(),
        reviewed_scope: serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "scope_version": 1,
            "action_id": "artifact.propose",
            "descriptor_version": 1,
            "dimensions": {
                "action": {
                    "kind": "action",
                    "value": "artifact.propose"
                },
                "descriptor": {
                    "kind": "descriptor_version",
                    "value": 1
                }
            },
            "context_class_digest":
                "sha256:2222222222222222222222222222222222222222222222222222222222222222"
        }))
        .unwrap(),
        automatic_effects: vec!["Persist a proposed artifact".into()],
        remaining_boundaries: vec!["Activation remains owner-controlled".into()],
        limits: ReviewLimits {
            quota: BudgetWindow {
                max: 5,
                window_secs: 604_800,
            },
            rate: BudgetWindow {
                max: 1,
                window_secs: 3_600,
            },
            expires_after_secs: 86_400,
        },
        fallback_behavior: ReviewFallbackBehavior {
            scope_mismatch: BoundaryBehavior::RequireApproval,
            compatibility_drift: BoundaryBehavior::RequireApproval,
            budget_exhaustion: BoundaryBehavior::RequireApproval,
            timeout: BoundaryBehavior::RequireApproval,
        },
        proposal_digest: Digest::parse(
            "sha256:3333333333333333333333333333333333333333333333333333333333333333",
        )
        .unwrap(),
        compatibility_digest: Digest::parse(
            "sha256:4444444444444444444444444444444444444444444444444444444444444444",
        )
        .unwrap(),
        available_decisions: [
            OwnerReviewDecision::Approve,
            OwnerReviewDecision::Reject,
            OwnerReviewDecision::Narrow,
        ]
        .into_iter()
        .collect(),
        lifecycle_controls: [
            ResponsibilityLifecycleControl::Pause,
            ResponsibilityLifecycleControl::Revoke,
        ]
        .into_iter()
        .collect(),
        evaluation_binding: None,
        binding_digest: Digest::parse(
            "sha256:85265a2739be2544180b28e8d6b06f082ba1a071fadb30d2ca7588a564ef9b78",
        )
        .unwrap(),
    }
}

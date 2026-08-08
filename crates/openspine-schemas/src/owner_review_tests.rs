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

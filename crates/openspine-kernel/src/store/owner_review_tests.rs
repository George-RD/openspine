//! Tests for the owner-review store (add-channel-neutral-responsibility-review,
//! #129). Covers persistence, the legal-transition table, idempotent
//! transitions, and the principal-bound decision audit.

use jiff::Timestamp;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use openspine_schemas::owner_review::{DecisionIntent, OwnerReviewState};
use ulid::Ulid;

use super::Store;

fn artifact_ref() -> ArtifactRef {
    ArtifactRef {
        digest: Digest::parse(
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap(),
        schema_version: 1,
    }
}

#[test]
fn owner_review_row_persists_and_round_trips() {
    let store = Store::open_in_memory().unwrap();
    let id = Ulid::new();
    let principal = Ulid::new();
    let now = Timestamp::now();
    let expires = now + jiff::Span::new().hours(1);
    store
        .insert_owner_review(id, &artifact_ref(), principal, expires, now)
        .unwrap();
    let row = store.owner_review_row(id).unwrap().expect("row exists");
    assert_eq!(row.id, id);
    assert_eq!(row.state, OwnerReviewState::Pending);
    assert_eq!(row.owner_principal_id, principal);
    assert_eq!(row.artifact_ref.digest, artifact_ref().digest);
    assert_eq!(row.expires_at, expires);
}

#[test]
fn owner_review_illegal_transition_is_refused() {
    let store = Store::open_in_memory().unwrap();
    let id = Ulid::new();
    let principal = Ulid::new();
    let now = Timestamp::now();
    let expires = now + jiff::Span::new().hours(1);
    store
        .insert_owner_review(id, &artifact_ref(), principal, expires, now)
        .unwrap();
    // Approved -> Pending is illegal.
    let err = store
        .transition_owner_review(
            id,
            OwnerReviewState::Approved,
            OwnerReviewState::Pending,
            "test",
        )
        .unwrap_err();
    assert!(err.to_string().contains("illegal owner-review transition"));
    // State unchanged.
    assert_eq!(
        store.owner_review_row(id).unwrap().unwrap().state,
        OwnerReviewState::Pending
    );
}

#[test]
fn owner_review_legal_transition_applies_and_is_idempotent() {
    let store = Store::open_in_memory().unwrap();
    let id = Ulid::new();
    let principal = Ulid::new();
    let now = Timestamp::now();
    let expires = now + jiff::Span::new().hours(1);
    store
        .insert_owner_review(id, &artifact_ref(), principal, expires, now)
        .unwrap();
    // Pending -> Approved is legal.
    assert!(store
        .transition_owner_review(
            id,
            OwnerReviewState::Pending,
            OwnerReviewState::Approved,
            "approve"
        )
        .unwrap());
    assert_eq!(
        store.owner_review_row(id).unwrap().unwrap().state,
        OwnerReviewState::Approved
    );
    // Re-applying the same transition is a no-op (already Approved).
    assert!(!store
        .transition_owner_review(
            id,
            OwnerReviewState::Pending,
            OwnerReviewState::Approved,
            "approve"
        )
        .unwrap());
}

#[test]
fn owner_review_revoked_reachable_from_any_non_terminal_state() {
    let store = Store::open_in_memory().unwrap();
    let id = Ulid::new();
    let principal = Ulid::new();
    let now = Timestamp::now();
    let expires = now + jiff::Span::new().hours(1);
    store
        .insert_owner_review(id, &artifact_ref(), principal, expires, now)
        .unwrap();
    // Pending -> Revoked is legal.
    assert!(store
        .transition_owner_review(
            id,
            OwnerReviewState::Pending,
            OwnerReviewState::Revoked,
            "revoke"
        )
        .unwrap());
    assert_eq!(
        store.owner_review_row(id).unwrap().unwrap().state,
        OwnerReviewState::Revoked
    );
}

#[test]
fn owner_review_decision_audit_records_principal_and_intent() {
    let store = Store::open_in_memory().unwrap();
    let id = Ulid::new();
    let principal = Ulid::new();
    let now = Timestamp::now();
    let expires = now + jiff::Span::new().hours(1);
    store
        .insert_owner_review(id, &artifact_ref(), principal, expires, now)
        .unwrap();
    let binding_digest = artifact_ref().digest;
    store
        .commit_owner_review_decision(
            id,
            DecisionIntent::Approve,
            principal,
            &binding_digest,
            None,
        )
        .unwrap();
    // The decision audit event is durable.
    let count = store
        .count_audit_events_of_kind("owner_review.decision")
        .unwrap();
    assert_eq!(count, 1);
}

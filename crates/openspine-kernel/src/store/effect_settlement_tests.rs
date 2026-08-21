//! Seam-shape tests for the two-phase settlement seam (#216, spec #208 D-003).
//!
//! These exercise `begin_effect` / `settle_effect` directly against a real
//! `Store`, asserting the durable fence + audit consequences of each
//! `EffectDisposition` transition. They prove the SHAPE; the connector-outcome
//! classification stays in Effect Truth #198.

use super::*;
use crate::api::effect_executors::EffectDisposition;

fn open_store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    let store = Store::open(&path).unwrap();
    (dir, store)
}

fn fence_inputs(id: ulid::Ulid, fingerprint: &str) -> PendingWriteFence<'_> {
    PendingWriteFence {
        id,
        grant_id: ulid::Ulid::new(),
        action_request_id: ulid::Ulid::new(),
        thread_id: "thread-1",
        request_fingerprint: fingerprint,
    }
}

fn begin(store: &Store, fingerprint: &str) -> EffectFence {
    let audit = AuditDescriptor::new("draft.pending_write_opened")
        .with_reason("fence claimed before the write");
    match store.begin_effect(fence_inputs(ulid::Ulid::new(), fingerprint), audit) {
        Ok(BeginEffect::Fenced(fence)) => fence,
        Ok(BeginEffect::AlreadyFenced) => panic!("expected Fenced, got AlreadyFenced"),
        Err(err) => panic!("begin_effect failed: {err:?}"),
    }
}

/// A `DeliveryUnknown`-style settlement retains + fences: the fence row stays
/// `pending` (no duplicate send is possible without operator reconciliation),
/// and both the begin and settle audit rows are recorded with a verifiable
/// chain. This is the acceptance criterion named on the ticket.
#[test]
fn retain_fenced_settlement_keeps_the_fence_open() {
    let (_dir, store) = open_store();
    let fence = begin(&store, "fp-retain");
    assert_eq!(store.count_pending_draft_writes().unwrap(), 1);

    let audit = AuditDescriptor::new("draft.delivery_unknown").with_reason("outcome unconfirmed");
    store
        .settle_effect(fence, EffectDisposition::DeliveryUnknown, audit)
        .unwrap();

    assert_eq!(
        store.count_pending_draft_writes().unwrap(),
        1,
        "a delivery-unknown settlement must leave the fence open"
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("draft.pending_write_opened")
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("draft.delivery_unknown")
            .unwrap(),
        1
    );
    assert!(store.verify_audit_chain().unwrap());
}

/// A `ConfirmedSuccess` settlement finalizes: the fence row is resolved and the
/// paired audit row is appended atomically.
#[test]
fn finalize_resolves_the_fence_and_audits() {
    let (_dir, store) = open_store();
    let fence = begin(&store, "fp-finalize");

    let audit = AuditDescriptor::new("draft.created");
    store
        .settle_effect(fence, EffectDisposition::ConfirmedSuccess, audit)
        .unwrap();

    assert_eq!(
        store.count_pending_draft_writes().unwrap(),
        0,
        "a confirmed success must resolve the fence"
    );
    assert_eq!(
        store.count_audit_events_of_kind("draft.created").unwrap(),
        1
    );
    assert!(store.verify_audit_chain().unwrap());
}

/// A `ConfirmedFailure` settlement cancels: the fence row is resolved (nothing
/// took hold, so nothing is retained) and the paired audit row is appended.
#[test]
fn cancel_resolves_the_fence_and_audits() {
    let (_dir, store) = open_store();
    let fence = begin(&store, "fp-cancel");

    let audit = AuditDescriptor::new("draft.creation_failed");
    store
        .settle_effect(fence, EffectDisposition::ConfirmedFailure, audit)
        .unwrap();

    assert_eq!(
        store.count_pending_draft_writes().unwrap(),
        0,
        "a confirmed failure must resolve the fence"
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("draft.creation_failed")
            .unwrap(),
        1
    );
}

/// `NotAttempted` is grouped with cancel: it also resolves the fence, covering
/// the fourth `EffectDisposition` variant even though the pilot never reaches
/// settlement with it.
#[test]
fn not_attempted_also_resolves_the_fence() {
    let (_dir, store) = open_store();
    let fence = begin(&store, "fp-not-attempted");

    let audit = AuditDescriptor::new("draft.not_attempted");
    store
        .settle_effect(fence, EffectDisposition::NotAttempted, audit)
        .unwrap();

    assert_eq!(store.count_pending_draft_writes().unwrap(), 0);
}

/// A second `begin_effect` for the same `request_fingerprint` while the first
/// fence is still open does not claim a second fence and writes no audit row,
/// preserving the concurrent-claim contract (D-050 TOCTOU closure).
#[test]
fn second_begin_for_same_fingerprint_does_not_claim() {
    let (_dir, store) = open_store();
    let _first = begin(&store, "fp-dup");

    let audit = AuditDescriptor::new("draft.pending_write_opened");
    let second = store
        .begin_effect(fence_inputs(ulid::Ulid::new(), "fp-dup"), audit)
        .unwrap();
    assert!(
        matches!(second, BeginEffect::AlreadyFenced),
        "a concurrent begin must lose the claim"
    );

    assert_eq!(
        store.count_pending_draft_writes().unwrap(),
        1,
        "the lost claim must not open a second fence"
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("draft.pending_write_opened")
            .unwrap(),
        1,
        "the lost claim must write no audit row"
    );
}

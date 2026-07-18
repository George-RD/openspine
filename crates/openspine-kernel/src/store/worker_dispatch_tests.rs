// openspine:allow-large-module reason: test submodule for worker_dispatch; split would break #[path] convention
//! Worker dispatch store tests (split out of `worker_dispatch.rs` to keep
//! that file under the 500-line gate). This is a submodule of
//! `worker_dispatch`, so `super::*` resolves to its items.

use super::*;
use crate::action_catalog::canonical_catalog;
use crate::store::tests::sample_grant;
use jiff::Timestamp;
use openspine_authority::worker_grant::mint_worker_grant;
use openspine_gate::{gate, ActionOrigin, NoEgress};
use openspine_schemas::action::{ActionId, ActionRequest, GateDecision};
use openspine_schemas::audit::AuditKind;
use openspine_schemas::briefcase::{CounterpartyRef, TaskClass, TaskShape};
use openspine_schemas::digest::{digest_of, Digest};
use openspine_schemas::event_bus::EventSubscriptionFilter;
use openspine_schemas::worker::{WorkerCommissionSpec, WorkerOutcome};

fn test_key() -> Vec<u8> {
    crate::grant_hmac_key().expect("test HMAC key present")
}

fn ref_of(byte: char) -> ArtifactRef {
    ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).unwrap(),
        schema_version: 1,
    }
}

fn request_digest() -> Digest {
    digest_of(&serde_json::json!({"purpose": "worker-task"}))
}

fn minimal_briefcase() -> Briefcase {
    Briefcase {
        schema_version: 1,
        task_shape: TaskShape {
            route_id: "owner_telegram_main_assistant".to_string(),
            workflow_id: "owner_control_conversation".to_string(),
            counterparty: CounterpartyRef::Unresolved {
                channel: "worker".to_string(),
                identifier: "worker-1".to_string(),
            },
        },
        source_snapshot_id: Digest::parse(format!("sha256:{}", "0".repeat(64))).unwrap(),
        depth: 1,
        tier: openspine_schemas::briefcase::RelationshipTier::Stranger,
        class: TaskClass::Conversation,
        sections: vec![],
        top_up_log: vec![],
    }
}

fn commission(store: &Store, parent: &TaskGrant) -> TaskGrant {
    let spec = WorkerCommissionSpec {
        agent_id: "worker_agent".to_string(),
        allowed_actions: vec![ActionId::new("openspine.status.read")],
        bound_parameters: vec![],
        expires_before: parent.expires_at,
        purpose: "worker-task".to_string(),
        route_id: parent.route_id.clone(),
        workflow_id: parent.workflow_id.clone(),
        capability_pack_id: parent.capability_pack_id.clone(),
        counterparty_channel: None,
        counterparty_identifier: None,
        task_class: TaskClass::Conversation,
    };
    let worker = mint_worker_grant(parent, &spec, &test_key()).expect("mint worker");
    record_worker_commissioned(
        store,
        parent.id,
        &worker,
        &ref_of('a'),
        &ref_of('b'),
        0,
        &minimal_briefcase(),
        &format!("receipt-{}", parent.id),
        &request_digest(),
    )
    .expect("commission persisted");
    worker
}

/// Build a shell `ActionRequest` against `grant_id` for `action` with empty
/// params — the shape the gate sees for a worker-invoked action.
fn gate_request(grant_id: Ulid, action: &str) -> ActionRequest {
    ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant_id,
        action: ActionId::new(action),
        target_ref: None,
        payload_ref: None,
        target_digest: None,
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        skill_attribution: None,
        requested_at: Timestamp::now(),
        schema_version: 1,
    }
}

/// Acceptance: a worker result returns as a *consumed bus event* — it is
/// recorded on the worker grant's audit aggregate (verifiable via the
/// event bus replay path) and carries the structured payload, not a
/// side-channel reply.
#[test]
fn result_is_consumed_bus_event() {
    let store = Store::open_in_memory().unwrap();
    let parent = sample_grant("worker-parent-token");
    let worker = commission(&store, &parent);
    // D-085: commissioning delivers the briefcase directly to the worker;
    // it must not create a shared task-board row.
    let board_rows: i64 = store
        .conn
        .lock()
        .query_row("SELECT COUNT(*) FROM task_board", [], |row| row.get(0))
        .unwrap();
    assert_eq!(board_rows, 0, "worker commissioning must bypass task_board");
    assert_eq!(
        worker_dispatch_state(&store, worker.id).unwrap(),
        Some(WorkerDispatchState::Dispatched),
        "commissioning opens a dispatched row"
    );

    let result = WorkerResult {
        outcome: WorkerOutcome::Completed,
        offered_slots: vec![],
        requests: vec![],
        notes_ref: None,
    };
    record_worker_result(&store, worker.id, &result).expect("result recorded");

    // The master consumes the result through the ordinary event bus.
    let filter = EventSubscriptionFilter::kinds([AuditKind::from_static("worker.result")]);
    let entries = store.replay_audit(&filter, 0).expect("replay");
    assert_eq!(entries[0].event.task_grant_id, Some(worker.id));
    let _payload: serde_json::Value =
        serde_json::from_str(entries[0].event.payload_json.as_deref().unwrap()).unwrap();
    assert_eq!(_payload["outcome"], "completed");
    assert_eq!(
        worker_dispatch_state(&store, worker.id).unwrap(),
        Some(WorkerDispatchState::Terminal),
        "recording a result flips the dispatch terminal"
    );

    // Receipt-keyed: a second result for the same dispatch is rejected,
    // never replayed (D-083 fail-closed).
    assert!(record_worker_result(&store, worker.id, &result).is_err());
}

/// Acceptance (offline chain verify, via the store's grant builder): a
/// freshly minted worker verifies offline under the same key, and a
/// second-level worker (worker-of-worker) verifies too, with no store
/// lookup involved in verification.
#[test]
fn worker_grant_verifies_offline() {
    let store = Store::open_in_memory().unwrap();
    let parent = sample_grant("worker-parent-token-2");
    let worker = commission(&store, &parent);
    assert!(worker.verify_mac(&test_key()), "worker verifies offline");

    let deeper_spec = WorkerCommissionSpec {
        agent_id: "deeper_agent".to_string(),
        allowed_actions: vec![ActionId::new("openspine.status.read")],
        bound_parameters: vec![],
        expires_before: worker.expires_at,
        purpose: "deeper-task".to_string(),
        route_id: worker.route_id.clone(),
        workflow_id: worker.workflow_id.clone(),
        capability_pack_id: worker.capability_pack_id.clone(),
        counterparty_channel: None,
        counterparty_identifier: None,
        task_class: TaskClass::Conversation,
    };
    let deeper = mint_worker_grant(&worker, &deeper_spec, &test_key()).expect("mint deeper");
    assert!(
        deeper.verify_mac(&test_key()),
        "worker-of-worker verifies offline"
    );
    // The store was only used to persist; verification is independent.
    let _ = store;
}

/// Blocker 3: a crash/retry that replays the SAME explicit receipt must not
/// mint a second grant or dispatch row — the commission is idempotent on the
/// receipt key, yet two *intentional* identical commissions (distinct
/// receipts) each land.
#[test]
fn commission_is_receipt_idempotent() {
    let store = Store::open_in_memory().unwrap();
    let parent = sample_grant("worker-parent-token-idem");
    let spec = WorkerCommissionSpec {
        agent_id: "worker_agent".to_string(),
        allowed_actions: vec![ActionId::new("openspine.status.read")],
        bound_parameters: vec![],
        expires_before: parent.expires_at,
        purpose: "worker-task".to_string(),
        route_id: parent.route_id.clone(),
        workflow_id: parent.workflow_id.clone(),
        capability_pack_id: parent.capability_pack_id.clone(),
        counterparty_channel: None,
        counterparty_identifier: None,
        task_class: TaskClass::Conversation,
    };
    let worker = mint_worker_grant(&parent, &spec, &test_key()).expect("mint worker");
    let receipt = format!("receipt-{}", parent.id);

    record_worker_commissioned(
        &store,
        parent.id,
        &worker,
        &ref_of('a'),
        &ref_of('b'),
        0,
        &minimal_briefcase(),
        &receipt,
        &request_digest(),
    )
    .expect("first commission persisted");

    let second = record_worker_commissioned(
        &store,
        parent.id,
        &worker,
        &ref_of('a'),
        &ref_of('b'),
        0,
        &minimal_briefcase(),
        &receipt,
        &request_digest(),
    );
    assert!(second.is_ok(), "idempotent commission must return Ok");

    assert_eq!(
        store.count_task_grants().unwrap(),
        1,
        "exactly one task_grants row after a duplicate receipt"
    );
    let dispatch_rows: i64 = store
        .conn
        .lock()
        .query_row("SELECT COUNT(*) FROM worker_dispatch", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        dispatch_rows, 1,
        "exactly one worker_dispatch row after a duplicate receipt"
    );
}

#[test]
fn commission_receipt_binding_rejects_different_parent_or_request() {
    let store = Store::open_in_memory().unwrap();
    let parent = sample_grant("worker-parent-binding");
    let worker = mint_worker_grant(
        &parent,
        &WorkerCommissionSpec {
            agent_id: "worker_agent".to_string(),
            allowed_actions: vec![ActionId::new("openspine.status.read")],
            bound_parameters: vec![],
            expires_before: parent.expires_at,
            purpose: "worker-task".to_string(),
            route_id: parent.route_id.clone(),
            workflow_id: parent.workflow_id.clone(),
            capability_pack_id: parent.capability_pack_id.clone(),
            counterparty_channel: None,
            counterparty_identifier: None,
            task_class: TaskClass::Conversation,
        },
        &test_key(),
    )
    .unwrap();
    let receipt = "receipt-binding";
    record_worker_commissioned(
        &store,
        parent.id,
        &worker,
        &ref_of('a'),
        &ref_of('b'),
        0,
        &minimal_briefcase(),
        receipt,
        &request_digest(),
    )
    .unwrap();
    let different_parent = sample_grant("worker-parent-other");
    assert_eq!(
        commissioned_grant_for_receipt(&store, different_parent.id, &request_digest(), receipt)
            .unwrap(),
        CommissionReceipt::Mismatch
    );
    assert_eq!(
        commissioned_grant_for_receipt(
            &store,
            parent.id,
            &digest_of(&serde_json::json!({"purpose": "different"})),
            receipt
        )
        .unwrap(),
        CommissionReceipt::Mismatch
    );
}

fn narrowed_gate_worker(parent: &TaskGrant) -> TaskGrant {
    let mut parent = parent.clone();
    parent.allowed_actions = vec![
        ActionId::new("openspine.status.read"),
        ActionId::new("worker.report_result"),
    ];
    parent.seal_root(&test_key());
    let spec = WorkerCommissionSpec {
        agent_id: "worker_agent".to_string(),
        allowed_actions: vec![ActionId::new("worker.report_result")],
        bound_parameters: vec![],
        expires_before: parent.expires_at,
        purpose: "worker-task".to_string(),
        route_id: parent.route_id.clone(),
        workflow_id: parent.workflow_id.clone(),
        capability_pack_id: parent.capability_pack_id.clone(),
        counterparty_channel: None,
        counterparty_identifier: None,
        task_class: TaskClass::Conversation,
    };
    mint_worker_grant(&parent, &spec, &test_key()).expect("mint worker")
}
/// A child is denied an action outside its narrowed allowlist.
#[test]
fn worker_denied_outside_narrowed_allowlist() {
    let store = Store::open_in_memory().unwrap();
    let mut parent = sample_grant("worker-parent-denied");
    parent.allowed_actions = vec![
        ActionId::new("openspine.status.read"),
        ActionId::new("worker.report_result"),
    ];
    parent.seal_root(&test_key());
    let parent_allowed = gate(
        &parent,
        &gate_request(parent.id, "openspine.status.read"),
        ActionOrigin::Shell,
        &store,
        &canonical_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert!(matches!(parent_allowed.decision, GateDecision::Allow));
    let worker = narrowed_gate_worker(&parent);
    let denied = gate(
        &worker,
        &gate_request(worker.id, "openspine.status.read"),
        ActionOrigin::Shell,
        &store,
        &canonical_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert!(matches!(denied.decision, GateDecision::Deny { .. }));
}

/// A child is allowed its exact report action when the parent grants it.
#[test]
fn worker_allowed_exact_report_action() {
    let store = Store::open_in_memory().unwrap();
    let worker = narrowed_gate_worker(&sample_grant("worker-parent-allowed"));
    let allowed = gate(
        &worker,
        &gate_request(worker.id, "worker.report_result"),
        ActionOrigin::Shell,
        &store,
        &canonical_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert!(matches!(allowed.decision, GateDecision::Allow));
}

/// An explicitly empty output-channel declaration stays empty after
/// commissioning, even when the parent carries a channel.
#[test]
fn classified_empty_output_channel_denial() {
    let mut parent = sample_grant("worker-parent-empty-output");
    parent.allowed_actions = vec![ActionId::new("openspine.status.read")];
    parent.output_channels = vec!["telegram.owner.reply".to_string()];
    parent.seal_root(&test_key());
    let spec = WorkerCommissionSpec {
        agent_id: "worker_agent".to_string(),
        allowed_actions: vec![ActionId::new("openspine.status.read")],
        bound_parameters: vec![],
        expires_before: parent.expires_at,
        purpose: "worker-task".to_string(),
        route_id: parent.route_id.clone(),
        workflow_id: parent.workflow_id.clone(),
        capability_pack_id: parent.capability_pack_id.clone(),
        counterparty_channel: None,
        counterparty_identifier: None,
        task_class: TaskClass::Conversation,
    };
    let worker = mint_worker_grant(&parent, &spec, &test_key()).expect("mint worker");
    assert!(
        !openspine_schemas::grant_chain::effectively_allows_output_channel(
            &worker,
            "telegram.owner.reply"
        ),
        "empty output-channel caveat must deny the parent channel"
    );
}

/// Blocker 7: commissioning bypasses `task_board` (dedicated assert), and a
/// replayed `worker.result` is receipt-keyed fail-closed (the second record
/// is rejected).
#[test]
fn replay_worker_result_is_receipt_keyed() {
    let store = Store::open_in_memory().unwrap();
    let worker = commission(&store, &sample_grant("worker-parent-replay"));
    let result = WorkerResult {
        outcome: WorkerOutcome::Completed,
        offered_slots: vec![],
        requests: vec![],
        notes_ref: None,
    };
    record_worker_result(&store, worker.id, &result).expect("first result recorded");
    assert!(record_worker_result(&store, worker.id, &result).is_err());
    let result_count: i64 = store
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE kind = 'worker.result'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        result_count, 1,
        "replay must emit exactly one worker.result event"
    );
}

#[test]
fn commissioning_persists_briefcase_without_board_row() {
    let store = Store::open_in_memory().unwrap();
    let worker = commission(&store, &sample_grant("worker-parent-board"));
    assert!(store.find_briefcase(worker.id).unwrap().is_some());
    let board_rows: i64 = store
        .conn
        .lock()
        .query_row("SELECT COUNT(*) FROM task_board", [], |row| row.get(0))
        .unwrap();
    assert_eq!(board_rows, 0, "worker commissioning must bypass task_board");
}

/// Regression (item 2): startup recovery is receipt-guarded. A dispatched
/// worker whose result has already been recorded is `terminal`, so
/// `pending_worker_dispatches` excludes it and it is never re-driven. A
/// still-dispatched (not yet receipted) row remains in the recovery set.
#[test]
fn receipted_worker_dispatch_is_not_recovered() {
    let store = Store::open_in_memory().unwrap();
    let worker = commission(&store, &sample_grant("worker-parent-recovery"));
    // Backdate the row so it is beyond the 100ms recovery grace window.
    store
        .conn
        .lock()
        .execute(
            "UPDATE worker_dispatch SET created_at = '2020-01-01T00:00:00Z' WHERE grant_id = ?1",
            params![worker.id.to_string()],
        )
        .unwrap();
    // Dispatched, no completion receipt yet -> must be pending recovery.
    let pending_before = pending_worker_dispatches(&store, Timestamp::now()).unwrap();
    assert_eq!(
        pending_before.len(),
        1,
        "dispatched row is pending recovery"
    );
    assert_eq!(pending_before[0].0, worker.id);

    // Record the result -> dispatch flips terminal (D-083 receipt-keyed flip).
    let result = WorkerResult {
        outcome: WorkerOutcome::Completed,
        offered_slots: vec![],
        requests: vec![],
        notes_ref: None,
    };
    record_worker_result(&store, worker.id, &result).expect("result recorded");
    assert_eq!(
        worker_dispatch_state(&store, worker.id).unwrap(),
        Some(WorkerDispatchState::Terminal),
        "recorded result flips dispatch terminal"
    );

    // Now excluded from recovery: an already-receipted row is never rerun.
    let pending_after = pending_worker_dispatches(&store, Timestamp::now()).unwrap();
    assert!(
        pending_after.is_empty(),
        "terminal (receipted) worker dispatch is skipped by recovery"
    );
}

/// Regression: `task.shell_failed` audit reason is a fixed class string, not
/// raw error text (D-012 plaintext discipline). The audit reason must never
/// contain a bearer token or other sensitive substring.
#[test]
fn shell_failed_audit_uses_fixed_reason() {
    let store = Store::open_in_memory().unwrap();
    let grant_id = Ulid::new();
    store
        .append_audit(
            "task.shell_failed",
            None,
            None,
            Some("worker shell failed"),
            Some(grant_id),
            &[],
            &[],
        )
        .unwrap();
    let reason: String = store
        .conn
        .lock()
        .query_row(
            "SELECT json_extract(meta_json, '$.reason') FROM audit_log WHERE kind = 'task.shell_failed'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        reason, "worker shell failed",
        "audit reason is fixed class string"
    );
    assert!(
        !reason.contains(grant_id.to_string().as_str()),
        "audit reason must not contain grant id: {reason}"
    );
    assert!(
        !reason.contains("token"),
        "audit reason must not contain token substring: {reason}"
    );
}

/// Regression: the watchdog sweep detects dispatched rows that are older than
/// the max age and have no result recorded (shell exited without reporting).
#[test]
fn stranded_worker_timeout_detects_expired() {
    let store = Store::open_in_memory().unwrap();
    let parent = sample_grant("watchdog-parent");
    let worker = commission(&store, &parent);
    // Backdate the row so it is beyond the watchdog threshold.
    store
        .conn
        .lock()
        .execute(
            "UPDATE worker_dispatch SET created_at = '2020-01-01T00:00:00Z' WHERE grant_id = ?1",
            params![worker.id.to_string()],
        )
        .unwrap();
    let stranded = stranded_worker_timeouts(&store, std::time::Duration::from_secs(3600)).unwrap();
    assert_eq!(
        stranded.len(),
        1,
        "old dispatched row is detected as stranded"
    );
    assert_eq!(stranded[0].0, worker.id);
    // After marking notified, it is no longer returned.
    mark_worker_stranded_notified(&store, worker.id).unwrap();
    let after = stranded_worker_timeouts(&store, std::time::Duration::from_secs(3600)).unwrap();
    assert!(after.is_empty(), "notified row is excluded from watchdog");
}

/// Regression: owner surfacing resolves the parent's bound chat and atomically
/// records the notification plus the recovery marker.
#[test]
fn stranded_worker_surface_uses_parent_bound_chat_atomically() {
    let store = Store::open_in_memory().unwrap();
    let parent = sample_grant("worker-parent-owner-chat");
    store.insert_task_grant(&parent, &ref_of('a'), 777).unwrap();
    let worker = commission(&store, &parent);
    store
        .conn
        .lock()
        .execute(
            "UPDATE worker_dispatch SET created_at = '2020-01-01T00:00:00Z' WHERE grant_id = ?1",
            params![worker.id.to_string()],
        )
        .unwrap();
    let parent_id = worker_parent_grant(&store, worker.id).unwrap().unwrap();
    let (_, _, chat_id) = store.find_task_grant_by_id(parent_id).unwrap().unwrap();
    assert_eq!(chat_id, 777);
    surface_stranded_worker(
        &store,
        chat_id,
        "sha256:owner-notification",
        worker.id,
        "stranded worker regression",
    )
    .unwrap();
    let (stored_chat, claimed): (i64, Option<String>) = store
        .conn
        .lock()
        .query_row(
            "SELECT chat_id, recovery_claimed_at FROM notify_dead_letters n \
             JOIN worker_dispatch w ON w.grant_id = n.task_grant_id \
             WHERE n.task_grant_id = ?1",
            params![worker.id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored_chat, 777);
    assert!(claimed.is_some(), "notification and marker commit together");
}

/// Regression: a failed conditional terminal transition rolls back the
/// notification insert, preserving retry eligibility without duplicates.
#[test]
fn stranded_worker_surface_rolls_back_when_dispatch_not_eligible() {
    let store = Store::open_in_memory().unwrap();
    let parent = sample_grant("worker-parent-owner-chat-rollback");
    store.insert_task_grant(&parent, &ref_of('b'), 888).unwrap();
    let worker = commission(&store, &parent);
    let result = WorkerResult {
        outcome: WorkerOutcome::Completed,
        offered_slots: vec![],
        requests: vec![],
        notes_ref: None,
    };
    record_worker_result(&store, worker.id, &result).unwrap();
    assert!(surface_stranded_worker(
        &store,
        888,
        "sha256:owner-notification",
        worker.id,
        "terminal worker regression",
    )
    .is_err());
    let count: i64 = store
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM notify_dead_letters WHERE task_grant_id = ?1",
            params![worker.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "failed terminal transition rolls back enqueue");
}

/// Regression (WkFinalSec P2): a fixed boot cutoff means a dispatch created
/// AFTER that cutoff is never returned as stranded, so a commission accepted
/// just after boot cannot be falsely surfaced/notified.
#[test]
fn pending_worker_dispatches_respects_fixed_cutoff() {
    let store = Store::open_in_memory().unwrap();
    let parent = sample_grant("cutoff-parent");
    let worker = commission(&store, &parent);

    // A cutoff in the past excludes the just-created dispatch.
    let past = Timestamp::now() - std::time::Duration::from_secs(3600);
    assert!(
        pending_worker_dispatches(&store, past).unwrap().is_empty(),
        "dispatches created after the cutoff are not stranded"
    );

    // A cutoff in the future includes it.
    let future = Timestamp::now() + std::time::Duration::from_secs(3600);
    let pending = pending_worker_dispatches(&store, future).unwrap();
    assert_eq!(
        pending.len(),
        1,
        "dispatches created before the cutoff are stranded"
    );
    assert_eq!(pending[0].0, worker.id);
}

/// Regression (advisory): surfacing a stranded worker with an unresolvable
/// owner chat (0) must reject without touching the dispatch row or enqueuing
/// any notification.
#[test]
fn surface_stranded_worker_rejects_zero_chat() {
    let store = Store::open_in_memory().unwrap();
    let worker = commission(&store, &sample_grant("zero-chat-parent"));
    let err = surface_stranded_worker(&store, 0, "ref", worker.id, "no owner chat");
    assert!(err.is_err(), "zero-owner-chat surfacing is rejected");
    let claimed: Option<String> = store
        .conn
        .lock()
        .query_row(
            "SELECT recovery_claimed_at FROM worker_dispatch WHERE grant_id = ?1",
            params![worker.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        claimed.is_none(),
        "zero-chat surfacing leaves row unclaimed"
    );
    let notifies: i64 = store
        .conn
        .lock()
        .query_row("SELECT COUNT(*) FROM notify_dead_letters", [], |r| r.get(0))
        .unwrap();
    assert_eq!(notifies, 0, "zero-chat surfacing enqueues no notification");
}

/// Regression (advisory): an enqueue failure must roll back the whole surface
/// transaction, leaving `recovery_claimed_at` NULL so the stranded worker
/// stays retryable on the next startup/watchdog sweep.
#[test]
fn surface_stranded_worker_rolls_back_on_enqueue_failure() {
    let store = Store::open_in_memory().unwrap();
    let worker = commission(&store, &sample_grant("enqueue-fail-parent"));
    // Force the notify insert to abort inside the surface transaction.
    store
        .conn
        .lock()
        .execute(
            "CREATE TRIGGER abort_notify BEFORE INSERT ON notify_dead_letters \
             BEGIN SELECT RAISE(ABORT, 'forced enqueue failure'); END",
            [],
        )
        .unwrap();
    let err = surface_stranded_worker(&store, 777, "ref", worker.id, "enqueue will fail");
    assert!(err.is_err(), "enqueue failure surfaces as an error");
    let claimed: Option<String> = store
        .conn
        .lock()
        .query_row(
            "SELECT recovery_claimed_at FROM worker_dispatch WHERE grant_id = ?1",
            params![worker.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        claimed.is_none(),
        "enqueue failure leaves row unclaimed (retryable)"
    );
}

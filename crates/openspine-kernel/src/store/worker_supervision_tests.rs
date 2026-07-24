//! Acceptance tests for AD-100 supervision and AD-102 identity addressing.

use super::*;
use crate::store::tests::sample_grant;
use crate::store::worker_dispatch::{
    record_worker_commissioned, record_worker_result, worker_dispatch_failed,
    worker_dispatch_state, WorkerDispatchState,
};
use jiff::Timestamp;
use openspine_authority::worker_grant::mint_worker_grant;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::briefcase::{
    Briefcase, CounterpartyRef, RelationshipTier, TaskClass, TaskShape,
};
use openspine_schemas::digest::{digest_of, Digest};
use openspine_schemas::event_bus::EventSubscriptionFilter;
use openspine_schemas::worker::{
    WorkerCommissionSpec, WorkerFailureReason, WorkerIdentity, WorkerOutcome, WorkerResult,
};
use ulid::Ulid;

fn key() -> Vec<u8> {
    crate::grant_hmac_key().expect("test HMAC key present")
}

fn artifact_ref(ch: char) -> ArtifactRef {
    ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", ch.to_string().repeat(64))).unwrap(),
        schema_version: 1,
    }
}

fn briefcase() -> Briefcase {
    Briefcase {
        schema_version: 1,
        task_shape: TaskShape {
            route_id: "owner_telegram_main_assistant".to_string(),
            workflow_id: "owner_control_conversation".to_string(),
            counterparty: CounterpartyRef::Unresolved {
                channel: "worker".to_string(),
                identifier: "worker".to_string(),
            },
        },
        source_snapshot_id: Digest::parse(format!("sha256:{}", "0".repeat(64))).unwrap(),
        depth: 1,
        tier: RelationshipTier::Stranger,
        class: TaskClass::Conversation,
        sections: vec![],
        top_up_log: vec![],
    }
}

fn commission(
    store: &Store,
    parent: &openspine_schemas::grant::TaskGrant,
    connector: &str,
) -> Result<openspine_schemas::grant::TaskGrant, StoreError> {
    let spec = WorkerCommissionSpec {
        agent_id: "worker_agent".to_string(),
        allowed_actions: vec![
            ActionId::new("openspine.status.read"),
            ActionId::new("worker.report_result"),
        ],
        bound_parameters: vec![],
        expires_before: parent.expires_at,
        purpose: "supervision-test-task".to_string(),
        route_id: parent.route_id.clone(),
        workflow_id: parent.workflow_id.clone(),
        capability_pack_id: parent.capability_pack_id.clone(),
        counterparty_channel: None,
        counterparty_identifier: None,
        task_class: TaskClass::Conversation,
    };
    let worker = mint_worker_grant(
        parent,
        &spec,
        &crate::action_catalog::canonical_catalog(),
        &key(),
    )
    .expect("mint worker");
    let identity = WorkerIdentity {
        owner: parent.user.clone(),
        conversation: parent.event_id.to_string(),
        task: worker.id.to_string(),
    };
    record_worker_commissioned(
        store,
        parent.id,
        &worker,
        &artifact_ref('a'),
        &artifact_ref('b'),
        42,
        &briefcase(),
        &format!("supervision-receipt-{}", worker.id),
        &digest_of(&serde_json::json!({"worker": worker.id.to_string()})),
        &identity,
        connector,
    )
    .map(|_| worker)
}

#[test]
fn worker_crash_emits_structured_failure_and_requires_recomposition() {
    let store = Store::open_in_memory().unwrap();
    let parent = sample_grant("supervision-parent");
    let worker = commission(&store, &parent, "flaky.connector").expect("commission");
    let now = Timestamp::now();
    // Establish the worker's claim before simulating its crash; failure
    // handling must reclaim this exact grant's slot.
    claim_conversation_in_flight(
        &store,
        &parent.user,
        &parent.event_id.to_string(),
        worker.id,
    )
    .unwrap();

    let failed = record_worker_failed(
        &store,
        worker.id,
        WorkerFailureReason::Crash,
        Some(&artifact_ref('d')),
        now,
        std::time::Duration::from_secs(30),
        3,
    )
    .expect("failure records");
    assert_eq!(failed.worker_grant_id, worker.id);
    assert_eq!(failed.parent_grant_id, parent.id);
    assert_eq!(failed.identity.task, worker.id.to_string());
    assert_eq!(failed.reason, WorkerFailureReason::Crash);
    assert!(failed.recomposition_permitted);
    assert_eq!(
        worker_dispatch_state(&store, worker.id).unwrap(),
        Some(WorkerDispatchState::Terminal)
    );
    assert!(worker_dispatch_failed(&store, worker.id).unwrap());
    let newer_holder = Ulid::new();
    claim_conversation_in_flight(
        &store,
        &parent.user,
        &parent.event_id.to_string(),
        newer_holder,
    )
    .unwrap();
    release_conversation_in_flight_for_grant(
        &store,
        &parent.user,
        &parent.event_id.to_string(),
        worker.id,
    )
    .unwrap();
    assert!(matches!(
        claim_conversation_in_flight(
            &store,
            &parent.user,
            &parent.event_id.to_string(),
            Ulid::new(),
        ),
        Err(StoreError::ConversationInFlight(_))
    ));
    release_conversation_in_flight_for_grant(
        &store,
        &parent.user,
        &parent.event_id.to_string(),
        newer_holder,
    )
    .unwrap();

    let events = store
        .replay_audit(
            &EventSubscriptionFilter::kinds([openspine_schemas::audit::AuditKind::from_static(
                "worker.failed",
            )]),
            0,
        )
        .unwrap();
    assert_eq!(events.len(), 1);
    let payload = events[0].event.payload_json.as_deref().unwrap();
    let decoded: openspine_schemas::worker::WorkerFailed = serde_json::from_str(payload).unwrap();
    assert_eq!(
        decoded, failed,
        "worker_failed is a structured event payload"
    );

    let late = WorkerResult {
        outcome: WorkerOutcome::Completed,
        offered_slots: vec![],
        requests: vec![],
        notes_ref: None,
    };
    assert!(record_worker_result(&store, worker.id, &late).is_err());

    let replacement = commission(&store, &parent, "flaky.connector").expect("recomposition");
    assert_ne!(replacement.id, worker.id);
    assert_eq!(
        store.count_task_grants().unwrap(),
        2,
        "dead worker + fresh grant"
    );
    let dispatch_rows: i64 = store
        .conn
        .lock()
        .query_row("SELECT COUNT(*) FROM worker_dispatch", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        dispatch_rows, 2,
        "recomposition creates a distinct dispatch row"
    );
}

#[test]
fn restart_caps_hold_under_flaky_connector() {
    let store = Store::open_in_memory().unwrap();
    let parent = sample_grant("flaky-parent");
    let window = std::time::Duration::from_secs(30);
    let base = Timestamp::now();
    let mut decisions = Vec::new();
    for offset in 0..3 {
        let worker = commission(&store, &parent, "flaky.connector").expect("commission under cap");
        let event = record_worker_failed(
            &store,
            worker.id,
            WorkerFailureReason::StartupFailure,
            None,
            base + std::time::Duration::from_secs(offset),
            window,
            3,
        )
        .expect("failure records");
        decisions.push(event.recomposition_permitted);
    }
    // The third failure hits the limit; its event truthfully reports that
    // continuation is now refused, and the commission boundary enforces it.
    assert_eq!(decisions, [true, true, false]);
    let held = commission(&store, &parent, "flaky.connector");
    assert!(matches!(held, Err(StoreError::WorkerRestartCapExceeded(_))));
    assert_eq!(
        store.count_task_grants().unwrap(),
        3,
        "refused commission mints no additional grant"
    );
    assert_eq!(
        connector_restart_count_in_window(
            &store,
            "flaky.connector",
            window,
            base + std::time::Duration::from_secs(4)
        )
        .unwrap(),
        3
    );
}

#[test]
fn identity_addressing_serializes_one_message_per_conversation() {
    let store = Store::open_in_memory().unwrap();
    let first = Ulid::new();
    let second = Ulid::new();
    claim_conversation_in_flight(&store, "owner-1", "conversation-1", first).unwrap();
    assert!(matches!(
        claim_conversation_in_flight(&store, "owner-1", "conversation-1", second),
        Err(StoreError::ConversationInFlight(_))
    ));
    claim_conversation_in_flight(&store, "owner-1", "conversation-2", second).unwrap();
    release_conversation_in_flight(&store, "owner-1", "conversation-1").unwrap();
    claim_conversation_in_flight(&store, "owner-1", "conversation-1", second).unwrap();
    release_conversation_in_flight_for_grant(&store, "owner-1", "conversation-1", first).unwrap();
    release_conversation_in_flight_for_grant(&store, "owner-1", "conversation-1", first).unwrap();
    assert!(matches!(
        claim_conversation_in_flight(&store, "owner-1", "conversation-1", first),
        Err(StoreError::ConversationInFlight(_))
    ));
    release_conversation_in_flight_for_grant(&store, "owner-1", "conversation-1", second).unwrap();
}

#[test]
fn legacy_unbound_failure_terminalizes_and_revokes_worker_token() {
    let store = Store::open_in_memory().unwrap();
    let worker_id = Ulid::new();
    let parent_id = Ulid::new();
    let now = Timestamp::now().to_string();
    store
        .conn
        .lock()
        .execute(
            "INSERT INTO worker_dispatch
             (grant_id, parent_grant_id, state, receipt_key, request_digest, token_ref,
              created_at, updated_at)
             VALUES (?1, ?2, 'dispatched', '', '', '', ?3, ?3)",
            rusqlite::params![worker_id.to_string(), parent_id.to_string(), now],
        )
        .unwrap();
    assert!(matches!(
        record_worker_failed(
            &store,
            worker_id,
            WorkerFailureReason::Crash,
            None,
            Timestamp::now(),
            std::time::Duration::from_secs(30),
            3,
        ),
        Err(StoreError::WorkerConnectorUnbound)
    ));
    assert!(worker_dispatch_failed(&store, worker_id).unwrap());
    let state: String = store
        .conn
        .lock()
        .query_row(
            "SELECT state FROM worker_dispatch WHERE grant_id = ?1",
            rusqlite::params![worker_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "terminal");
}

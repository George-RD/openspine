//! Worker result consumer tests, split out of `worker_result_consumer.rs`
//! to keep that file under the 500-line gate. `super::*` resolves to the
//! consumer module's items.

use super::*;
use crate::store::worker_dispatch::{record_worker_commissioned, record_worker_result};
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::test_state_with_telegram;
use jiff::Timestamp;
use openspine_authority::worker_grant::mint_worker_grant;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::{ArtifactRef, Lifecycle};
use openspine_schemas::briefcase::{Briefcase, CounterpartyRef, TaskClass, TaskShape};
use openspine_schemas::digest::{digest_of, Digest};
use openspine_schemas::grant::{GrantLimits, GrantMode, TaskGrant};
use openspine_schemas::worker::{
    WorkerCommissionSpec, WorkerIdentity, WorkerOutcome, WorkerResult,
};
use rusqlite::{params, OptionalExtension};
use serde_json::json;
use ulid::Ulid;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_briefcase() -> Briefcase {
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

fn artifact_ref(byte: char) -> ArtifactRef {
    ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).unwrap(),
        schema_version: 1,
    }
}

fn parent_grant() -> TaskGrant {
    let now = Timestamp::now();
    let mut grant = TaskGrant {
        id: Ulid::new(),
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user: "owner".to_string(),
        purpose: "test".to_string(),
        issued_by: "kernel".to_string(),
        issued_at: now,
        expires_at: now + std::time::Duration::from_secs(600),
        event_id: Ulid::new(),
        route_id: "owner_telegram_main_assistant".to_string(),
        agent_id: "main_assistant_agent".to_string(),
        workflow_id: "owner_control_conversation".to_string(),
        capability_pack_id: "owner_control_basic_pack".to_string(),
        authority_sources: vec![],
        selection_tokens: vec![],
        allowed_actions: vec![
            ActionId::new("openspine.status.read"),
            ActionId::new("telegram.reply:owner_channel"),
            ActionId::new("worker.report_result"),
        ],
        approval_required_actions: vec![],
        denied_actions: vec![],
        allowed_egress_classes: vec![],
        output_channels: vec!["telegram.owner.reply".to_string()],
        limits: GrantLimits {
            max_model_calls: 8,
            max_artifacts: 20,
            max_runtime_seconds: 120,
        },
        task_token: "consumer-parent-token".to_string(),
        root_grant_id: Ulid::nil(),
        parent_grant_id: None,
        mode: GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
        persona_id: None,
    };
    grant.root_grant_id = grant.id;
    grant.seal_root(&crate::grant_hmac_key().expect("test HMAC key"));
    grant
}

async fn commission_and_record(state: &AppState) -> ArtifactRef {
    let store = &state.store;
    let key = crate::grant_hmac_key().expect("test HMAC key");
    let parent = parent_grant();
    let parent_pending = state.artifacts.put(b"parent-pending").unwrap();
    store
        .insert_grant_and_briefcase_atomic(
            &parent,
            &parent_pending,
            state.owner_user_id,
            &test_briefcase(),
        )
        .unwrap();
    let spec = WorkerCommissionSpec {
        agent_id: "worker_agent".to_string(),
        allowed_actions: vec![
            ActionId::new("openspine.status.read"),
            ActionId::new("worker.report_result"),
        ],
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
    let worker = mint_worker_grant(
        &parent,
        &spec,
        &crate::action_catalog::canonical_catalog(),
        &key,
    )
    .expect("mint worker");
    let worker_pending = state.artifacts.put(b"worker-pending").unwrap();
    let worker_token = state.artifacts.put(worker.task_token.as_bytes()).unwrap();
    let request_digest = digest_of(&json!({"purpose": "worker-task"}));
    record_worker_commissioned(
        store,
        parent.id,
        &worker,
        &worker_pending,
        &worker_token,
        state.owner_user_id,
        &test_briefcase(),
        "receipt-consumer-test",
        &request_digest,
        &WorkerIdentity {
            owner: parent.user.clone(),
            conversation: parent.event_id.to_string(),
            task: worker.id.to_string(),
        },
        "test.connector",
    )
    .expect("commission persisted");
    let detail_ref = artifact_ref('d');
    let result = WorkerResult {
        outcome: WorkerOutcome::Completed,
        offered_slots: vec![openspine_schemas::worker::WorkerSlot {
            id: "slot-7".to_string(),
            label: "Preferred slot".to_string(),
        }],
        requests: vec![openspine_schemas::worker::WorkerRequest {
            kind: "approval".to_string(),
            detail_ref: Some(detail_ref.clone()),
        }],
        notes_ref: Some(detail_ref.clone()),
    };
    record_worker_result(store, worker.id, &result).expect("result recorded");
    detail_ref
}

fn worker_result_event_id_and_seq(state: &AppState) -> (String, i64) {
    state
        .store
        .conn
        .lock()
        .query_row(
            "SELECT id, seq FROM audit_log WHERE kind = 'worker.result' LIMIT 1",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )
        .unwrap()
}

fn checkpoint_last_acked(state: &AppState) -> Option<i64> {
    state
        .store
        .conn
        .lock()
        .query_row(
            "SELECT last_acked_global_seq FROM consumer_checkpoints WHERE consumer_id = 'worker_result_consumer'",
            [],
            |r| r.get(0),
        )
        .optional()
        .unwrap()
}

#[tokio::test]
async fn worker_result_consumer_relays_through_parent_gated_reply() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {"message_id": 1, "date": 0, "chat": {"id": 555, "type": "private"}, "text": "sent"}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let connector =
        TelegramConnector::with_api_url("test-token".to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);
    let detail_ref = commission_and_record(&state).await;
    worker_result_consumer_iteration(&state)
        .await
        .expect("consumer iteration relays through parent grant");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1, "exactly one owner relay is sent");
    let body = String::from_utf8_lossy(&requests[0].body);
    assert!(body.contains("slot-7"), "slot id survives relay: {body}");
    assert!(
        body.contains("approval"),
        "request kind survives relay: {body}"
    );
    assert!(
        body.contains(detail_ref.digest.as_str()),
        "digest survives relay: {body}"
    );
}

#[tokio::test]
async fn checkpoint_load_error_fails_closed_without_replay() {
    let connector = TelegramConnector::with_api_url(
        "test-token".to_string(),
        "http://127.0.0.1:1".parse().unwrap(),
    );
    let state = test_state_with_telegram(connector);
    state
        .store
        .conn
        .lock()
        .execute(
            "INSERT INTO consumer_checkpoints (consumer_id, last_acked_global_seq, checkpoint_json) VALUES (?1, 7, ?2)",
            params![CONSUMER_ID, "{not-json"],
        )
        .unwrap();
    let err = worker_result_consumer_iteration(&state).await.unwrap_err();
    assert!(err.to_string().contains("checkpoint load failed"));
}

/// Regression (item 1): a `worker.result` event whose relay marker is already
/// `delivered` (the durable post-send handoff committed on a prior run) is
/// skipped by the pre-send marker check on replay — the owner is never relayed
/// twice. The checkpoint is NOT advanced past an event the marker says was
/// already handled.
#[tokio::test]
async fn worker_result_relay_is_idempotent_on_replay() {
    let server = MockServer::start().await;
    // No send is ever allowed: if the consumer relayed, this test fails.
    Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {"message_id": 1, "date": 0, "chat": {"id": 555, "type": "private"}, "text": "sent"}
        })))
        .expect(0)
        .mount(&server)
        .await;
    let connector =
        TelegramConnector::with_api_url("test-token".to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);
    commission_and_record(&state).await;
    let (event_id, global_seq) = worker_result_event_id_and_seq(&state);

    // Simulate the post-send durable handoff having committed, but the
    // consumer checkpoint still behind (e.g. a second reader / replay).
    state
        .store
        .conn
        .lock()
        .execute(
            "INSERT OR REPLACE INTO worker_result_relays
             (event_id, global_seq, task_grant_id, state, attempts, last_error, created_at, updated_at)
             VALUES (?1, ?2, NULL, 'delivered', 1, NULL, ?3, ?3)",
            params![event_id, global_seq, Timestamp::now().to_string()],
        )
        .unwrap();

    worker_result_consumer_iteration(&state)
        .await
        .expect("consumer iteration must not error on an already-delivered marker");

    // No send happened, and the checkpoint was not advanced past the event.
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "already-delivered marker prevents a duplicate relay"
    );
    assert!(
        checkpoint_last_acked(&state).is_none() || checkpoint_last_acked(&state) == Some(0),
        "checkpoint not advanced past an already-delivered event"
    );
}

/// Regression (item 3): a transient relay failure retries up to 5 attempts
/// (durable attempt counter), then dead-letters with an audit receipt and only
/// then advances the checkpoint. The main loop is never terminated.
#[tokio::test]
async fn worker_result_relay_retries_then_dead_letters() {
    let server = MockServer::start().await;
    // Every relay attempt fails; the breaker may also open, but each failure
    // still counts as an attempt and is durable.
    Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"ok": false})))
        .mount(&server)
        .await;
    let connector =
        TelegramConnector::with_api_url("test-token".to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);
    commission_and_record(&state).await;
    let (_event_id, global_seq) = worker_result_event_id_and_seq(&state);

    for _ in 0..5 {
        worker_result_consumer_iteration(&state)
            .await
            .expect("iteration must not error on transient relay failure");
    }

    let dead_letters = state.store.worker_result_dead_letters().unwrap();
    assert_eq!(
        dead_letters.len(),
        1,
        "exactly one dead-letter after 5 attempts"
    );
    assert_eq!(
        dead_letters[0].1, 5,
        "dead-letter records the full 5-attempt durable counter"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("worker.result.relay_dead_letter")
            .unwrap(),
        1,
        "dead-letter audit receipt recorded exactly once"
    );
    assert_eq!(
        checkpoint_last_acked(&state),
        Some(global_seq),
        "checkpoint advances only after the dead-letter commit"
    );
    // Owner notification was enqueued via the failure_surfacing dead-letter path.
    let notify_dead: i64 = state
        .store
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM notify_dead_letters WHERE state != 'resolved'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        notify_dead >= 1,
        "dead-letter owner notification enqueued: {notify_dead}"
    );
}

/// Regression (item 2): when the artifact store put fails, the relay is left
/// retryable — no dead-letter is committed and the checkpoint is NOT advanced.
#[tokio::test]
async fn worker_result_relay_artifact_put_failure_stays_retryable() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"ok": false})))
        .mount(&server)
        .await;
    let connector =
        TelegramConnector::with_api_url("test-token".to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);
    commission_and_record(&state).await;
    let (_event_id, _global_seq) = worker_result_event_id_and_seq(&state);

    // Inject an artifact-store put failure for THIS store only (per-instance).
    state.artifacts.set_fault_put_for_test(true);
    let result = worker_result_consumer_iteration(&state).await;
    state.artifacts.set_fault_put_for_test(false);
    result.expect("iteration must not error on artifact put failure");

    assert!(
        state.store.worker_result_dead_letters().unwrap().is_empty(),
        "artifact-put failure must not commit a dead-letter"
    );
    assert_eq!(
        checkpoint_last_acked(&state),
        None,
        "checkpoint must stay unadvanced while the relay is retryable"
    );
}

/// Regression (item 3 + advisory): when the resolved owner chat is 0 (no
/// resolvable owner), the relay is left retryable — no dead-letter is committed
/// and the checkpoint is NOT advanced.
#[tokio::test]
async fn worker_result_relay_unresolvable_owner_stays_retryable() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"ok": false})))
        .mount(&server)
        .await;
    let connector =
        TelegramConnector::with_api_url("test-token".to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);
    commission_and_record(&state).await;
    // Force the parent grant's bound chat to 0 so the owner is unresolvable.
    state
        .store
        .conn
        .lock()
        .execute(
            "UPDATE task_grants SET bound_chat_id = 0 WHERE bound_chat_id = ?1",
            params![state.owner_user_id],
        )
        .unwrap();
    let (_event_id, _global_seq) = worker_result_event_id_and_seq(&state);

    // Drive five attempts (the normal dead-letter threshold). With chat 0 the
    // relay is left retryable every time, so even at attempt 5 no dead-letter
    // is committed and the checkpoint never advances.
    for _ in 0..5 {
        let result = worker_result_consumer_iteration(&state).await;
        result.expect("iteration must not error on unresolvable owner");
    }

    assert!(
        state.store.worker_result_dead_letters().unwrap().is_empty(),
        "unresolvable owner (chat 0) must not commit a dead-letter"
    );
    assert_eq!(
        checkpoint_last_acked(&state),
        None,
        "checkpoint must stay unadvanced while the relay is retryable"
    );
}

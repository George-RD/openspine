use super::*;
use crate::test_support::fixtures::test_state;
use openspine_schemas::event_bus::ConsumerCheckpoint;
use std::time::Duration;

#[tokio::test]
async fn armed_secret_intake_blocks_until_normal_capture_consumes_it() {
    let state = test_state();
    let owner_surface = crate::test_support::owner_surface_for(&state, 42);
    let proof = crate::identity::OwnerVerifiedProof::test_new();
    assert!(crate::secret_intake::arm(
        &state,
        &owner_surface,
        state.owner.principal_id.as_ulid(),
        &proof,
        crate::secret_intake::SecretMode::Intake,
        "census.credential",
    )
    .unwrap());
    let pending = state.store.get_kv("secret.intake.pending").unwrap();
    assert!(pending.is_some());
    let snapshot = state.store.package_outstanding_work(Timestamp::now()).unwrap();
    assert!(!snapshot.is_quiescent(), "armed credential writes must block");
    assert_eq!(state.store.get_kv("secret.intake.pending").unwrap(), pending);
    assert!(state.secrets.get("census.credential").unwrap().is_none());

    assert_eq!(
        crate::secret_intake::capture(&state, &owner_surface, "test-only-value")
            .await
            .unwrap(),
        Some(crate::secret_intake::CaptureOutcome::Stored(
            crate::secret_intake::SecretMode::Intake
        ))
    );
    assert!(state.store.get_kv("secret.intake.pending").unwrap().is_none());
    assert!(state.store.package_outstanding_work(Timestamp::now()).unwrap().is_quiescent());
}

#[test]
fn malformed_secret_intake_cannot_look_quiescent_or_be_consumed_by_review() {
    let store = Store::open_in_memory().unwrap();
    store.set_kv("secret.intake.pending", "not-json").unwrap();
    assert!(!store.package_outstanding_work(Timestamp::now()).unwrap().is_quiescent());
    assert_eq!(store.get_kv("secret.intake.pending").unwrap().as_deref(), Some("not-json"));
}

fn seed_timer_backlog(store: &Store, payload: Option<&str>) -> u64 {
    store.with_conn_for_test(|conn| {
        Store::append_audit_conn_with_options(
            conn, "workflow.timer_fired", None, None, None, None,
            &[], &[], Some("system"), payload,
        ).unwrap();
    });
    let filter = EventSubscriptionFilter::kinds([AuditKind::from_static("workflow.timer_fired")]);
    let seq = store.replay_audit(&filter, 0).unwrap().pop().unwrap().global_seq;
    // The independent task consumer has already dealt with its copy.
    store.save_consumer_checkpoint(
        "task_board_timer_consumer",
        &PersistedConsumerState {
            schema_version: 1,
            checkpoint: ConsumerCheckpoint { schema_version: 1, last_acked_global_seq: seq },
            filter,
        },
    ).unwrap();
    seq
}

fn assert_dark_window_ack(store: &Store, seq: u64) {
    let saved = store.load_consumer_checkpoint("standing_rule_dark_window_consumer")
        .unwrap().expect("the runtime must persist even a skipped event");
    assert_eq!(saved.checkpoint.last_acked_global_seq, seq);
    assert_eq!(
        store.package_outstanding_work(Timestamp::now()).unwrap()
            .source(OutstandingWorkSource::EventConsumerBacklog),
        OutstandingWorkCounts::default()
    );
}

#[tokio::test]
async fn skipped_timer_acknowledgement_survives_consumer_restart() {
    let state = test_state();
    let seq = seed_timer_backlog(&state.store, None);
    assert_eq!(
        state.store.package_outstanding_work(Timestamp::now()).unwrap()
            .source(OutstandingWorkSource::EventConsumerBacklog).outstanding,
        1
    );
    // Empty recovery state and an unowned timer do not await effects. The
    // real consumer completes this tick before its first one-second sleep.
    assert!(tokio::time::timeout(
        Duration::from_millis(50),
        crate::pipeline::run_standing_rule_dark_window_consumer(&state),
    ).await.is_err());
    assert_dark_window_ack(&state.store, seq);
    assert!(tokio::time::timeout(
        Duration::from_millis(50),
        crate::pipeline::run_standing_rule_dark_window_consumer(&state),
    ).await.is_err());
    assert_dark_window_ack(&state.store, seq);
}

#[tokio::test]
async fn failed_timer_checkpoint_is_retried_without_a_new_event() {
    let state = test_state();
    let seq = seed_timer_backlog(&state.store, Some(r#"{"timer_id":"not-a-standing-rule"}"#));
    state.store.with_conn_for_test(|conn| {
        conn.execute_batch(
            "CREATE TRIGGER reject_census_checkpoint BEFORE INSERT ON consumer_checkpoints
             WHEN NEW.consumer_id = 'standing_rule_dark_window_consumer'
             BEGIN SELECT RAISE(ABORT, 'injected checkpoint failure'); END;",
        ).unwrap();
    });
    let mut consumer = Box::pin(crate::pipeline::run_standing_rule_dark_window_consumer(&state));
    assert!(tokio::time::timeout(Duration::from_millis(50), consumer.as_mut()).await.is_err());
    assert!(state.store.load_consumer_checkpoint("standing_rule_dark_window_consumer")
        .unwrap().is_none());
    state.store.with_conn_for_test(|conn| {
        conn.execute_batch("DROP TRIGGER reject_census_checkpoint").unwrap();
    });
    // Keep the same live consumer: an in-memory watermark advanced before
    // persistence would strand the acknowledgement until a later event.
    assert!(tokio::time::timeout(Duration::from_millis(1200), consumer.as_mut()).await.is_err());
    assert_dark_window_ack(&state.store, seq);
}

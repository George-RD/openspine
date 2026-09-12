fn seed_task_timer_backlog(store: &Store, timer: &str) -> u64 {
    store.with_conn_for_test(|conn| {
        Store::append_audit_conn_with_options(
            conn,
            "workflow.timer_fired",
            None,
            None,
            None,
            None,
            &[],
            &[],
            Some("system"),
            Some(&format!(r#"{{"timer_id":"{timer}"}}"#)),
        )
        .unwrap();
    });
    let filter = EventSubscriptionFilter::kinds([AuditKind::from_static("workflow.timer_fired")]);
    let seq = store.replay_audit(&filter, 0).unwrap().pop().unwrap().global_seq;
    store.save_consumer_checkpoint(
        "standing_rule_dark_window_consumer",
        &PersistedConsumerState {
            schema_version: 1,
            checkpoint: ConsumerCheckpoint { schema_version: 1, last_acked_global_seq: seq },
            filter,
        },
    ).unwrap();
    seq
}

fn reject_task_checkpoint(store: &Store) {
    store.with_conn_for_test(|conn| {
        conn.execute_batch(
            "CREATE TRIGGER reject_task_checkpoint BEFORE INSERT ON consumer_checkpoints
             WHEN NEW.consumer_id = 'task_board_timer_consumer'
             BEGIN SELECT RAISE(ABORT, 'injected checkpoint failure'); END;",
        ).unwrap();
    });
}

#[tokio::test]
async fn failed_task_checkpoint_retries_in_same_consumer_without_new_event() {
    let state = test_state();
    let seq = seed_task_timer_backlog(&state.store, "not-a-task");
    reject_task_checkpoint(&state.store);
    let mut consumer = Box::pin(crate::pipeline::run_task_deadline_consumer(&state));
    assert!(tokio::time::timeout(Duration::from_millis(50), consumer.as_mut()).await.is_err());
    assert!(state.store.load_consumer_checkpoint("task_board_timer_consumer").unwrap().is_none());
    state.store.with_conn_for_test(|conn| {
        conn.execute_batch("DROP TRIGGER reject_task_checkpoint").unwrap();
    });
    assert!(tokio::time::timeout(Duration::from_millis(1200), consumer.as_mut()).await.is_err());
    let saved = state.store.load_consumer_checkpoint("task_board_timer_consumer")
        .unwrap().expect("the same consumer must retry its failed durable acknowledgement");
    assert_eq!(saved.checkpoint.last_acked_global_seq, seq);
    assert_eq!(
        state.store.package_outstanding_work(Timestamp::now()).unwrap()
            .source(OutstandingWorkSource::EventConsumerBacklog),
        OutstandingWorkCounts::default()
    );
}

#[tokio::test]
async fn failed_task_checkpoint_stops_before_later_events_in_batch() {
    let state = test_state();
    for timer in ["first-unowned", "second-unowned"] {
        seed_task_timer_backlog(&state.store, timer);
    }
    reject_task_checkpoint(&state.store);
    let mut consumer = Box::pin(crate::pipeline::run_task_deadline_consumer(&state));
    assert!(tokio::time::timeout(Duration::from_millis(50), consumer.as_mut()).await.is_err());
    state.store.with_conn_for_test(|conn| {
        let handled: i64 = conn.query_row("SELECT COUNT(*) FROM dispatch_state", [], |r| r.get(0)).unwrap();
        assert_eq!(handled, 1, "later events must not overtake a failed acknowledgement");
    });
    assert_eq!(
        state.store.package_outstanding_work(Timestamp::now()).unwrap()
            .source(OutstandingWorkSource::EventConsumerBacklog),
        OutstandingWorkCounts { outstanding: 2, terminal: 0, unknown: 0 }
    );
}

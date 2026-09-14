use super::*;

#[test]
fn backlog_count_matches_validated_replay_for_each_filter_and_watermark() {
    let store = Store::open_in_memory().unwrap();
    for (kind, aggregate) in [("worker.failed", "a"), ("worker.result", "b"), ("worker.failed", "b")] {
        store.with_immediate_tx(|tx| {
            Store::append_audit_conn_with_options(tx, kind, None, None, None, None, &[], &[], Some(aggregate), None)?;
            Ok(())
        }).unwrap();
    }
    for kinds in [None, Some(vec![]), Some(vec![AuditKind::from_static("worker.failed")]), Some(vec![AuditKind::from_static("worker.failed"), AuditKind::from_static("worker.result")])] {
        for aggregate_id in [None, Some("a".to_string()), Some("b".to_string()), Some("missing".to_string())] {
            let filter = EventSubscriptionFilter { schema_version: 1, kinds: kinds.clone(), aggregate_id };
            for after in 0..=4 {
                let replay = store.replay_audit(&filter, after).unwrap();
                let count = store.with_deferred_read(|tx| matching_backlog(tx, &filter, after)).unwrap();
                assert_eq!(count, i64::try_from(replay.len()).unwrap());
            }
        }
    }
}

#[test]
fn backlog_count_validates_nonmatching_kind_before_filtering() {
    for column in ["event_json", "meta_json"] {
        let store = Store::open_in_memory().unwrap();
        store.append_audit("census.unrelated", None, None, None, None, &[], &[]).unwrap();
        store.append_audit("worker.failed", None, None, None, None, &[], &[]).unwrap();
        store.with_conn_for_test(|conn| {
            let sql = format!("UPDATE audit_log SET {column} = '{{}}' WHERE seq = 1");
            conn.execute(&sql, []).unwrap();
        });
        let filter = EventSubscriptionFilter::kinds([AuditKind::from_static("worker.failed")]);
        assert!(store.replay_audit(&filter, 0).is_err());
        assert!(store.with_deferred_read(|tx| matching_backlog(tx, &filter, 0)).is_err());
        // The checkpoint is exclusive. Older rows are not part of this scan;
        // full chain verification belongs to the maintenance entry boundary.
        assert_eq!(store.with_deferred_read(|tx| matching_backlog(tx, &filter, 1)).unwrap(), 1);
    }
}

#[test]
fn large_backlog_is_counted_exactly_without_changing_rows() {
    let store = Store::open_in_memory().unwrap();
    store.with_immediate_tx(|tx| {
        for _ in 0..1024 {
            Store::append_audit_conn_with_options(tx, "worker.failed", None, None, None, None, &[], &[], None, None)?;
        }
        Ok(())
    }).unwrap();
    let before = store.all_audit_event_jsons().unwrap();
    let snapshot = store.package_outstanding_work(Timestamp::now()).unwrap();
    assert_eq!(snapshot.source(OutstandingWorkSource::EventConsumerBacklog), OutstandingWorkCounts { outstanding: 1024, terminal: 0, unknown: 0 });
    assert_eq!(store.all_audit_event_jsons().unwrap(), before);
}

#[test]
fn backlog_watermark_outside_sql_range_fails_closed() {
    let store = Store::open_in_memory().unwrap();
    assert!(matches!(
        store.with_deferred_read(|tx| matching_backlog(tx, &EventSubscriptionFilter::all(), u64::MAX)),
        Err(StoreError::NumericRange)
    ));
}

fn save_test_checkpoint(store: &Store, filter: &EventSubscriptionFilter, seq: u64) {
    store.save_consumer_checkpoint(
        "worker_failed_consumer",
        &PersistedConsumerState {
            schema_version: 1,
            checkpoint: openspine_schemas::event_bus::ConsumerCheckpoint {
                schema_version: 1,
                last_acked_global_seq: seq,
            },
            filter: filter.clone(),
        },
    ).unwrap();
}

fn append_coordinate(store: &Store, kind: &str, aggregate: &str) {
    store.with_immediate_tx(|tx| {
        Store::append_audit_conn_with_options(
            tx, kind, None, None, None, None, &[], &[], Some(aggregate), None,
        )?;
        Ok(())
    }).unwrap();
}

#[test]
fn checkpoint_beyond_empty_or_populated_ledger_blocks_as_unknown_without_repair() {
    for populated in [false, true] {
        let store = Store::open_in_memory().unwrap();
        if populated {
            append_coordinate(&store, "worker.failed", "system");
        }
        let filter = EventSubscriptionFilter::kinds([AuditKind::from_static("worker.failed")]);
        save_test_checkpoint(&store, &filter, 9);
        let checkpoint_before = store.load_consumer_checkpoint("worker_failed_consumer").unwrap();
        let audit_before = store.all_audit_event_jsons().unwrap();
        let snapshot = store.package_outstanding_work(Timestamp::now()).unwrap();
        assert_eq!(
            snapshot.source(OutstandingWorkSource::EventConsumerBacklog),
            OutstandingWorkCounts { outstanding: 0, terminal: 0, unknown: 1 },
        );
        assert!(!snapshot.is_quiescent());
        assert_eq!(store.load_consumer_checkpoint("worker_failed_consumer").unwrap(), checkpoint_before);
        assert_eq!(store.all_audit_event_jsons().unwrap(), audit_before);
    }
}

#[test]
fn checkpoint_must_name_an_existing_coordinate_not_only_be_below_the_maximum() {
    let store = Store::open_in_memory().unwrap();
    for _ in 0..3 {
        append_coordinate(&store, "worker.failed", "system");
    }
    let filter = EventSubscriptionFilter::kinds([AuditKind::from_static("worker.failed")]);
    save_test_checkpoint(&store, &filter, 2);
    store.with_conn_for_test(|conn| {
        assert_eq!(conn.execute("DELETE FROM audit_log WHERE seq = 2", []).unwrap(), 1);
    });
    for required in [false, true] {
        assert_eq!(
            store.with_deferred_read(|tx| checkpoint_state(tx, "worker_failed_consumer", &filter, required)).unwrap(),
            CheckpointState::Invalid,
        );
    }
}

#[test]
fn checkpoint_coordinate_must_match_both_kind_and_aggregate() {
    for (kind, aggregate) in [("worker.result", "a"), ("worker.failed", "b")] {
        let store = Store::open_in_memory().unwrap();
        append_coordinate(&store, "worker.failed", "a");
        append_coordinate(&store, kind, aggregate);
        append_coordinate(&store, "worker.failed", "a");
        let filter = EventSubscriptionFilter {
            schema_version: 1,
            kinds: Some(vec![AuditKind::from_static("worker.failed")]),
            aggregate_id: Some("a".into()),
        };
        save_test_checkpoint(&store, &filter, 2);
        for required in [false, true] {
            assert_eq!(
                store.with_deferred_read(|tx| checkpoint_state(tx, "worker_failed_consumer", &filter, required)).unwrap(),
                CheckpointState::Invalid,
                "{kind}:{aggregate}",
            );
        }
    }
}

#[test]
fn checkpoint_coordinate_reuses_audit_projection_validation() {
    for column in ["event_json", "meta_json"] {
        let store = Store::open_in_memory().unwrap();
        append_coordinate(&store, "worker.failed", "system");
        let filter = EventSubscriptionFilter::kinds([AuditKind::from_static("worker.failed")]);
        save_test_checkpoint(&store, &filter, 1);
        store.with_conn_for_test(|conn| {
            conn.execute(&format!("UPDATE audit_log SET {column} = '{{}}' WHERE seq = 1"), []).unwrap();
        });
        assert!(store.with_deferred_read(|tx| checkpoint_state(tx, "worker_failed_consumer", &filter, false)).is_err());
    }
}

#[test]
fn zero_and_real_matching_checkpoints_remain_valid_across_unrelated_events() {
    let store = Store::open_in_memory().unwrap();
    let filter = EventSubscriptionFilter {
        schema_version: 1,
        kinds: Some(vec![AuditKind::from_static("worker.failed")]),
        aggregate_id: Some("a".into()),
    };
    save_test_checkpoint(&store, &filter, 0);
    assert_eq!(store.with_deferred_read(|tx| checkpoint_state(tx, "worker_failed_consumer", &filter, true)).unwrap(), CheckpointState::Valid(0));
    append_coordinate(&store, "worker.failed", "a");
    append_coordinate(&store, "worker.result", "b");
    append_coordinate(&store, "worker.failed", "a");
    for seq in [1, 3] {
        save_test_checkpoint(&store, &filter, seq);
        assert_eq!(store.with_deferred_read(|tx| checkpoint_state(tx, "worker_failed_consumer", &filter, true)).unwrap(), CheckpointState::Valid(seq));
    }
}

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
        append_coordinate(&store, "worker.failed", "a");
        store.with_conn_for_test(|conn| {
            // Replace the second event through the normal append helper below instead.
            let _ = conn;
        });
        // Each fixture needs one valid event, the nonmatching coordinate,
        // and a later valid event. Use a fresh store to keep the coordinate exact.
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

#[test]
fn spend_alert_census_distinguishes_live_terminal_and_unknown_states() {
    for alert_state in [0_i64, 1, 2, -1, 3, i64::MAX] {
        let store = Store::open_in_memory().unwrap();
        store.with_conn_for_test(|conn| {
            conn.execute(
                "INSERT INTO daily_spend (day, model_calls, connector_calls, alert_state) VALUES ('2099-01-01', 0, 0, ?1)",
                [alert_state],
            ).unwrap();
        });
        let expected = match alert_state {
            0 => OutstandingWorkCounts { outstanding: 0, terminal: 1, unknown: 0 },
            1 | 2 => OutstandingWorkCounts { outstanding: 1, terminal: 0, unknown: 0 },
            _ => OutstandingWorkCounts { outstanding: 0, terminal: 0, unknown: 1 },
        };
        let snapshot = store.package_outstanding_work(Timestamp::now()).unwrap();
        assert_eq!(snapshot.source(OutstandingWorkSource::SpendAlerts), expected, "{alert_state}");
        assert_eq!(snapshot.is_quiescent(), alert_state == 0);
        store.with_conn_for_test(|conn| {
            let stored: i64 = conn.query_row("SELECT alert_state FROM daily_spend", [], |row| row.get(0)).unwrap();
            assert_eq!(stored, alert_state, "review must not normalize drift");
        });
    }
}

#[test]
fn audit_timestamp_projection_drift_is_rejected_by_replay_count_and_checkpoint() {
    for timestamp in ["not-a-time", "1900-01-01T00:00:00Z"] {
        let store = Store::open_in_memory().unwrap();
        append_coordinate(&store, "worker.failed", "a");
        store.with_conn_for_test(|conn| {
            conn.execute("UPDATE audit_log SET ts = ?1 WHERE seq = 1", [timestamp]).unwrap();
        });
        for aggregate_id in [None, Some("a".to_string())] {
            for kind in ["worker.failed", "census.nonmatching"] {
                let filter = EventSubscriptionFilter {
                    schema_version: 1,
                    kinds: Some(vec![AuditKind::from_static(kind)]),
                    aggregate_id: aggregate_id.clone(),
                };
                assert!(store.replay_audit(&filter, 0).is_err());
                assert!(store.with_deferred_read(|tx| matching_backlog(tx, &filter, 0)).is_err());
                assert!(store.with_deferred_read(|tx| Store::audit_checkpoint_matches_conn(tx, &filter, 1)).is_err());
            }
        }
        store.with_conn_for_test(|conn| {
            let stored: String = conn.query_row("SELECT ts FROM audit_log WHERE seq = 1", [], |row| row.get(0)).unwrap();
            assert_eq!(stored, timestamp, "validation must not repair the projection");
        });
    }
}

#[test]
fn audit_timestamp_equivalent_encoding_preserves_the_validated_instant() {
    let store = Store::open_in_memory().unwrap();
    append_coordinate(&store, "worker.failed", "a");
    let filter = EventSubscriptionFilter::all();
    let original = store.replay_audit(&filter, 0).unwrap().remove(0).event.ts;
    let equivalent = format!("{}+00:00", original.to_string().trim_end_matches('Z'));
    store.with_conn_for_test(|conn| {
        conn.execute("UPDATE audit_log SET ts = ?1 WHERE seq = 1", [&equivalent]).unwrap();
    });
    assert_eq!(store.replay_audit(&filter, 0).unwrap().remove(0).event.ts, original);
    assert_eq!(store.with_deferred_read(|tx| matching_backlog(tx, &filter, 0)).unwrap(), 1);
    assert!(store.with_deferred_read(|tx| Store::audit_checkpoint_matches_conn(tx, &filter, 1)).unwrap());
}

#[test]
fn aggregate_projection_cannot_hide_corruption_from_replay_or_count() {
    for selected in ["a", "b", "unrelated"] {
        let store = Store::open_in_memory().unwrap();
        append_coordinate(&store, "census.signal", "a");
        store.with_conn_for_test(|conn| {
            conn.execute("UPDATE audit_log SET aggregate_id = 'b' WHERE seq = 1", []).unwrap();
        });
        let filter = EventSubscriptionFilter {
            schema_version: 1,
            kinds: Some(vec![AuditKind::from_static("census.signal")]),
            aggregate_id: Some(selected.into()),
        };
        assert!(store.replay_audit(&filter, 0).is_err(), "{selected}");
        assert!(store.with_deferred_read(|tx| matching_backlog(tx, &filter, 0)).is_err(), "{selected}");
        assert_eq!(store.with_deferred_read(|tx| matching_backlog(tx, &filter, 1)).unwrap(), 0);
        store.with_conn_for_test(|conn| {
            let aggregate: String = conn.query_row("SELECT aggregate_id FROM audit_log WHERE seq = 1", [], |row| row.get(0)).unwrap();
            assert_eq!(aggregate, "b", "validation must not repair the projection");
        });
    }
}

#[test]
fn screener_census_detects_aggregate_drift_after_static_consumers_catch_up() {
    use openspine_schemas::nerve::{ModelTier, NerveBudget, NerveMeasure, NerveScope, Severity, SpeakThreshold};
    let store = Store::open_in_memory().unwrap();
    let scope = NerveScope { data_classes: vec!["census".into()], data_scopes: vec!["system".into()] };
    store.register_advisee_limits("agent:aggregate-census", &scope, ModelTier::Standard).unwrap();
    let filter = EventSubscriptionFilter {
        schema_version: 1,
        kinds: Some(vec![AuditKind::from_static("census.signal")]),
        aggregate_id: Some("a".into()),
    };
    let declaration = NerveDeclaration {
        id: ulid::Ulid::new(), schema_version: 1, nerve_type: NerveType::Screener,
        advisee_id: "agent:aggregate-census".into(), subscription_filter: filter.clone(),
        measure: NerveMeasure::ManipulationTag,
        speak_threshold: SpeakThreshold { severity_min: Severity::Warn, min_confidence: 0.5 },
        budget: NerveBudget { window_kind: "task".into(), window_seconds: 3600, suggestions_max: 1 },
        model_tier: ModelTier::Cheap, scope,
    };
    store.register_nerve(&declaration).unwrap();
    append_coordinate(&store, "census.signal", "a");
    let signal = store.replay_audit(&filter, 0).unwrap().pop().unwrap();
    for (consumer, kind) in [
        ("worker_result_consumer", "worker.result"),
        ("worker_failed_consumer", "worker.failed"),
        ("task_board_timer_consumer", "workflow.timer_fired"),
        ("standing_rule_dark_window_consumer", "workflow.timer_fired"),
    ] {
        if consumer != "standing_rule_dark_window_consumer" {
            append_coordinate(&store, kind, "system");
        }
        let filter = EventSubscriptionFilter::kinds([AuditKind::from_static(kind)]);
        let after = store.replay_audit(&filter, 0).unwrap().pop().unwrap().global_seq;
        store.save_consumer_checkpoint(consumer, &PersistedConsumerState {
            schema_version: 1,
            checkpoint: openspine_schemas::event_bus::ConsumerCheckpoint { schema_version: 1, last_acked_global_seq: after },
            filter,
        }).unwrap();
    }
    let before = store.package_outstanding_work(Timestamp::now()).unwrap();
    assert_eq!(before.source(OutstandingWorkSource::EventConsumerBacklog), OutstandingWorkCounts { outstanding: 1, terminal: 0, unknown: 0 });
    store.with_conn_for_test(|conn| {
        conn.execute("UPDATE audit_log SET aggregate_id = 'b' WHERE id = ?1", [signal.event.id.to_string()]).unwrap();
    });
    let checkpoint_before = store.load_consumer_checkpoint(&format!("nerve:{}", declaration.id)).unwrap();
    let audit_before = store.all_audit_event_jsons().unwrap();
    assert!(store.package_outstanding_work(Timestamp::now()).is_err());
    assert_eq!(store.load_consumer_checkpoint(&format!("nerve:{}", declaration.id)).unwrap(), checkpoint_before);
    assert_eq!(store.all_audit_event_jsons().unwrap(), audit_before);
}

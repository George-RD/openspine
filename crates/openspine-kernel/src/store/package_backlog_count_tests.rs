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

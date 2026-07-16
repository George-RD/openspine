//! Tests for the AD-105 event-bus read path (`event_bus.rs`).

use super::*;
use openspine_schemas::audit::AuditKind;
use openspine_schemas::event_bus::EventSubscriptionFilter;
use std::collections::BTreeSet;
use ulid::Ulid;

fn append(store: &Store, kind: &str, grant: Option<Ulid>) {
    store
        .append_audit(kind, None, None, None, grant, &[], &[])
        .unwrap();
}

#[test]
fn ledger_append_is_visible_before_consumer_runs() {
    let store = Store::open_in_memory().unwrap();
    let event = store
        .append_audit("kernel.started", None, None, None, None, &[], &[])
        .unwrap();

    // Append returned ⇒ row is durable and queryable before any consumer.
    let entries = store
        .replay_audit(&EventSubscriptionFilter::all(), 0)
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].event.id, event.id);
    assert_eq!(entries[0].event.kind.as_str(), "kernel.started");
    assert_eq!(entries[0].global_seq, 1);
    assert_eq!(entries[0].event.aggregate_id, "system");
    assert_eq!(entries[0].event.aggregate_seq, 1);
}

#[test]
fn unique_ids_and_per_aggregate_sequences() {
    let store = Store::open_in_memory().unwrap();
    let grant_a = Ulid::new();
    let grant_b = Ulid::new();

    append(&store, "a.one", Some(grant_a));
    append(&store, "b.one", Some(grant_b));
    append(&store, "a.two", Some(grant_a));
    append(&store, "sys.one", None);
    append(&store, "b.two", Some(grant_b));
    append(&store, "sys.two", None);

    let all = store
        .replay_audit(&EventSubscriptionFilter::all(), 0)
        .unwrap();
    assert_eq!(all.len(), 6);

    let ids: BTreeSet<_> = all.iter().map(|e| e.event.id).collect();
    assert_eq!(ids.len(), 6, "all event IDs must be unique");

    let a_id = format!("task_grant:{grant_a}");
    let b_id = format!("task_grant:{grant_b}");
    let a_seqs: Vec<_> = all
        .iter()
        .filter(|e| e.event.aggregate_id == a_id)
        .map(|e| e.event.aggregate_seq)
        .collect();
    let b_seqs: Vec<_> = all
        .iter()
        .filter(|e| e.event.aggregate_id == b_id)
        .map(|e| e.event.aggregate_seq)
        .collect();
    let sys_seqs: Vec<_> = all
        .iter()
        .filter(|e| e.event.aggregate_id == "system")
        .map(|e| e.event.aggregate_seq)
        .collect();

    assert_eq!(a_seqs, vec![1, 2]);
    assert_eq!(b_seqs, vec![1, 2]);
    assert_eq!(sys_seqs, vec![1, 2]);
}

#[test]
fn filtered_replay_is_idempotent() {
    let store = Store::open_in_memory().unwrap();
    append(&store, "authority.granted", None);
    append(&store, "action.gated", None);
    append(&store, "artifact.activated", None);
    append(&store, "action.gated", None);

    let filter = EventSubscriptionFilter::kinds([AuditKind::from_static("action.gated")]);
    let mut consumer = IdempotentConsumer::new("test-consumer", filter);

    #[derive(Default, Clone, PartialEq, Eq, Debug)]
    struct State {
        kinds: Vec<String>,
        count: usize,
    }

    let mut state = State::default();
    consumer
        .replay(&store, &mut state, |s, event| {
            s.kinds.push(event.kind.as_str().to_string());
            s.count += 1;
            Ok::<(), &str>(())
        })
        .unwrap();

    assert_eq!(state.count, 2);
    assert_eq!(
        state.kinds,
        vec!["action.gated".to_string(), "action.gated".to_string()]
    );
    let after_first = state.clone();
    let ckpt_after_first = consumer.checkpoint().clone();

    // Second replay: pure no-op.
    consumer
        .replay(&store, &mut state, |s, event| {
            s.kinds.push(event.kind.as_str().to_string());
            s.count += 1;
            Ok::<(), &str>(())
        })
        .unwrap();

    assert_eq!(state, after_first);
    assert_eq!(consumer.checkpoint(), &ckpt_after_first);
}

#[test]
fn failed_handler_does_not_advance_checkpoint() {
    let store = Store::open_in_memory().unwrap();
    append(&store, "action.gated", None);
    append(&store, "action.gated", None);

    let filter = EventSubscriptionFilter::kinds([AuditKind::from_static("action.gated")]);
    let mut consumer = IdempotentConsumer::new("failing", filter);
    let mut seen = 0usize;

    let err = consumer
        .replay(&store, &mut seen, |n, _| {
            *n += 1;
            if *n == 1 {
                Err("boom")
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    match err {
        ConsumerError::Handler { global_seq, .. } => assert_eq!(global_seq, 1),
        other => panic!("unexpected error: {other}"),
    }
    assert_eq!(consumer.checkpoint().last_acked_global_seq, 0);
    assert_eq!(seen, 1);

    // Retry succeeds for both (handler no longer fails).
    let mut seen2 = 0usize;
    consumer
        .replay(&store, &mut seen2, |n, _| {
            *n += 1;
            Ok::<(), &str>(())
        })
        .unwrap();
    assert_eq!(seen2, 2);
    assert_eq!(consumer.checkpoint().last_acked_global_seq, 2);
}

#[test]
fn persisted_checkpoint_survives_reload() {
    let store = Store::open_in_memory().unwrap();
    append(&store, "kernel.started", None);
    append(&store, "kernel.started", None);

    let filter = EventSubscriptionFilter::all();
    let mut consumer =
        IdempotentConsumer::with_persisted_checkpoint(&store, "durable", filter.clone()).unwrap();
    let mut n = 0usize;
    consumer
        .replay(&store, &mut n, |c, _| {
            *c += 1;
            Ok::<(), &str>(())
        })
        .unwrap();
    assert_eq!(n, 2);
    assert_eq!(consumer.checkpoint().last_acked_global_seq, 2);

    let reloaded =
        IdempotentConsumer::with_persisted_checkpoint(&store, "durable", filter).unwrap();
    assert_eq!(reloaded.checkpoint().last_acked_global_seq, 2);
}

#[test]
fn consumer_id_is_bound_to_filter() {
    let store = Store::open_in_memory().unwrap();
    append(&store, "action.gated", None);

    let filter_a = EventSubscriptionFilter::kinds([AuditKind::from_static("action.gated")]);
    let mut consumer =
        IdempotentConsumer::with_persisted_checkpoint(&store, "bound", filter_a).unwrap();
    let mut n = 0usize;
    consumer
        .replay(&store, &mut n, |c, _| {
            *c += 1;
            Ok::<(), &str>(())
        })
        .unwrap();

    let filter_b = EventSubscriptionFilter::kinds([AuditKind::from_static("authority.granted")]);
    let err = IdempotentConsumer::with_persisted_checkpoint(&store, "bound", filter_b).unwrap_err();
    assert!(matches!(err, ConsumerError::FilterMismatch { .. }));
}

#[test]
fn event_id_defense_skips_duplicate_in_process() {
    let store = Store::open_in_memory().unwrap();
    let event = store
        .append_audit("action.gated", None, None, None, None, &[], &[])
        .unwrap();

    let filter = EventSubscriptionFilter::all();
    let mut consumer = IdempotentConsumer::new("dup", filter);
    // Pre-seed the seen set as if the handler already ran for this id.
    consumer.seen_event_ids.insert(event.id);

    // Reset watermark so replay would re-deliver the row by seq.
    consumer.checkpoint.last_acked_global_seq = 0;
    let mut n = 0usize;
    consumer
        .replay(&store, &mut n, |c, _| {
            *c += 1;
            Ok::<(), &str>(())
        })
        .unwrap();
    assert_eq!(n, 0, "handler must not re-run for a seen event id");
    assert_eq!(consumer.checkpoint().last_acked_global_seq, 1);
}

#[test]
fn kind_filter_preserves_global_order() {
    let store = Store::open_in_memory().unwrap();
    append(&store, "authority.granted", None);
    append(&store, "action.gated", None);
    append(&store, "artifact.activated", None);

    let filter = EventSubscriptionFilter::kinds([AuditKind::from_static("action.gated")]);
    let entries = store.replay_audit(&filter, 0).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].event.kind.as_str(), "action.gated");
    assert_eq!(entries[0].global_seq, 2);
}

#[test]
fn replay_after_watermark_skips_earlier_rows() {
    let store = Store::open_in_memory().unwrap();
    append(&store, "a", None);
    append(&store, "b", None);
    append(&store, "c", None);

    let entries = store
        .replay_audit(&EventSubscriptionFilter::all(), 2)
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].global_seq, 3);
    assert_eq!(entries[0].event.kind.as_str(), "c");
}

#[test]
fn legacy_audit_log_opens_and_migrates() {
    // Simulate a pre-AD-105 on-disk DB: audit_log without aggregate columns.
    // Store::open must still succeed (SCHEMA_SQL must not create an index on
    // missing columns before migrations add them).
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("kernel.db");
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            // Pre-AD-105 shape: no UNIQUE on id, no aggregate columns.
            // Migrations must add unique indexes after ADD COLUMN.
            "CREATE TABLE audit_log (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                id TEXT NOT NULL,
                ts TEXT NOT NULL,
                kind TEXT NOT NULL,
                prev_hash TEXT NOT NULL,
                hash TEXT NOT NULL,
                meta_json TEXT NOT NULL,
                event_json TEXT NOT NULL
            );",
        )
        .unwrap();
        // Genuine pre-AD-105 row: metadata and event JSON omit aggregate
        // coordinates; migration supplies system/0 columns without rewriting
        // the hash-chained payload.
        let meta = serde_json::json!({
            "id": "01J00000000000000000000000",
            "ts": "2026-01-01T00:00:00Z",
            "kind": "kernel.started",
            "action": null,
            "decision": null,
            "reason": null,
            "task_grant_id": null,
            "target_refs": [],
            "payload_refs": []
        });
        let event = serde_json::json!({
            "id": "01J00000000000000000000000",
            "schema_version": 1,
            "ts": "2026-01-01T00:00:00Z",
            "kind": "kernel.started",
            "action": null,
            "decision": null,
            "reason": null,
            "task_grant_id": null,
            "target_refs": [],
            "payload_refs": [],
            "prev_hash": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            "hash": "sha256:1111111111111111111111111111111111111111111111111111111111111111"
        });
        conn.execute(
            "INSERT INTO audit_log (id, ts, kind, prev_hash, hash, meta_json, event_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "01J00000000000000000000000", "2026-01-01T00:00:00Z", "kernel.started",
                "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                "sha256:1111111111111111111111111111111111111111111111111111111111111111",
                meta.to_string(), event.to_string()
            ],
        ).unwrap();
    }
    let store = Store::open(&db_path).expect("legacy DB must open via ad-hoc migrations");
    // Unique indexes must exist after migration.
    {
        let conn = store.conn.lock();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master                  WHERE type = 'index' AND name IN                  ('idx_audit_id', 'idx_audit_aggregate_seq_unique')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 2,
            "unique bus indexes must be created for legacy DBs"
        );
    }
    let event = store
        .append_audit("kernel.started", None, None, None, None, &[], &[])
        .unwrap();
    assert_eq!(event.aggregate_id, "system");
    assert_eq!(event.aggregate_seq, 1);
    let entries = store
        .replay_audit(&EventSubscriptionFilter::all(), 0)
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].event.aggregate_id, "system");
    assert_eq!(entries[0].event.aggregate_seq, 0);
    assert_eq!(entries[0].event.kind.as_str(), "kernel.started");
}

#[test]
fn tampered_event_json_is_rejected_before_delivery() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_audit("action.gated", None, None, Some("original"), None, &[], &[])
        .unwrap();
    {
        let conn = store.conn.lock();
        conn.execute("UPDATE audit_log SET event_json = replace(event_json, 'original', 'tampered') WHERE seq = 1", []).unwrap();
    }
    let err = store
        .replay_audit(&EventSubscriptionFilter::all(), 0)
        .unwrap_err();
    assert!(err.to_string().contains("metadata") || err.to_string().contains("mismatch"));
}

#[test]
fn checkpoint_filter_and_watermark_regressions_fail_closed() {
    let store = Store::open_in_memory().unwrap();
    let filter = EventSubscriptionFilter::all();
    let base = PersistedConsumerState {
        schema_version: 1,
        checkpoint: ConsumerCheckpoint {
            schema_version: 1,
            last_acked_global_seq: 5,
        },
        filter: filter.clone(),
    };
    store.save_consumer_checkpoint("cas", &base).unwrap();
    let lower = PersistedConsumerState {
        schema_version: 1,
        checkpoint: ConsumerCheckpoint {
            schema_version: 1,
            last_acked_global_seq: 4,
        },
        filter: filter.clone(),
    };
    assert!(matches!(
        store.save_consumer_checkpoint("cas", &lower),
        Err(StoreError::CheckpointRegression(_))
    ));
    let other = PersistedConsumerState {
        schema_version: 1,
        checkpoint: ConsumerCheckpoint {
            schema_version: 1,
            last_acked_global_seq: 6,
        },
        filter: EventSubscriptionFilter::kinds([openspine_schemas::audit::AuditKind::from_static(
            "other",
        )]),
    };
    assert!(matches!(
        store.save_consumer_checkpoint("cas", &other),
        Err(StoreError::CheckpointFilterMismatch(_))
    ));
}

#[test]
fn aggregate_column_tamper_breaks_startup_verification() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_audit("kernel.started", None, None, None, None, &[], &[])
        .unwrap();
    {
        let conn = store.conn.lock();
        conn.execute(
            "UPDATE audit_log SET aggregate_id = 'tampered' WHERE seq = 1",
            [],
        )
        .unwrap();
    }
    assert!(!store.verify_audit_chain().unwrap());
}

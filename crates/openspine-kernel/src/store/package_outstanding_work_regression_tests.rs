use super::*;
use crate::store::event_bus::PersistedConsumerState;
use openspine_schemas::audit::AuditKind;
use openspine_schemas::event_bus::{ConsumerCheckpoint, EventSubscriptionFilter};
use openspine_schemas::nerve::{
    ModelTier, NerveBudget, NerveDeclaration, NerveMeasure, NerveScope, NerveType, Severity,
    SpeakThreshold,
};
use ulid::Ulid;

fn counts(store: &Store, source: OutstandingWorkSource) -> OutstandingWorkCounts {
    store
        .package_outstanding_work(Timestamp::now())
        .unwrap()
        .source(source)
}

fn assert_blocks(store: &Store, source: OutstandingWorkSource) {
    let snapshot = store.package_outstanding_work(Timestamp::now()).unwrap();
    let source_counts = snapshot.source(source);
    assert!(
        source_counts.outstanding > 0 || source_counts.unknown > 0,
        "{} must contain outstanding or unknown work",
        source.as_str()
    );
    assert!(!snapshot.is_quiescent(), "{} must block", source.as_str());
}

fn checkpoint(store: &Store, consumer_id: &str, filter: EventSubscriptionFilter, seq: u64) {
    store
        .save_consumer_checkpoint(
            consumer_id,
            &PersistedConsumerState {
                schema_version: 1,
                checkpoint: ConsumerCheckpoint {
                    schema_version: 1,
                    last_acked_global_seq: seq,
                },
                filter,
            },
        )
        .unwrap();
}

fn register_test_nerve(store: &Store, kind: &'static str) -> (Ulid, EventSubscriptionFilter) {
    let scope = NerveScope {
        data_classes: vec!["census".into()],
        data_scopes: vec!["system".into()],
    };
    store
        .register_advisee_limits("agent:census", &scope, ModelTier::Standard)
        .unwrap();
    let filter = EventSubscriptionFilter {
        schema_version: 1,
        kinds: Some(vec![AuditKind::from_static(kind)]),
        aggregate_id: Some("system".into()),
    };
    let declaration = NerveDeclaration {
        id: Ulid::new(),
        schema_version: 1,
        nerve_type: NerveType::Advisor,
        advisee_id: "agent:census".into(),
        subscription_filter: filter.clone(),
        measure: NerveMeasure::Legibility,
        speak_threshold: SpeakThreshold {
            severity_min: Severity::Warn,
            min_confidence: 0.5,
        },
        budget: NerveBudget {
            window_kind: "task".into(),
            window_seconds: 3600,
            suggestions_max: 1,
        },
        model_tier: ModelTier::Cheap,
        scope,
    };
    store.register_nerve(&declaration).unwrap();
    (declaration.id, filter)
}

#[test]
fn pending_disclosure_question_blocks_until_resolved() {
    let store = Store::open_in_memory().unwrap();
    store.with_conn_for_test(|conn| {
        conn.execute(
            "INSERT INTO disclosure_pending_questions (pending_id, question_json, created_at)
             VALUES ('disclosure-pending', '{}', 1)",
            [],
        )
        .unwrap();
    });

    assert_eq!(
        counts(&store, OutstandingWorkSource::DisclosurePendingQuestions),
        OutstandingWorkCounts {
            outstanding: 1,
            terminal: 0,
            unknown: 0,
        }
    );
    assert_blocks(&store, OutstandingWorkSource::DisclosurePendingQuestions);

    store.with_conn_for_test(|conn| {
        conn.execute(
            "DELETE FROM disclosure_pending_questions WHERE pending_id = 'disclosure-pending'",
            [],
        )
        .unwrap();
    });
    assert!(store
        .package_outstanding_work(Timestamp::now())
        .unwrap()
        .is_quiescent());
}

#[test]
fn pending_nerve_delivery_blocks_until_acknowledged() {
    let store = Store::open_in_memory().unwrap();
    store.with_conn_for_test(|conn| {
        conn.execute(
            "INSERT INTO nerve_interjection_deliveries
             (interjection_id, class_digest, gate_visible)
             VALUES ('nerve-delivery-pending', 'sha256:delivery', 1)",
            [],
        )
        .unwrap();
    });

    assert_eq!(
        counts(&store, OutstandingWorkSource::NerveInterjectionDeliveries),
        OutstandingWorkCounts {
            outstanding: 1,
            terminal: 0,
            unknown: 0,
        }
    );
    assert_blocks(&store, OutstandingWorkSource::NerveInterjectionDeliveries);

    store.with_conn_for_test(|conn| {
        conn.execute(
            "DELETE FROM nerve_interjection_deliveries WHERE interjection_id = 'nerve-delivery-pending'",
            [],
        )
        .unwrap();
    });
    assert!(store
        .package_outstanding_work(Timestamp::now())
        .unwrap()
        .is_quiescent());
}

#[test]
fn secret_intake_pending_kv_blocks_until_cleared() {
    let store = Store::open_in_memory().unwrap();
    store
        .set_kv("secret.intake.pending", r#"{"future":"shape"}"#)
        .unwrap();

    assert_eq!(
        counts(&store, OutstandingWorkSource::SecretIntakePending),
        OutstandingWorkCounts {
            outstanding: 1,
            terminal: 0,
            unknown: 0,
        }
    );
    assert_blocks(&store, OutstandingWorkSource::SecretIntakePending);

    store.delete_kv("secret.intake.pending").unwrap();
    assert!(store
        .package_outstanding_work(Timestamp::now())
        .unwrap()
        .is_quiescent());
}

#[test]
fn static_consumer_event_blocks_until_checkpoint_acknowledges_it() {
    let store = Store::open_in_memory().unwrap();
    let filter = EventSubscriptionFilter::kinds([AuditKind::from_static("worker.failed")]);
    store
        .append_audit("worker.failed", None, None, None, None, &[], &[])
        .unwrap();

    assert_blocks(&store, OutstandingWorkSource::EventConsumerBacklog);
    let entry = store.replay_audit(&filter, 0).unwrap().pop().unwrap();
    checkpoint(&store, "worker_failed_consumer", filter, entry.global_seq);

    assert_eq!(
        counts(&store, OutstandingWorkSource::EventConsumerBacklog),
        OutstandingWorkCounts::default()
    );
    assert!(store
        .package_outstanding_work(Timestamp::now())
        .unwrap()
        .is_quiescent());
}

#[test]
fn static_consumer_filter_mismatch_is_unknown_and_blocks_fail_closed() {
    let store = Store::open_in_memory().unwrap();
    checkpoint(
        &store,
        "worker_failed_consumer",
        EventSubscriptionFilter::kinds([AuditKind::from_static("action.gated")]),
        0,
    );

    assert_eq!(
        counts(&store, OutstandingWorkSource::EventConsumerBacklog),
        OutstandingWorkCounts {
            outstanding: 0,
            terminal: 0,
            unknown: 1,
        }
    );
    assert_blocks(&store, OutstandingWorkSource::EventConsumerBacklog);
}

#[test]
fn registered_nerve_event_blocks_and_missing_checkpoint_is_unknown() {
    let store = Store::open_in_memory().unwrap();
    let (nerve_id, filter) = register_test_nerve(&store, "census.signal");
    store
        .append_audit("census.signal", None, None, None, None, &[], &[])
        .unwrap();

    assert_blocks(&store, OutstandingWorkSource::EventConsumerBacklog);
    let entry = store.replay_audit(&filter, 0).unwrap().pop().unwrap();
    checkpoint(
        &store,
        &format!("nerve:{nerve_id}"),
        filter.clone(),
        entry.global_seq,
    );
    assert_eq!(
        counts(&store, OutstandingWorkSource::EventConsumerBacklog),
        OutstandingWorkCounts::default()
    );

    store.with_conn_for_test(|conn| {
        conn.execute(
            "DELETE FROM consumer_checkpoints WHERE consumer_id = ?1",
            [format!("nerve:{nerve_id}")],
        )
        .unwrap();
    });
    assert_eq!(
        counts(&store, OutstandingWorkSource::EventConsumerBacklog),
        OutstandingWorkCounts {
            outstanding: 0,
            terminal: 0,
            unknown: 1,
        }
    );
    assert_blocks(&store, OutstandingWorkSource::EventConsumerBacklog);
}

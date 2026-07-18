use super::super::Store;
use openspine_schemas::event_bus::EventSubscriptionFilter;
use openspine_schemas::nerve::{
    AdvisorObjection, InterjectionProvenance, ModelTier, NerveBudget, NerveDeclaration,
    NerveMeasure, NerveScope, NerveType, Severity, SpeakThreshold,
};
use std::sync::{Arc, Barrier};
use std::thread;
use ulid::Ulid;

fn declaration() -> NerveDeclaration {
    NerveDeclaration {
        id: Ulid::new(),
        schema_version: 1,
        nerve_type: NerveType::Advisor,
        advisee_id: "agent:advisee".into(),
        subscription_filter: EventSubscriptionFilter {
            schema_version: 1,
            kinds: Some(vec![openspine_schemas::audit::AuditKind::from_static(
                "email",
            )]),
            aggregate_id: Some("selected_thread".into()),
        },
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
        scope: advisee_scope(),
    }
}

fn advisee_scope() -> NerveScope {
    NerveScope {
        data_classes: vec!["email".into()],
        data_scopes: vec!["selected_thread".into()],
    }
}

fn provenance() -> InterjectionProvenance {
    InterjectionProvenance {
        pattern: "concurrent test".into(),
        sources: vec!["test:nerve".into()],
    }
}

#[test]
fn concurrent_cross_connection_admission_spends_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    let first = Store::open(&path).unwrap();
    let nerve = declaration();
    first
        .register_advisee_limits(&nerve.advisee_id, &advisee_scope(), ModelTier::Standard)
        .unwrap();
    first.register_nerve(&nerve).unwrap();
    let second = Store::open(&path).unwrap();
    first
        .conn
        .lock()
        .busy_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    second
        .conn
        .lock()
        .busy_timeout(std::time::Duration::from_secs(2))
        .unwrap();

    let barrier = Arc::new(Barrier::new(2));
    let run = |store: Store| {
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            store.admit_interjection(
                nerve.id,
                "race",
                Severity::Warn,
                0.9,
                provenance(),
                false,
                Some(AdvisorObjection {
                    concern_class: "race".into(),
                    cited_clause: "clause".into(),
                }),
                None,
            )
        })
    };
    let first_handle = run(first);
    let second_handle = run(second);
    let first_result = first_handle.join().unwrap();
    let second_result = second_handle.join().unwrap();
    let successes = usize::from(first_result.is_ok()) + usize::from(second_result.is_ok());
    assert_eq!(successes, 1);
    let failures = [first_result, second_result];
    assert!(failures.iter().any(|result| {
        matches!(
            result,
            Err(openspine_schemas::nerve::NerveError::BudgetExhausted)
        )
    }));
}

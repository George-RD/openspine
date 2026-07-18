use super::super::Store;
use openspine_schemas::event_bus::EventSubscriptionFilter;
use openspine_schemas::nerve::{
    AdvisorObjection, InterjectionProvenance, InterjectionReaction, ModelTier, NerveBudget,
    NerveDeclaration, NerveMeasure, NerveScope, NerveType, Severity, SpeakThreshold,
};
use ulid::Ulid;

fn advisee_scope() -> NerveScope {
    NerveScope {
        data_classes: vec!["email".into(), "memory".into()],
        data_scopes: vec!["selected_thread".into()],
    }
}

fn register_nerve(
    store: &Store,
    nerve: &NerveDeclaration,
) -> Result<(), openspine_schemas::nerve::NerveError> {
    store.register_advisee_limits(&nerve.advisee_id, &advisee_scope(), ModelTier::Standard)?;
    let mut bounded = nerve.clone();
    if bounded.scope.data_classes.is_empty() && bounded.scope.data_scopes.is_empty() {
        bounded.scope = advisee_scope();
        bounded.subscription_filter = EventSubscriptionFilter {
            schema_version: 1,
            kinds: Some(vec![openspine_schemas::audit::AuditKind::from_static(
                "email",
            )]),
            aggregate_id: Some("selected_thread".into()),
        };
    }
    store.register_nerve(&bounded)
}

fn declaration(scope: NerveScope, budget: u32) -> NerveDeclaration {
    NerveDeclaration {
        id: Ulid::new(),
        schema_version: 1,
        nerve_type: NerveType::Advisor,
        advisee_id: "agent:advisee".into(),
        subscription_filter: EventSubscriptionFilter::all(),
        measure: NerveMeasure::Legibility,
        speak_threshold: SpeakThreshold {
            severity_min: Severity::Warn,
            min_confidence: 0.5,
        },
        budget: NerveBudget {
            window_kind: "task".into(),
            window_seconds: 3600,
            suggestions_max: budget,
        },
        model_tier: ModelTier::Cheap,
        scope,
    }
}

fn objection(class: &str) -> AdvisorObjection {
    AdvisorObjection {
        concern_class: class.into(),
        cited_clause: "grant binding".into(),
    }
}

fn provenance() -> InterjectionProvenance {
    InterjectionProvenance {
        pattern: "missing checkable reasoning".into(),
        sources: vec!["artifact:sha256:abc".into()],
    }
}

#[test]
fn broader_scope_is_unregistrable_and_writes_no_row() {
    let store = Store::open_in_memory().unwrap();
    let nerve = declaration(
        NerveScope {
            data_classes: vec!["email".into(), "memory".into(), "secrets".into()],
            data_scopes: vec!["selected_thread".into()],
        },
        1,
    );
    let err = register_nerve(&store, &nerve).unwrap_err();
    assert_eq!(
        err,
        openspine_schemas::nerve::NerveError::ScopeExceedsAdvisee
    );
    assert!(store.load_nerve(nerve.id).unwrap().is_none());
}

#[test]
fn valid_scope_registers_and_wider_tier_is_rejected() {
    let store = Store::open_in_memory().unwrap();
    let mut narrow = declaration(
        NerveScope {
            data_classes: vec!["email".into()],
            data_scopes: vec!["selected_thread".into()],
        },
        1,
    );
    narrow.subscription_filter = EventSubscriptionFilter {
        schema_version: 1,
        kinds: Some(vec![openspine_schemas::audit::AuditKind::from_static(
            "email.message",
        )]),
        aggregate_id: Some("selected_thread".into()),
    };
    register_nerve(&store, &narrow).unwrap();
    assert_eq!(store.load_nerve(narrow.id).unwrap(), Some(narrow.clone()));
    let checkpoint_json: String = store
        .conn
        .lock()
        .query_row(
            "SELECT checkpoint_json FROM consumer_checkpoints WHERE consumer_id = ?1",
            rusqlite::params![format!("nerve:{}", narrow.id)],
            |row| row.get(0),
        )
        .unwrap();
    let checkpoint: serde_json::Value = serde_json::from_str(&checkpoint_json).unwrap();
    assert_eq!(checkpoint["checkpoint"]["last_acked_global_seq"], 0);
    assert_eq!(
        checkpoint["filter"],
        serde_json::to_value(&narrow.subscription_filter).unwrap()
    );

    let mut strong = declaration(NerveScope::default(), 1);
    strong.model_tier = ModelTier::Strong;
    let err = register_nerve(&store, &strong).unwrap_err();
    assert_eq!(
        err,
        openspine_schemas::nerve::NerveError::TierExceedsAdvisee
    );
}

#[test]
fn admission_returns_structure_only_after_budget_debit() {
    let store = Store::open_in_memory().unwrap();
    let nerve = declaration(NerveScope::default(), 1);
    register_nerve(&store, &nerve).unwrap();
    let admitted = store
        .admit_interjection(
            nerve.id,
            "underspecified_effect",
            Severity::Warn,
            0.9,
            provenance(),
            false,
            Some(objection("underspecified_effect")),
            None,
        )
        .unwrap();
    assert!(admitted.gate_visible);
    assert_eq!(admitted.class, "underspecified_effect");
    assert!(admitted.id != Ulid::nil());
    assert!(serde_json::to_value(&admitted)
        .unwrap()
        .get("answer")
        .is_none());
    let exhausted = store
        .admit_interjection(
            nerve.id,
            "underspecified_effect",
            Severity::Critical,
            1.0,
            provenance(),
            false,
            Some(objection("underspecified_effect")),
            None,
        )
        .unwrap_err();
    assert_eq!(
        exhausted,
        openspine_schemas::nerve::NerveError::BudgetExhausted
    );
    let conn = store.conn.lock();
    conn.execute("UPDATE nerve_interjection_budgets SET window_started_ns = 0 WHERE nerve_id = ?1 AND window_kind = ?2", rusqlite::params![nerve.id.to_string(), nerve.budget.window_kind]).unwrap();
    drop(conn);
    store
        .admit_interjection(
            nerve.id,
            "after_rollover",
            Severity::Warn,
            0.9,
            provenance(),
            false,
            Some(objection("after_rollover")),
            None,
        )
        .unwrap();
}

#[test]
fn threshold_provenance_and_payload_class_are_checked_before_spend() {
    let store = Store::open_in_memory().unwrap();
    let nerve = declaration(NerveScope::default(), 1);
    register_nerve(&store, &nerve).unwrap();
    assert_eq!(
        store
            .admit_interjection(
                nerve.id,
                "low",
                Severity::Info,
                0.9,
                provenance(),
                false,
                Some(objection("low")),
                None
            )
            .unwrap_err(),
        openspine_schemas::nerve::NerveError::ThresholdNotMet
    );
    let zero = declaration(NerveScope::default(), 0);
    assert!(register_nerve(&store, &zero).is_err());
    let mut bad = provenance();
    bad.sources = vec![String::new()];
    assert_eq!(
        store
            .admit_interjection(
                nerve.id,
                "low",
                Severity::Warn,
                0.9,
                bad,
                false,
                Some(objection("low")),
                None
            )
            .unwrap_err(),
        openspine_schemas::nerve::NerveError::InvalidProvenance
    );
    assert_eq!(
        store
            .admit_interjection(
                nerve.id,
                "wrong",
                Severity::Warn,
                0.9,
                provenance(),
                false,
                Some(objection("right")),
                None
            )
            .unwrap_err(),
        openspine_schemas::nerve::NerveError::InvalidPayload
    );
    store
        .admit_interjection(
            nerve.id,
            "ok",
            Severity::Warn,
            0.9,
            provenance(),
            false,
            Some(objection("ok")),
            None,
        )
        .unwrap();
}

#[test]
fn five_ignored_reactions_retire_class_and_all_signals_persist() {
    let store = Store::open_in_memory().unwrap();
    let nerve = declaration(NerveScope::default(), 10);
    register_nerve(&store, &nerve).unwrap();
    let mut issued = Vec::new();
    for index in 0..5 {
        let emitted = store
            .admit_interjection(
                nerve.id,
                "noisy",
                Severity::Warn,
                0.9,
                provenance(),
                false,
                Some(objection("noisy")),
                None,
            )
            .unwrap();
        store
            .record_reaction(nerve.id, &emitted, InterjectionReaction::Ignored)
            .unwrap();
        issued.push(emitted);
        if index < 4 {
            assert!(!store.class_retired(nerve.id, "noisy").unwrap());
        }
    }
    assert!(store.class_retired(nerve.id, "noisy").unwrap());
    let forged = openspine_schemas::nerve::NerveInterjection {
        class: "forged".into(),
        ..issued.first().unwrap().clone()
    };
    assert_eq!(
        store
            .record_reaction(nerve.id, &forged, InterjectionReaction::Ignored)
            .unwrap_err(),
        openspine_schemas::nerve::NerveError::InvalidPayload
    );
    let helpful = store
        .admit_interjection(
            nerve.id,
            "helpful",
            Severity::Warn,
            0.9,
            provenance(),
            false,
            Some(objection("helpful")),
            None,
        )
        .unwrap();
    let annoyed = store
        .admit_interjection(
            nerve.id,
            "helpful",
            Severity::Warn,
            0.9,
            provenance(),
            false,
            Some(objection("helpful")),
            None,
        )
        .unwrap();
    store
        .record_reaction(nerve.id, &helpful, InterjectionReaction::Engaged)
        .unwrap();
    store
        .record_reaction(nerve.id, &annoyed, InterjectionReaction::Annoyed)
        .unwrap();
    let counts: (i64, i64, i64) = store.conn.lock().query_row("SELECT ignored_count, engaged_count, annoyed_count FROM nerve_decay WHERE nerve_id = ?1 AND class = ?2", rusqlite::params![nerve.id.to_string(), openspine_schemas::digest::digest_of_bytes(b"helpful").to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    assert_eq!(counts, (0, 1, 1));
}

#[test]
fn registered_replay_dispatches_typed_declarations_and_checkpoints() {
    let store = Store::open_in_memory().unwrap();
    let mut nerve = declaration(
        NerveScope {
            data_classes: vec!["email".into()],
            data_scopes: vec!["system".into()],
        },
        1,
    );
    nerve.subscription_filter = EventSubscriptionFilter {
        schema_version: 1,
        kinds: Some(vec![openspine_schemas::audit::AuditKind::from_static(
            "email",
        )]),
        aggregate_id: Some("system".into()),
    };
    store
        .register_advisee_limits(&nerve.advisee_id, &nerve.scope, ModelTier::Standard)
        .unwrap();
    store.register_nerve(&nerve).unwrap();
    store
        .append_audit("email", None, None, None, None, &[], &[])
        .unwrap();
    store
        .append_audit("other", None, None, None, None, &[], &[])
        .unwrap();
    let mut seen = Vec::new();
    store
        .replay_registered_nerves(|decl, event| {
            assert_eq!(decl.id, nerve.id);
            seen.push(event.kind.as_str().to_string());
            Ok(())
        })
        .unwrap();
    assert_eq!(seen, vec!["email"]);
    let mut duplicate_calls = 0;
    store
        .replay_registered_nerves(|_, _| {
            duplicate_calls += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(duplicate_calls, 0);
    store
        .append_audit("email", None, None, None, None, &[], &[])
        .unwrap();
    assert!(store
        .replay_registered_nerves(|_, _| Err("retry".into()))
        .is_err());
    store
        .replay_registered_nerves(|_, event| {
            seen.push(event.kind.as_str().to_string());
            Ok(())
        })
        .unwrap();
    assert_eq!(seen, vec!["email", "email"]);
}

#[test]
fn nerve_tables_never_store_interjection_plaintext() {
    let store = Store::open_in_memory().unwrap();
    let nerve = declaration(NerveScope::default(), 2);
    register_nerve(&store, &nerve).unwrap();
    let marker = "distinctive-secret-marker";
    let issued = store
        .admit_interjection(
            nerve.id,
            marker,
            Severity::Warn,
            0.99,
            provenance(),
            false,
            Some(AdvisorObjection {
                concern_class: marker.into(),
                cited_clause: marker.into(),
            }),
            None,
        )
        .unwrap();
    store
        .record_reaction(nerve.id, &issued, InterjectionReaction::Ignored)
        .unwrap();
    let conn = store.conn.lock();
    for table in [
        "nerve_registrations",
        "nerve_advisee_limits",
        "nerve_interjection_budgets",
        "nerve_decay",
        "nerve_issuances",
        "nerve_reactions",
    ] {
        let mut stmt = conn.prepare(&format!("SELECT * FROM {table}")).unwrap();
        let columns = stmt.column_count();
        let rows = stmt
            .query_map([], |row| {
                let mut values = Vec::new();
                for index in 0..columns {
                    let value: rusqlite::types::Value = row.get(index)?;
                    values.push(format!("{value:?}"));
                }
                Ok(values.join("|"))
            })
            .unwrap();
        for row in rows {
            assert!(
                !row.unwrap().contains(marker),
                "plaintext leaked in {table}"
            );
        }
    }
}

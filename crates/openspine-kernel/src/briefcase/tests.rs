use super::*;
use openspine_schemas::briefcase::{RelationshipTier, TopUpOutcome};
use openspine_schemas::identity::RelationshipKind;
fn grant_with(id: &str, event_id: &str, token: &str) -> TaskGrant {
    serde_json::from_value(serde_json::json!({"id":id,"schema_version":1,"lifecycle_state":"active","user":"owner","purpose":"test","issued_by":"kernel","issued_at":"2026-01-01T00:00:00Z","expires_at":"2030-01-01T00:00:00Z","event_id":event_id,"route_id":"route","agent_id":"agent","workflow_id":"workflow","capability_pack_id":"pack","allowed_actions":[],"approval_required_actions":[],"denied_actions":[],"output_channels":[],"limits":{"max_model_calls":1,"max_artifacts":1,"max_runtime_seconds":1},"task_token":token,"root_grant_id":id,"parent_grant_id":null,"mode":"live","chain":[],"caveat_mac":""})).unwrap()
}
fn grant() -> TaskGrant {
    grant_with(
        "01J00000000000000000000000",
        "01J00000000000000000000001",
        "secret",
    )
}

#[tokio::test]
async fn email_pack_fails_without_counterparty_address() {
    // Email-lane packing MUST fail (not silently degrade) when no
    // counterparty address is carried from the authorized preflight
    // snapshot — the "unavailable:email_counterparty" placeholder is gone.
    let state = crate::test_support::fixtures::test_state();
    let g = grant();
    let result = crate::briefcase::pack_for_pipeline(
        &state,
        Some("thread-1"),
        openspine_schemas::event::Lane::ExternalCommunication,
        &g,
        None,
    )
    .await;
    assert!(
        matches!(result, Err(BriefcaseKernelError::SourceUnavailable(_))),
        "email-lane packing must fail without a counterparty address, got {result:?}"
    );
}
#[test]
fn deterministic_pack_bytes() {
    let g = grant();
    let id = Ulid::new();
    let cp = CounterpartyRef::Bound {
        identity_id: id,
        relationship: RelationshipKind::Owner,
    };
    let a = pack_for_task(
        &g,
        cp.clone(),
        serde_json::json!({"identity_id": id}),
        TaskClass::Conversation,
        &SourcePool::default(),
    )
    .unwrap();
    let b = pack_for_task(
        &g,
        cp,
        serde_json::json!({"identity_id": id}),
        TaskClass::Conversation,
        &SourcePool::default(),
    )
    .unwrap();
    assert_eq!(a.canonical_bytes(), b.canonical_bytes());
}

#[test]
fn independent_grants_with_identical_semantics_pack_byte_identical() {
    // Two grants minted independently differ in instance-only fields
    // (id, event_id, task_token, timestamps, MAC). The stable semantic
    // projection MUST exclude all of those, so both pack byte-identical.
    let g1 = grant_with(
        "01J000000000000000000000AA",
        "01J000000000000000000000AB",
        "token-one",
    );
    let g2 = grant_with(
        "01J000000000000000000000BA",
        "01J000000000000000000000BB",
        "token-two",
    );
    let id = Ulid::new();
    let cp = CounterpartyRef::Bound {
        identity_id: id,
        relationship: RelationshipKind::Owner,
    };
    let slice = serde_json::json!({"identity_id": id});
    let a = pack_for_task(
        &g1,
        cp.clone(),
        slice.clone(),
        TaskClass::Conversation,
        &SourcePool::default(),
    )
    .unwrap();
    let b = pack_for_task(
        &g2,
        cp,
        slice,
        TaskClass::Conversation,
        &SourcePool::default(),
    )
    .unwrap();
    assert_eq!(a.canonical_bytes(), b.canonical_bytes());
}

#[test]
fn denied_topup_replay_is_rejected_before_evaluating() {
    let g = grant();
    let cp = CounterpartyRef::Bound {
        identity_id: Ulid::new(),
        relationship: RelationshipKind::Owner,
    };
    let mut b = pack_for_task(
        &g,
        cp,
        serde_json::json!({}),
        TaskClass::Conversation,
        &SourcePool::default(),
    )
    .unwrap();
    let req = TopUpRequest {
        request_id: Ulid::new(),
        section_key: "calendar".into(),
        kind: SectionKind::Preference,
        requested_depth: 99,
        justification: "denied".into(),
    };
    // Empty policy → first request is denied and recorded.
    let pool = SourcePool::default();
    let policy = TopUpPolicy::default();
    let first = apply_top_up(&mut b, &req, &policy, &pool).unwrap();
    assert!(matches!(
        first.outcome,
        openspine_schemas::briefcase::TopUpOutcome::Denied { .. }
    ));
    assert_eq!(b.top_up_log.len(), 1);
    // Resubmitting the same request_id MUST be rejected, not re-recorded.
    assert!(apply_top_up(&mut b, &req, &policy, &pool).is_err());
    assert_eq!(b.top_up_log.len(), 1);
}

#[test]
fn cross_connection_topup_race_resolved_by_transaction() {
    use std::sync::Arc;
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("race.db");
    let g = grant();
    let cp = CounterpartyRef::Bound {
        identity_id: Ulid::new(),
        relationship: RelationshipKind::Owner,
    };
    let briefcase = pack_for_task(
        &g,
        cp,
        serde_json::json!({}),
        TaskClass::Conversation,
        &SourcePool::default(),
    )
    .unwrap();
    let store_a = crate::store::Store::open(&path).unwrap();
    store_a.insert_briefcase(g.id, &briefcase).unwrap();
    let store_b = crate::store::Store::open(&path).unwrap();
    let pool = SourcePool {
        learned: vec![LearnedSource {
            key: "skill".into(),
            kind: SectionKind::Skill,
            payload: serde_json::json!({"x": 1}),
            applicable_tiers: vec![],
            applicable_workflows: vec![],
        }],
    };
    let policy = TopUpPolicy::new([(
        (
            RelationshipTier::Owner,
            TaskClass::Conversation,
            SectionKind::Skill,
        ),
        2,
    )]);
    let request = TopUpRequest {
        request_id: Ulid::new(),
        section_key: "skill".into(),
        kind: SectionKind::Skill,
        requested_depth: 1,
        justification: "race".into(),
    };
    let req = Arc::new(request);
    let pool_arc = Arc::new(pool);
    let policy_arc = Arc::new(policy);
    let sa = Arc::new(store_a);
    let sb = Arc::new(store_b);
    let ra = Arc::clone(&req);
    let pa = Arc::clone(&pool_arc);
    let pol_a = Arc::clone(&policy_arc);
    let h1 =
        std::thread::spawn(move || apply_top_up_for_grant(&sa, g.id, &ra, &pol_a, &pa).is_ok());
    let rb = Arc::clone(&req);
    let pb = Arc::clone(&pool_arc);
    let pol_b = Arc::clone(&policy_arc);
    let h2 =
        std::thread::spawn(move || apply_top_up_for_grant(&sb, g.id, &rb, &pol_b, &pb).is_ok());
    let ok1 = h1.join().unwrap();
    let ok2 = h2.join().unwrap();
    // Exactly one of the two contending connections wins.
    assert!(
        ok1 ^ ok2,
        "exactly one top-up must succeed under contention"
    );
    let final_store = crate::store::Store::open(&path).unwrap();
    let loaded = final_store.find_briefcase(g.id).unwrap().unwrap();
    assert_eq!(loaded.top_up_log.len(), 1);
}
#[test]
fn topup_replay_rejected() {
    let g = grant();
    let cp = CounterpartyRef::Bound {
        identity_id: Ulid::new(),
        relationship: RelationshipKind::Owner,
    };
    let mut b = pack_for_task(
        &g,
        cp,
        serde_json::json!({}),
        TaskClass::Conversation,
        &SourcePool::default(),
    )
    .unwrap();
    let req = TopUpRequest {
        request_id: Ulid::new(),
        section_key: "skill".into(),
        kind: SectionKind::Skill,
        requested_depth: 1,
        justification: "needed".into(),
    };
    let policy = TopUpPolicy::new([(
        (
            RelationshipTier::Owner,
            TaskClass::Conversation,
            SectionKind::Skill,
        ),
        2,
    )]);
    let pool = SourcePool {
        learned: vec![LearnedSource {
            key: "skill".into(),
            kind: SectionKind::Skill,
            payload: serde_json::json!({"x":1}),
            applicable_tiers: vec![],
            applicable_workflows: vec![],
        }],
    };
    assert!(matches!(
        apply_top_up(&mut b, &req, &policy, &pool).unwrap().outcome,
        TopUpOutcome::Allowed
    ));
    assert!(apply_top_up(&mut b, &req, &policy, &pool).is_err());
}
#[test]
fn persisted_top_up_is_atomic_and_replay_protected_after_reload() {
    let g = grant();
    let cp = CounterpartyRef::Bound {
        identity_id: Ulid::new(),
        relationship: RelationshipKind::Owner,
    };
    let briefcase = pack_for_task(
        &g,
        cp,
        serde_json::json!({}),
        TaskClass::Conversation,
        &SourcePool::default(),
    )
    .unwrap();
    let store = crate::store::Store::open_in_memory().unwrap();
    store.insert_briefcase(g.id, &briefcase).unwrap();
    let request = TopUpRequest {
        request_id: Ulid::new(),
        section_key: "skill".into(),
        kind: SectionKind::Skill,
        requested_depth: 1,
        justification: "needed".into(),
    };
    let policy = TopUpPolicy::new([(
        (
            RelationshipTier::Owner,
            TaskClass::Conversation,
            SectionKind::Skill,
        ),
        2,
    )]);
    let pool = SourcePool {
        learned: vec![LearnedSource {
            key: "skill".into(),
            kind: SectionKind::Skill,
            payload: serde_json::json!({"x": 1}),
            applicable_tiers: vec![],
            applicable_workflows: vec![],
        }],
    };
    apply_top_up_for_grant(&store, g.id, &request, &policy, &pool).unwrap();
    let loaded = store.find_briefcase(g.id).unwrap().unwrap();
    assert_eq!(loaded.top_up_log.len(), 1);
    assert!(apply_top_up_for_grant(&store, g.id, &request, &policy, &pool).is_err());
    assert_eq!(
        store
            .find_briefcase(g.id)
            .unwrap()
            .unwrap()
            .top_up_log
            .len(),
        1
    );
}

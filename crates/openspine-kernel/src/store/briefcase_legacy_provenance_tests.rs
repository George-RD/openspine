//! Upgrade regression for #251's schema-1 System placeholders (#220).
use super::tests::sample_grant;
use super::{Store, StoreError};
use crate::briefcase::{
    apply_top_up_for_grant, apply_top_up_for_grant_atomic, pack_for_task, SourcePool,
};
use openspine_schemas::action::ActionId;
use openspine_schemas::briefcase::{
    Briefcase, CounterpartyRef, LearnedSource, SectionKind, TaskClass, TopUpPolicy, TopUpRequest,
};
use openspine_schemas::identity::RelationshipKind;
use openspine_schemas::provenance::ProvenanceOrigin;
use serde_json::json;
use ulid::Ulid;

fn stored_json(store: &Store, id: Ulid) -> String {
    store.with_conn_for_test(|conn| {
        conn.query_row(
            "SELECT briefcase_json FROM briefcases WHERE task_grant_id = ?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .unwrap()
    })
}

fn legacy_pack(kind: SectionKind) -> (Ulid, Briefcase) {
    let grant = sample_grant("legacy-provenance");
    let pool = SourcePool {
        learned: [SectionKind::Preference, SectionKind::Skill]
            .into_iter()
            .map(|kind| LearnedSource {
                key: format!("{kind:?}"),
                kind,
                payload: json!({"owner_context": "private"}),
                applicable_tiers: vec![],
                applicable_workflows: vec![],
            })
            .collect(),
    };
    let mut briefcase = pack_for_task(
        &grant,
        CounterpartyRef::Bound {
            identity_id: Ulid::new(),
            relationship: RelationshipKind::Owner,
        },
        json!({}),
        TaskClass::Conversation,
        &pool,
    )
    .unwrap();
    assert_eq!(briefcase.schema_version, 1);
    briefcase
        .sections
        .iter_mut()
        .find(|s| s.kind == kind)
        .unwrap()
        .origin = Some(ProvenanceOrigin::System {});
    (grant.id, briefcase)
}

#[test]
fn legacy_system_owner_sections_are_refused_after_reopen_without_rewriting() {
    for kind in [
        SectionKind::Grant,
        SectionKind::Preference,
        SectionKind::Skill,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.db");
        let (id, briefcase) = legacy_pack(kind);
        let store = Store::open(&path).unwrap();
        store.insert_briefcase(id, &briefcase).unwrap();
        let before = stored_json(&store, id);
        drop(store);
        let store = Store::open(&path).unwrap();
        assert!(
            store.find_briefcase(id).is_err(),
            "legacy {kind:?} must not load"
        );
        let worker_id = Ulid::new();
        assert!(crate::briefcase_visibility::view_for_worker(&store, id, worker_id).is_err());
        assert!(store.worker_visibility(id, worker_id).unwrap().is_none());
        assert_eq!(
            stored_json(&store, id),
            before,
            "origin history stays intact"
        );
    }
}

#[test]
fn legacy_system_owner_sections_cannot_be_topped_up_or_audited_as_applied() {
    for kind in [
        SectionKind::Grant,
        SectionKind::Preference,
        SectionKind::Skill,
    ] {
        let store = Store::open_in_memory().unwrap();
        let (id, briefcase) = legacy_pack(kind);
        store.insert_briefcase(id, &briefcase).unwrap();
        let before = stored_json(&store, id);
        let request = TopUpRequest {
            request_id: Ulid::new(),
            section_key: "new_skill".into(),
            kind: SectionKind::Skill,
            requested_depth: 1,
            justification: "new context".into(),
        };
        let sources = SourcePool::default();
        let policy = TopUpPolicy::default();
        assert!(apply_top_up_for_grant(&store, id, &request, &policy, &sources).is_err());
        assert!(apply_top_up_for_grant_atomic(
            &store,
            id,
            &request,
            &policy,
            &sources,
            &ActionId::new("briefcase.topup"),
        )
        .is_err());
        assert_eq!(stored_json(&store, id), before);
        assert_eq!(
            store
                .count_audit_events_of_kind("briefcase.topup.applied")
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .count_audit_events_of_kind("briefcase.topup.denied")
                .unwrap(),
            0
        );
        // Even a mutation closure that could erase the invalid section must
        // not run: this is rejection, never silent provenance repair.
        let result: Result<(), StoreError> =
            store.mutate_briefcase(id, |_| panic!("must not mutate"));
        assert!(result.is_err());
    }
}

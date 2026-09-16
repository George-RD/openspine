use super::*;
use crate::identity::OwnerVerifiedProof;
use crate::skill::ceremony::{install_mined_skill, owner_decide_promotion, OwnerSkillDecision};
use openspine_schemas::skill::{Skill, SkillProvenance, SkillState, SkillVisibility};

fn pending_skill(store: &Store) -> Skill {
    let body = "Draft a concise reply in the owner's writing style.".to_string();
    let mut skill = Skill {
        id: "census-mined-skill".into(),
        schema_version: 1,
        version: 1,
        provenance: SkillProvenance::MinerDistilled,
        state: SkillState::PendingReview,
        title: "Draft replies".into(),
        content_digest: Skill::digest_of_body(&body),
        body,
        task_shape: vec!["email_reply".into()],
        visibility: SkillVisibility {
            agents: vec!["email_reply_drafter".into()],
            packs: vec![],
        },
    };
    install_mined_skill(store, &mut skill, Timestamp::now()).unwrap();
    skill
}

fn skill_counts(snapshot: &PackageOutstandingWork) -> OutstandingWorkCounts {
    OutstandingWorkSource::ALL
        .into_iter()
        .find(|source| source.as_str() == "skills.pending_review")
        .map(|source| snapshot.source(source))
        .unwrap_or_default()
}

#[test]
fn pending_skill_promotion_blocks_without_an_ordinary_grant_or_review() {
    let store = Store::open_in_memory().unwrap();
    let skill = pending_skill(&store);
    let before = store.all_audit_event_jsons().unwrap();
    let snapshot = store.package_outstanding_work(Timestamp::now()).unwrap();
    assert_eq!(
        skill_counts(&snapshot),
        OutstandingWorkCounts {
            outstanding: 1,
            terminal: 0,
            unknown: 0,
        }
    );
    assert!(!snapshot.is_quiescent());
    assert_eq!(store.count_task_grants().unwrap(), 0);
    assert_eq!(
        crate::store::skill_store::get_skill(&store, &skill.id, 1).unwrap(),
        Some(skill)
    );
    assert_eq!(store.all_audit_event_jsons().unwrap(), before);
}

#[test]
fn real_owner_skill_decisions_clear_pending_promotion_work() {
    for approve in [false, true] {
        let store = Store::open_in_memory().unwrap();
        let owner = store.bootstrap_owner_principal(42, "Owner").unwrap();
        let skill = pending_skill(&store);
        store
            .record_skill_preview(
                &skill.id,
                1,
                &owner.id.to_string(),
                &skill.content_digest,
                "MinerDistilled",
                "",
                "digest",
                "rendered preview summary",
            )
            .unwrap();
        let before = store.package_outstanding_work(Timestamp::now()).unwrap();
        assert!(!before.is_quiescent());
        owner_decide_promotion(
            &store,
            owner.id,
            &OwnerVerifiedProof::test_new(),
            &skill.id,
            1,
            if approve {
                OwnerSkillDecision::Approve
            } else {
                OwnerSkillDecision::Reject {
                    reason: "not needed".into(),
                }
            },
        )
        .unwrap();
        let after = store.package_outstanding_work(Timestamp::now()).unwrap();
        assert_eq!(
            skill_counts(&after),
            OutstandingWorkCounts {
                outstanding: 0,
                terminal: 1,
                unknown: 0,
            }
        );
        assert!(after.is_quiescent());
        assert!(!before.is_quiescent(), "captured result must remain owned");
    }
}

#[test]
fn malformed_skill_states_and_schema_versions_remain_unknown() {
    for (state, schema) in [
        ("not-json", 1_i64),
        ("\"future_state\"", 1),
        ("null", 1),
        ("\"pending_review\"", 2),
    ] {
        let store = Store::open_in_memory().unwrap();
        pending_skill(&store);
        store.with_conn_for_test(|conn| {
            conn.execute(
                "UPDATE skills SET state = ?1, schema_version = ?2",
                rusqlite::params![state, schema],
            )
            .unwrap();
        });
        let before = store.all_audit_event_jsons().unwrap();
        let snapshot = store.package_outstanding_work(Timestamp::now()).unwrap();
        assert_eq!(
            skill_counts(&snapshot),
            OutstandingWorkCounts {
                outstanding: 0,
                terminal: 0,
                unknown: 1,
            }
        );
        assert!(!snapshot.is_quiescent());
        assert_eq!(store.all_audit_event_jsons().unwrap(), before);
        store.with_conn_for_test(|conn| {
            let stored: (String, i64) = conn
                .query_row(
                    "SELECT state, schema_version FROM skills",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(stored, (state.to_string(), schema));
        });
    }
}

#[test]
fn settled_skill_states_are_not_pending_promotion_work() {
    for state in [
        SkillState::Installed,
        SkillState::Rejected,
        SkillState::Retired,
    ] {
        let store = Store::open_in_memory().unwrap();
        pending_skill(&store);
        store.with_conn_for_test(|conn| {
            conn.execute(
                "UPDATE skills SET state = ?1",
                [serde_json::to_string(&state).unwrap()],
            )
            .unwrap();
        });
        let snapshot = store.package_outstanding_work(Timestamp::now()).unwrap();
        assert_eq!(
            skill_counts(&snapshot),
            OutstandingWorkCounts {
                outstanding: 0,
                terminal: 1,
                unknown: 0,
            }
        );
        assert!(snapshot.is_quiescent());
    }
}

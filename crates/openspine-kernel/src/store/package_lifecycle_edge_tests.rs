use super::*;
use crate::identity::OwnerVerifiedProof;
use crate::skill::ceremony::{install_mined_skill, owner_decide_promotion, OwnerSkillDecision};
use openspine_schemas::skill::{Skill, SkillProvenance, SkillState, SkillVisibility};
use std::time::Duration;

fn pending_skill(store: &Store) -> Skill {
    let body = "Draft a concise reply in the owner's writing style.".to_string();
    let mut skill = Skill {
        id: "census-mined-skill".into(), schema_version: 1, version: 1,
        provenance: SkillProvenance::MinerDistilled, state: SkillState::PendingReview,
        title: "Draft replies".into(), content_digest: Skill::digest_of_body(&body), body,
        task_shape: vec!["email_reply".into()],
        visibility: SkillVisibility { agents: vec!["email_reply_drafter".into()], packs: vec![] },
    };
    install_mined_skill(store, &mut skill, Timestamp::now()).unwrap();
    skill
}

fn skill_counts(snapshot: &PackageOutstandingWork) -> OutstandingWorkCounts {
    OutstandingWorkSource::ALL.into_iter()
        .find(|source| source.as_str() == "skills.pending_review")
        .map(|source| snapshot.source(source)).unwrap_or_default()
}

#[test]
fn pending_skill_promotion_blocks_without_an_ordinary_grant_or_review() {
    let store = Store::open_in_memory().unwrap();
    let skill = pending_skill(&store);
    let before = store.all_audit_event_jsons().unwrap();
    let snapshot = store.package_outstanding_work(Timestamp::now()).unwrap();
    assert_eq!(skill_counts(&snapshot), OutstandingWorkCounts { outstanding: 1, terminal: 0, unknown: 0 });
    assert!(!snapshot.is_quiescent());
    assert_eq!(store.count_task_grants().unwrap(), 0);
    assert_eq!(crate::store::skill_store::get_skill(&store, &skill.id, 1).unwrap(), Some(skill));
    assert_eq!(store.all_audit_event_jsons().unwrap(), before);
}

#[test]
fn real_owner_skill_decisions_clear_pending_promotion_work() {
    for approve in [false, true] {
        let store = Store::open_in_memory().unwrap();
        let owner = store.bootstrap_owner_principal(42, "Owner").unwrap();
        let skill = pending_skill(&store);
        store.record_skill_preview(
            &skill.id, 1, &owner.id.to_string(), &skill.content_digest,
            "MinerDistilled", "", "digest", "rendered preview summary",
        ).unwrap();
        let before = store.package_outstanding_work(Timestamp::now()).unwrap();
        assert!(!before.is_quiescent());
        owner_decide_promotion(
            &store, owner.id, &OwnerVerifiedProof::test_new(), &skill.id, 1,
            if approve { OwnerSkillDecision::Approve } else { OwnerSkillDecision::Reject { reason: "not needed".into() } },
        ).unwrap();
        let after = store.package_outstanding_work(Timestamp::now()).unwrap();
        assert_eq!(skill_counts(&after), OutstandingWorkCounts { outstanding: 0, terminal: 1, unknown: 0 });
        assert!(after.is_quiescent());
        assert!(!before.is_quiescent(), "captured result must remain owned");
    }
}

#[test]
fn malformed_skill_states_and_schema_versions_remain_unknown() {
    for (state, schema) in [("not-json", 1_i64), ("\"future_state\"", 1), ("null", 1), ("\"pending_review\"", 2)] {
        let store = Store::open_in_memory().unwrap();
        pending_skill(&store);
        store.with_conn_for_test(|conn| {
            conn.execute("UPDATE skills SET state = ?1, schema_version = ?2", rusqlite::params![state, schema]).unwrap();
        });
        let before = store.all_audit_event_jsons().unwrap();
        let snapshot = store.package_outstanding_work(Timestamp::now()).unwrap();
        assert_eq!(skill_counts(&snapshot), OutstandingWorkCounts { outstanding: 0, terminal: 0, unknown: 1 });
        assert!(!snapshot.is_quiescent());
        assert_eq!(store.all_audit_event_jsons().unwrap(), before);
        store.with_conn_for_test(|conn| {
            let stored: (String, i64) = conn.query_row("SELECT state, schema_version FROM skills", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
            assert_eq!(stored, (state.to_string(), schema));
        });
    }
}

#[test]
fn settled_skill_states_are_not_pending_promotion_work() {
    for state in [SkillState::Installed, SkillState::Rejected, SkillState::Retired] {
        let store = Store::open_in_memory().unwrap();
        pending_skill(&store);
        store.with_conn_for_test(|conn| {
            conn.execute("UPDATE skills SET state = ?1", [serde_json::to_string(&state).unwrap()]).unwrap();
        });
        let snapshot = store.package_outstanding_work(Timestamp::now()).unwrap();
        assert_eq!(skill_counts(&snapshot), OutstandingWorkCounts { outstanding: 0, terminal: 1, unknown: 0 });
        assert!(snapshot.is_quiescent());
    }
}

async fn ordinary_proposal() -> (crate::pipeline::AppState, openspine_schemas::grant::TaskGrant, ulid::Ulid, wiremock::MockServer) {
    use crate::test_support::fixtures::{owner_update, seed_owner_history, test_state_with_telegram};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true, "result": { "message_id": 1, "date": 0,
                "chat": {"id": 555, "type": "private"}, "text": "sent" }
        }))).mount(&server).await;
    let state = test_state_with_telegram(crate::telegram::TelegramConnector::with_api_url(
        "test-token".into(), server.uri().parse().unwrap(),
    ));
    let grant = crate::pipeline::handle_owner_update(&state, &owner_update("hello lyra")).await.unwrap().unwrap();
    seed_owner_history(&state, &grant);
    let (_, _, surface) = state.store.find_task_grant_by_id(grant.id).unwrap().unwrap();
    let payload = serde_json::json!({
        "kind": "route", "yaml": "id: census-ordinary-route\nschema_version: 1\nlifecycle_state: proposed\npriority: 100\nagent: main_assistant_agent\nworkflow: owner_control_conversation\ncapability_pack: owner_control_basic_pack\n"
    });
    let result = crate::api::artifact_propose::dispatch_artifact_propose(
        &state, &grant, &"artifact.propose".into(), &surface, Some(&payload),
    ).await.unwrap();
    let request_id = result["action_request_id"].as_str().unwrap().parse().unwrap();
    state.store.with_conn_for_test(|conn| {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM owner_reviews", [], |row| row.get(0)).unwrap();
        assert_eq!(count, 0, "ordinary proposal uses only its digest-bound callback");
    });
    (state, grant, request_id, server)
}

#[tokio::test]
async fn ordinary_proposal_becomes_terminal_when_its_callback_grant_expires() {
    let (state, grant, request, _server) = ordinary_proposal().await;
    let before_expiry = state.store.package_outstanding_work_with_reviews(grant.expires_at - Duration::from_nanos(1), &state.artifacts).unwrap();
    assert_eq!(before_expiry.source(OutstandingWorkSource::ProposedArtifacts), OutstandingWorkCounts { outstanding: 1, terminal: 0, unknown: 0 });
    let audit = state.store.all_audit_event_jsons().unwrap();
    let request_before = state.store.find_action_request(request).unwrap();
    let after = state.store.package_outstanding_work_with_reviews(grant.expires_at, &state.artifacts).unwrap();
    assert_eq!(after.source(OutstandingWorkSource::ActionRequests), OutstandingWorkCounts { outstanding: 0, terminal: 1, unknown: 0 });
    assert_eq!(after.source(OutstandingWorkSource::ProposedArtifacts), OutstandingWorkCounts { outstanding: 0, terminal: 1, unknown: 0 });
    assert_eq!(state.store.find_action_request(request).unwrap(), request_before);
    assert_eq!(state.store.all_audit_event_jsons().unwrap(), audit);
    assert_eq!(state.store.find_proposed_artifact_by_action_request(request).unwrap().unwrap().state, openspine_schemas::artifact::Lifecycle::ReviewRequired);
}

#[tokio::test]
async fn ordinary_proposal_approval_path_survives_the_grant_sweep() {
    let (state, grant, request, _server) = ordinary_proposal().await;
    let as_of = grant.expires_at + Duration::from_secs(172800);
    state.store.sweep_expired_grants(as_of).unwrap();
    assert!(state.store.find_task_grant_by_id(grant.id).unwrap().is_none());
    let audit = state.store.all_audit_event_jsons().unwrap();
    let request_before = state.store.find_action_request(request).unwrap();
    let snapshot = state.store.package_outstanding_work_with_reviews(as_of, &state.artifacts).unwrap();
    assert_eq!(snapshot.source(OutstandingWorkSource::ProposedArtifacts), OutstandingWorkCounts { outstanding: 0, terminal: 1, unknown: 0 });
    assert_eq!(state.store.find_action_request(request).unwrap(), request_before);
    assert_eq!(state.store.all_audit_event_jsons().unwrap(), audit);
}

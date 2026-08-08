//! Scheduled-miner grant resolution cases, split from
//! `artifact_propose_miner_tests.rs` for the 500-line gate.

use super::*;
use crate::reflection_miner_runtime::{
    find_active_grant_by_route, reflection_miner_tick, REFLECTION_SCHEDULED_MINER_ROUTE,
    REFLECTION_SCHEDULED_SUBMITTER_ROUTE,
};

#[tokio::test]
async fn scheduled_reflection_miner_tick_mines_repeated_approval() {
    use openspine_schemas::action::{ActionId, GateDecision};
    use openspine_schemas::artifact::ArtifactRef;
    use openspine_schemas::digest::Digest;

    let server = MockServer::start().await;
    let token = "driver-test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{token}/SendMessage")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": OWNER_CHAT_ID, "type": "private"},
                "text": "sent",
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let connector =
        TelegramConnector::with_api_url(token.to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);

    // First tick composes the scheduled grants from active artifacts; nothing
    // is learnable from an empty audit ledger.
    let dispatched_first = reflection_miner_tick(&state).await.unwrap();
    assert_eq!(dispatched_first, 0);

    let miner_grant = find_active_grant_by_route(&state, REFLECTION_SCHEDULED_MINER_ROUTE)
        .unwrap()
        .expect("miner grant must have been composed on first tick")
        .0;
    let submitter_grant = find_active_grant_by_route(&state, REFLECTION_SCHEDULED_SUBMITTER_ROUTE)
        .unwrap()
        .expect("submitter grant must have been composed on first tick")
        .0;
    assert_eq!(miner_grant.agent_id, "reflection_miner_agent");
    assert_eq!(miner_grant.workflow_id, "reflection_miner_scheduled");
    assert_eq!(miner_grant.capability_pack_id, "reflection_miner_pack");
    assert!(miner_grant.output_channels.is_empty());
    assert!(!miner_grant.authority_sources.is_empty());
    assert_eq!(submitter_grant.agent_id, "reflection_submitter_agent");
    assert_eq!(
        submitter_grant.workflow_id,
        "reflection_submitter_scheduled"
    );
    assert_eq!(
        submitter_grant.capability_pack_id,
        "reflection_submitter_pack"
    );
    let owner_grant = handle_owner_update(&state, &owner_update("capture owner history"))
        .await
        .unwrap()
        .expect("owner update must compose an owner-control grant");
    seed_owner_history(&state, &owner_grant);

    // Seed real, kernel-verifiable owner evidence. The scheduled miner packs
    // allowed events across this owner's grants into its own bounded scope.
    let pending_ref = ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
        schema_version: 1,
    };
    let unapproved_ref = ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
        schema_version: 1,
    };
    let approved_action = ActionId::new("openspine.status.read");
    let decision = GateDecision::Allow;
    for _ in 0..3 {
        state
            .store
            .append_audit(
                "action.gated",
                Some(&approved_action),
                Some(&decision),
                None,
                Some(owner_grant.id),
                &[],
                std::slice::from_ref(&unapproved_ref),
            )
            .unwrap();
    }
    for _ in 0..2 {
        state
            .store
            .append_audit(
                "action.gated",
                Some(&approved_action),
                Some(&decision),
                Some(crate::store::OWNER_APPROVAL_GATE_REASON),
                Some(owner_grant.id),
                &[],
                std::slice::from_ref(&pending_ref),
            )
            .unwrap();
    }

    // Second tick derives the observation from the verified audit slice and
    // dispatches one standing_rule proposal through the normal lifecycle.
    let dispatched = reflection_miner_tick(&state).await.unwrap();
    assert_eq!(dispatched, 1);
    assert!(
        state
            .store
            .proposed_artifact_exists("standing_rule", pending_ref.digest.as_str(), 1)
            .unwrap(),
        "a standing_rule proposal must have been persisted through the lifecycle"
    );
    assert!(
        state
            .store
            .count_audit_events_of_kind("reflection.miner.provenance")
            .unwrap()
            >= 1
    );
}

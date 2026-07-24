use super::*;
use crate::reflection_miner_runtime::{
    find_active_grant_by_route, reflection_miner_tick, REFLECTION_SCHEDULED_MINER_ROUTE,
    REFLECTION_SCHEDULED_SUBMITTER_ROUTE,
};

#[tokio::test]
async fn artifact_propose_accepts_persona_kind() {
    let server = MockServer::start().await;
    let token = "test-token";
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
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    seed_owner_history(&state, &grant);

    let persona_yaml = "id: digest_brief_default\n\
        schema_version: 1\n\
        version: 2\n\
        lifecycle_state: proposed\n\
        guidance: Present a compact decision brief before detail\n";
    let payload = json!({"kind": "persona", "yaml": persona_yaml});
    let result = dispatch_artifact_propose(
        &state,
        &grant,
        &ActionId::new("artifact.propose"),
        OWNER_CHAT_ID,
        Some(&payload),
    )
    .await
    .expect("a well-formed persona correction proposal must be accepted");
    assert_eq!(result["proposed"], true);

    let action_request_id: Ulid = result["action_request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let row = state
        .store
        .find_proposed_artifact_by_action_request(action_request_id)
        .unwrap()
        .expect("dispatch must persist a proposed_artifacts row");
    assert_eq!(row.kind, "persona");
    assert_eq!(row.artifact_id, "digest_brief_default");
    assert_eq!(row.version, 2);
    assert_eq!(row.state, Lifecycle::ReviewRequired);
}

#[tokio::test]
async fn artifact_propose_accepts_miner_reflection_correction_route() {
    let server = MockServer::start().await;
    let token = "test-token";
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
    let submitting_grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a submitting grant");
    seed_owner_history(&state, &submitting_grant);

    let now = Timestamp::now();
    let miner_id = Ulid::new();
    let miner_grant = TaskGrant {
        id: miner_id,
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user: "owner".into(),
        purpose: "scheduled reflection miner".into(),
        issued_by: "kernel".into(),
        issued_at: now,
        expires_at: now + std::time::Duration::from_secs(60),
        event_id: Ulid::new(),
        route_id: "reflection_route".into(),
        agent_id: "reflection_miner".into(),
        workflow_id: "reflection_workflow".into(),
        capability_pack_id: "reflection_pack".into(),
        authority_sources: vec![],
        selection_tokens: vec![],
        allowed_actions: vec![ActionId::new("model.generate:approved_provider")],
        approval_required_actions: vec![],
        denied_actions: vec![],
        allowed_egress_classes: vec![],
        output_channels: vec![],
        limits: GrantLimits {
            max_model_calls: 1,
            max_artifacts: 4,
            max_runtime_seconds: 60,
        },
        task_token: "miner-token".into(),
        root_grant_id: miner_id,
        parent_grant_id: None,
        mode: GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
        persona_id: None,
    };
    // The owner-correction grant is the normal lifecycle provenance anchor;
    // use its exact event/exchange for the miner observation so activation's
    // ProducedBy row and the miner proposal provenance are identical.
    let (_, source_exchange, _) = state
        .store
        .find_task_grant_by_id(submitting_grant.id)
        .unwrap()
        .expect("submitting grant provenance must exist");
    let source_event_id = submitting_grant.event_id;
    let briefcase = MinerBriefcase::scoped(
        miner_id,
        "owner-correction:42",
        vec![AuditTrailEntry {
            scope: "owner-correction:42".into(),
            artifact_id: "digest_brief_default".into(),
            event_id: source_event_id,
            exchange: source_exchange.clone(),
            classification: DataClassification::Private,
        }],
    )
    .unwrap();
    let miner = OrdinaryMinerGrant::admit(&miner_grant, &Constraints::default(), briefcase)
        .expect("kernel-authenticated ordinary miner grant must admit");
    let provenance = ReflectionProvenance {
        source_event_id,
        source_exchange,
    };
    let observation = CorrectionObservation::persona_digest(
        2,
        "Present a compact decision brief before detail",
        "The owner asked for a faster scan",
        Some("Do not bury the decision in detail".into()),
        provenance.clone(),
    );
    let proposal = ReflectionMiner
        .mine(
            &miner,
            &[openspine_schemas::reflection_miner::ReflectionObservation::Correction(observation)],
        )
        .unwrap()
        .remove(0);
    assert_eq!(proposal.provenance, provenance);
    let ReflectionProposalBody::InstructionRewrite { eval_probe, .. } = &proposal.body else {
        panic!("miner correction must be a positive rewrite");
    };
    assert!(
        eval_probe.is_some(),
        "negative constraint must remain an eval probe"
    );

    let payload = proposal
        .to_proposal_payload()
        .expect("persona proposal must serialize for artifact.propose");
    let result = dispatch_artifact_propose(
        &state,
        &submitting_grant,
        &ActionId::new("artifact.propose"),
        OWNER_CHAT_ID,
        Some(&payload),
    )
    .await
    .expect("the miner proposal must enter the normal lifecycle");
    let action_request_id: Ulid = result["action_request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let row = state
        .store
        .find_proposed_artifact_by_action_request(action_request_id)
        .unwrap()
        .expect("the normal lifecycle must persist the miner proposal");
    assert_eq!(row.kind, "persona");
    assert_eq!(row.artifact_id, "digest_brief_default");
    assert_eq!(row.version, 2);
    assert_eq!(row.state, Lifecycle::ReviewRequired);
}

#[tokio::test]
async fn reflection_miner_runtime_wires_ad135_route_through_lifecycle() {
    use openspine_schemas::action::GateDecision;
    use openspine_schemas::artifact::ArtifactRef;
    use openspine_schemas::digest::Digest;
    use openspine_schemas::policy::Constraints;
    use openspine_schemas::reflection_miner::{
        CorrectionObservation, ReflectionObservation, ReflectionProvenance,
    };
    let server = MockServer::start().await;
    let token = "runtime-test-token";
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
    let submitting_grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a submitting grant");
    seed_owner_history(&state, &submitting_grant);

    let now = Timestamp::now();
    let pending_ref = ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", "1".repeat(64))).unwrap(),
        schema_version: 1,
    };
    let mut miner_grant = TaskGrant {
        id: Ulid::new(),
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user: state.owner_principal_id.to_string(),
        purpose: "scheduled reflection miner".into(),
        issued_by: "kernel".into(),
        issued_at: now,
        expires_at: now + std::time::Duration::from_secs(60),
        event_id: Ulid::new(),
        route_id: "reflection_route".into(),
        agent_id: "reflection_miner".into(),
        workflow_id: "reflection_workflow".into(),
        capability_pack_id: "reflection_pack".into(),
        authority_sources: vec![],
        selection_tokens: vec![],
        allowed_actions: vec![ActionId::new("model.generate:approved_provider")],
        approval_required_actions: vec![],
        denied_actions: vec![],
        allowed_egress_classes: vec![],
        output_channels: vec![],
        limits: GrantLimits {
            max_model_calls: 2,
            max_artifacts: 4,
            max_runtime_seconds: 60,
        },
        task_token: "runtime-miner-token".into(),
        root_grant_id: Ulid::new(),
        parent_grant_id: None,
        mode: GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
        persona_id: None,
    };
    miner_grant.root_grant_id = miner_grant.id;
    miner_grant.seal_root(&crate::grant_hmac_key().expect("test grant key"));
    state
        .store
        .insert_task_grant(&miner_grant, &pending_ref, OWNER_CHAT_ID)
        .unwrap();
    crate::reflection_miner_runtime::reserve_model_call(&state, miner_grant.id, 1).unwrap();

    let approval_action = ActionId::new("openspine.status.read");
    let approval_decision = GateDecision::Allow;
    let _approval = state
        .store
        .append_audit(
            "action.gate_decision",
            Some(&approval_action),
            Some(&approval_decision),
            Some("approved"),
            Some(miner_grant.id),
            std::slice::from_ref(&pending_ref),
            &[],
        )
        .unwrap();
    state
        .store
        .append_audit(
            "action.gate_decision",
            Some(&approval_action),
            Some(&approval_decision),
            Some("approved"),
            Some(miner_grant.id),
            std::slice::from_ref(&pending_ref),
            &[],
        )
        .unwrap();
    let correction_ref = ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", "2".repeat(64))).unwrap(),
        schema_version: 1,
    };
    let correction = state
        .store
        .append_audit(
            "owner.correction",
            None,
            None,
            Some("owner asked for a faster scan"),
            Some(submitting_grant.id),
            std::slice::from_ref(&correction_ref),
            &[],
        )
        .unwrap();

    let observations = [ReflectionObservation::Correction(
        CorrectionObservation::persona_digest(
            2,
            "Present a compact decision brief before detail",
            "The owner asked for a faster scan",
            Some("Do not bury the decision in detail".into()),
            ReflectionProvenance {
                source_event_id: correction.id,
                source_exchange: correction_ref,
            },
        ),
    )];
    let dispatched = crate::reflection_miner_runtime::run_reflection_miner(
        &state,
        &observations,
        &Constraints::default(),
        miner_grant.id,
        submitting_grant.id,
        OWNER_CHAT_ID,
    )
    .await
    .unwrap();
    // `run_reflection_miner` reserved one model call internally; the grant's
    // total allowance (2) is now exhausted, so a further reservation fails.
    assert!(matches!(
        crate::reflection_miner_runtime::reserve_model_call(&state, miner_grant.id, 1),
        Err(crate::reflection_miner_runtime::MinerRuntimeError::ModelBudgetExhausted)
    ));
    assert_eq!(dispatched, 1);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("reflection.miner.model_gated")
            .unwrap(),
        1
    );
    assert!(
        state
            .store
            .count_audit_events_of_kind("reflection.miner.provenance")
            .unwrap()
            >= 1
    );
}
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

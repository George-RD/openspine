// openspine:allow-large-module reason: Reconfirmation integration tests cover all owner-tap failure and retry paths.
use super::artifact_propose::dispatch_artifact_propose;
use super::artifact_propose_tests::route_yaml;
use super::dispatch_tests::OWNER_CHAT_ID;
use crate::pipeline::handle_owner_update;
use crate::store::learned_artifacts::CompatibilityStatus;
use crate::telegram::{CallbackQueryUpdate, TelegramConnector, TelegramUpdate};
use crate::test_support::fixtures::{owner_update, seed_owner_history, test_state_with_telegram};
use jiff::Timestamp;
use openspine_schemas::action::{ActionId, ActionRequest};
use openspine_schemas::digest::{digest_of, digest_of_bytes};
use serde_json::json;
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
fn approve_callback_update(action_request_id: Ulid) -> TelegramUpdate {
    let mut update = owner_update("");
    update.text = None;
    update.callback_query = Some(CallbackQueryUpdate {
        id: "cb-1".to_string(),
        data: Some(format!("approve_draft:{action_request_id}")),
    });
    update
}
fn telegram_stub(server: &MockServer) -> TelegramConnector {
    TelegramConnector::with_api_url("test-token".to_string(), server.uri().parse().unwrap())
}
async fn mount_send_message_ok(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": OWNER_CHAT_ID, "type": "private"},
                "text": "sent",
            }
        })))
        .mount(server)
        .await;
}
#[tokio::test]
async fn reconfirm_tap_restores_orphaned_artifact() {
    let server = MockServer::start().await;
    mount_send_message_ok(&server).await;
    let state = test_state_with_telegram(telegram_stub(&server));
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("grant composed");
    seed_owner_history(&state, &grant);
    let payload =
        json!({"kind": "route", "yaml": route_yaml("reconfirm_target_route", "proposed")});
    let result = dispatch_artifact_propose(
        &state,
        &grant,
        &ActionId::new("artifact.propose"),
        OWNER_CHAT_ID,
        Some(&payload),
    )
    .await
    .expect("proposal accepted");
    let propose_request_id: Ulid = result["action_request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    handle_owner_update(&state, &approve_callback_update(propose_request_id))
        .await
        .unwrap();

    let learned = state.store.list_learned_artifacts().unwrap();
    assert_eq!(learned.len(), 1);
    assert_eq!(learned[0].artifact_id, "reconfirm_target_route");
    assert_eq!(learned[0].compatibility, CompatibilityStatus::Compatible);

    let removed_agent = {
        let mut registry = state.registry.write();
        let removed = registry.agents.remove("main_assistant_agent");
        registry.routes.retain(|r| r.id != "reconfirm_target_route");
        removed.expect("base agent fixture exists")
    };
    let overlay_path =
        state
            .overlay_dir
            .join("routes")
            .join(crate::artifact_loader::overlay_filename(
                "reconfirm_target_route",
                1,
            ));
    let yaml = std::fs::read(&overlay_path).unwrap();
    let review_ref = state.artifacts.put(&yaml).unwrap();
    let request_id = Ulid::new();
    state
        .store
        .mark_reconfirmation_required(
            "route",
            "reconfirm_target_route",
            1,
            request_id,
            review_ref.digest.as_str(),
        )
        .unwrap();
    let target_digest = digest_of(&json!({
        "kind": "route",
        "artifact_id": "reconfirm_target_route",
        "version": 1,
    }));
    let reconfirm_request = ActionRequest {
        id: request_id,
        task_grant_id: Ulid::new(),
        action: ActionId::new("artifact.reconfirm"),
        target_ref: None,
        payload_ref: Some(review_ref.clone()),
        target_digest: Some(target_digest),
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        requested_at: Timestamp::now(),
        schema_version: 1,
    };
    state
        .store
        .insert_action_request(&reconfirm_request)
        .unwrap();

    state
        .registry
        .write()
        .agents
        .insert("main_assistant_agent".to_string(), removed_agent);
    handle_owner_update(&state, &approve_callback_update(request_id))
        .await
        .unwrap();

    {
        let registry = state.registry.read();
        assert!(registry
            .routes
            .iter()
            .any(|r| r.id == "reconfirm_target_route" && r.version == 1));
    }
    let learned = state.store.list_learned_artifacts().unwrap();
    assert_eq!(learned[0].compatibility, CompatibilityStatus::OwnerAccepted);
    let anchor = learned[0]
        .accepted_via
        .as_ref()
        .expect("every successful reconfirm records a ReconfirmAnchor");
    assert_eq!(anchor.request_id, request_id);
    assert_eq!(anchor.reviewed_ref, review_ref);
    assert!(matches!(
        learned[0].provenance,
        crate::store::learned_artifacts::Provenance::ProducedBy { .. }
    ));
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("artifact.reconfirmed")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn reconfirm_tap_refuses_tampered_yaml() {
    let server = MockServer::start().await;
    mount_send_message_ok(&server).await;
    let state = test_state_with_telegram(telegram_stub(&server));

    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("grant composed");
    seed_owner_history(&state, &grant);
    let payload = json!({"kind": "route", "yaml": route_yaml("tamper_target_route", "proposed")});
    let result = dispatch_artifact_propose(
        &state,
        &grant,
        &ActionId::new("artifact.propose"),
        OWNER_CHAT_ID,
        Some(&payload),
    )
    .await
    .expect("proposal accepted");
    let propose_request_id: Ulid = result["action_request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    handle_owner_update(&state, &approve_callback_update(propose_request_id))
        .await
        .unwrap();

    {
        let mut registry = state.registry.write();
        registry.agents.remove("main_assistant_agent");
        registry.routes.retain(|r| r.id != "tamper_target_route");
    }
    let overlay_path =
        state
            .overlay_dir
            .join("routes")
            .join(crate::artifact_loader::overlay_filename(
                "tamper_target_route",
                1,
            ));
    let yaml = std::fs::read(&overlay_path).unwrap();
    let approved_digest = digest_of_bytes(&yaml).to_string();
    let mut tampered = yaml.clone();
    tampered.extend_from_slice(b"\n# tampered before compat\n");
    std::fs::write(&overlay_path, &tampered).unwrap();
    let review_ref = state.artifacts.put(&tampered).unwrap();
    let request_id = Ulid::new();
    state
        .store
        .mark_reconfirmation_required(
            "route",
            "tamper_target_route",
            1,
            request_id,
            &approved_digest,
        )
        .unwrap();
    let reconfirm_request = ActionRequest {
        id: request_id,
        task_grant_id: Ulid::new(),
        action: ActionId::new("artifact.reconfirm"),
        target_ref: None,
        payload_ref: Some(review_ref),
        target_digest: Some(digest_of(
            &json!({"kind":"route","artifact_id":"tamper_target_route","version":1}),
        )),
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        requested_at: Timestamp::now(),
        schema_version: 1,
    };
    state
        .store
        .insert_action_request(&reconfirm_request)
        .unwrap();

    handle_owner_update(&state, &approve_callback_update(request_id))
        .await
        .unwrap();

    {
        let registry = state.registry.read();
        assert!(!registry
            .routes
            .iter()
            .any(|r| r.id == "tamper_target_route"));
    }
    let learned = state.store.list_learned_artifacts().unwrap();
    assert_eq!(
        learned[0].compatibility,
        CompatibilityStatus::ReconfirmationRequired
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("artifact.reconfirm_digest_mismatch")
            .unwrap(),
        1
    );
}

async fn orphaned_reconfirm_fixture(
    state: &crate::pipeline::AppState,
    artifact_id: &str,
) -> (Ulid, openspine_schemas::artifact::ArtifactRef) {
    let grant = handle_owner_update(state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("grant composed");
    seed_owner_history(state, &grant);
    let payload = json!({"kind": "route", "yaml": route_yaml(artifact_id, "proposed")});
    let result = dispatch_artifact_propose(
        state,
        &grant,
        &ActionId::new("artifact.propose"),
        OWNER_CHAT_ID,
        Some(&payload),
    )
    .await
    .expect("proposal accepted");
    let propose_request_id: Ulid = result["action_request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    handle_owner_update(state, &approve_callback_update(propose_request_id))
        .await
        .unwrap();
    let overlay_path = state
        .overlay_dir
        .join("routes")
        .join(crate::artifact_loader::overlay_filename(artifact_id, 1));
    let yaml = std::fs::read(&overlay_path).unwrap();
    let review_ref = state.artifacts.put(&yaml).unwrap();
    let request_id = Ulid::new();
    state
        .store
        .mark_reconfirmation_required(
            "route",
            artifact_id,
            1,
            request_id,
            review_ref.digest.as_str(),
        )
        .unwrap();
    {
        let mut registry = state.registry.write();
        registry.routes.retain(|r| r.id != artifact_id);
    }
    let target_digest = digest_of(&json!({"kind":"route","artifact_id":artifact_id,"version":1}));
    let reconfirm_request = ActionRequest {
        id: request_id,
        task_grant_id: Ulid::new(),
        action: ActionId::new("artifact.reconfirm"),
        target_ref: None,
        payload_ref: Some(review_ref.clone()),
        target_digest: Some(target_digest),
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        requested_at: Timestamp::now(),
        schema_version: 1,
    };
    state
        .store
        .insert_action_request(&reconfirm_request)
        .unwrap();
    (request_id, review_ref)
}

#[tokio::test]
async fn reconfirm_transaction_failure_leaves_registry_unchanged_and_retries() {
    let server = MockServer::start().await;
    mount_send_message_ok(&server).await;
    let state = test_state_with_telegram(telegram_stub(&server));
    let (request_id, _review_ref) =
        orphaned_reconfirm_fixture(&state, "failure_target_route").await;

    state.store.set_fail_next_owner_reconfirmation(true);
    let first = handle_owner_update(&state, &approve_callback_update(request_id)).await;
    assert!(
        first.is_err(),
        "injected transaction failure must surface as Err"
    );

    {
        let registry = state.registry.read();
        assert!(!registry
            .routes
            .iter()
            .any(|r| r.id == "failure_target_route"));
    }
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("artifact.reconfirmed")
            .unwrap(),
        0
    );
    assert!(!state.store.is_action_request_used(request_id).unwrap());

    handle_owner_update(&state, &approve_callback_update(request_id))
        .await
        .unwrap();
    {
        let registry = state.registry.read();
        assert!(registry
            .routes
            .iter()
            .any(|r| r.id == "failure_target_route" && r.version == 1));
    }
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("artifact.reconfirmed")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn reconfirm_tap_refuses_base_namespace_collision() {
    let server = MockServer::start().await;
    mount_send_message_ok(&server).await;
    let mut state = test_state_with_telegram(telegram_stub(&server));
    let (request_id, _review_ref) =
        orphaned_reconfirm_fixture(&state, "collision_target_route").await;

    state
        .base_artifact_ids
        .insert(("route".to_string(), "collision_target_route".to_string()));

    handle_owner_update(&state, &approve_callback_update(request_id))
        .await
        .unwrap();

    {
        let registry = state.registry.read();
        assert!(!registry
            .routes
            .iter()
            .any(|r| r.id == "collision_target_route"));
    }
    let learned = state.store.list_learned_artifacts().unwrap();
    assert_eq!(
        learned[0].compatibility,
        CompatibilityStatus::ReconfirmationRequired
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("artifact.reconfirm_namespace_collision")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("artifact.reconfirmed")
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn reconfirm_legacy_migration_establishes_produced_by_and_proposal() {
    let server = MockServer::start().await;
    mount_send_message_ok(&server).await;
    let state = test_state_with_telegram(telegram_stub(&server));

    let yaml = route_yaml("legacy_target_route", "proposed");
    let overlay_path = state
        .overlay_dir
        .join("routes")
        .join("legacy_target_route-v1.yaml");
    std::fs::create_dir_all(overlay_path.parent().unwrap()).unwrap();
    std::fs::write(&overlay_path, yaml.as_bytes()).unwrap();
    let bytes = std::fs::read(&overlay_path).unwrap();
    let review_ref = state.artifacts.put(&bytes).unwrap();
    let request_id = Ulid::new();
    let row = crate::store::learned_artifacts::LearnedArtifact {
        kind: "route".into(),
        artifact_id: "legacy_target_route".into(),
        version: 1,
        namespace: openspine_schemas::artifact::ArtifactNamespace::Overlay,
        provenance: crate::store::learned_artifacts::Provenance::LegacyMigration {
            discovered_at: Timestamp::now(),
        },
        accepted_via: None,
        learned_at: Timestamp::now(),
        compatibility: CompatibilityStatus::ReconfirmationRequired,
        nomination: crate::store::learned_artifacts::NominationStatus::None,
        pending_reconfirmation_id: Some(request_id),
        pending_yaml_digest: Some(digest_of_bytes(&bytes).to_string()),
        accepted_dependency_fingerprint: None,
        source_path: Some(overlay_path.to_string_lossy().into_owned()),
        accepted_base_epoch: None,
    };
    state.store.record_learned_artifact(&row).unwrap();
    let target_digest = digest_of(&json!({
        "kind":"route","artifact_id":"legacy_target_route","version":1
    }));
    let reconfirm_request = ActionRequest {
        id: request_id,
        task_grant_id: Ulid::new(),
        action: ActionId::new("artifact.reconfirm"),
        target_ref: None,
        payload_ref: Some(review_ref.clone()),
        target_digest: Some(target_digest),
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        requested_at: Timestamp::now(),
        schema_version: 1,
    };
    state
        .store
        .insert_action_request(&reconfirm_request)
        .unwrap();

    handle_owner_update(&state, &approve_callback_update(request_id))
        .await
        .unwrap();

    let learned = state.store.list_learned_artifacts().unwrap();
    assert_eq!(learned[0].compatibility, CompatibilityStatus::OwnerAccepted);
    assert!(matches!(
        learned[0].provenance,
        crate::store::learned_artifacts::Provenance::ProducedBy { .. }
    ));
    assert!(learned[0].accepted_via.is_some());
    let proposal = state
        .store
        .find_proposed_artifact("route", "legacy_target_route", 1)
        .unwrap()
        .expect("fresh legacy proposal was minted");
    assert_eq!(
        proposal.state,
        openspine_schemas::artifact::Lifecycle::Active
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("artifact.proposed")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("artifact.activated")
            .unwrap(),
        1
    );
    {
        let registry = state.registry.read();
        assert!(registry
            .routes
            .iter()
            .any(|r| r.id == "legacy_target_route" && r.version == 1));
    }
}

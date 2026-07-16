use serde_json::json;
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use openspine_schemas::artifact::Lifecycle;

use super::artifact_activation_tests::approve_callback_update;
use super::artifact_propose::dispatch_artifact_propose;
use super::dispatch_tests::OWNER_CHAT_ID;
use crate::config::{ProviderAuth, ProviderConfig, ProviderKind};
use crate::model_gateway::ProviderClient;
use crate::pipeline::handle_owner_update;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::{owner_update, seed_owner_history, test_state_with_telegram};

#[tokio::test]
async fn injected_activation_tx_failure_keeps_approved_old_state() {
    let telegram_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {"message_id": 1, "date": 0, "chat": {"id": OWNER_CHAT_ID, "type": "private"}, "text": "sent"}
        })))
        .mount(&telegram_server)
        .await;
    let provider_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "READY OWNER SAFE"}]
        })))
        .mount(&provider_server)
        .await;

    let mut state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".to_string(),
        telegram_server.uri().parse().unwrap(),
    ));
    let provider = ProviderConfig {
        id: "swapped-provider".to_string(),
        kind: ProviderKind::Anthropic,
        base_url: Some(provider_server.uri()),
        model: "new-model".to_string(),
        auth: ProviderAuth::ApiKey {
            env: "UNUSED".into(),
        },
    };
    state.provider_pool.insert(
        provider.id.clone(),
        ProviderClient::from_config(&provider, "new-key".into()),
    );
    state.provider_config_digests.insert(
        provider.id.clone(),
        crate::config::provider_config_digest(&provider),
    );

    let grant = handle_owner_update(&state, &owner_update("swap model"))
        .await
        .unwrap()
        .unwrap();
    seed_owner_history(&state, &grant);
    let proposal = json!({
        "kind": "model_swap",
        "yaml": "id: base\nversion: 1\nlifecycle_state: proposed\nrole: base\ntarget_provider_id: swapped-provider\ngolden_set_id: model_swap_default\n"
    });
    let result = dispatch_artifact_propose(&state, &grant, OWNER_CHAT_ID, Some(&proposal))
        .await
        .unwrap();
    let request_id: Ulid = result["action_request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    state.store.fail_next_activation_tx_for_test();
    let callback = handle_owner_update(&state, &approve_callback_update(request_id)).await;
    assert!(
        callback.is_err(),
        "injected activation failure must surface"
    );

    let row = state
        .store
        .find_proposed_artifact_by_action_request(request_id)
        .unwrap()
        .unwrap();
    assert_eq!(row.state, Lifecycle::Approved);
    crate::model_swap_recovery::reconcile_model_swap_overlay(
        &state.store,
        &state.artifacts,
        &state.overlay_dir,
    )
    .unwrap();
    let mut restarted_registry = crate::artifact_loader::ArtifactRegistry::default();
    crate::artifact_loader::load_registry_into(&mut restarted_registry, &state.overlay_dir)
        .unwrap();
    assert!(restarted_registry.model_swaps.is_empty());
    assert!(state.registry.read().model_swaps.is_empty());
    assert_eq!(
        state
            .active_model_providers
            .read()
            .get(&openspine_schemas::model_swap::ModelRole::Base)
            .map(String::as_str),
        Some("test-provider")
    );
    assert!(!state
        .overlay_dir
        .join("model_swaps/base-v1.pending")
        .exists());
    assert!(!state.overlay_dir.join("model_swaps/base-v1.yaml").exists());
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("artifact.activated")
            .unwrap(),
        0
    );
}

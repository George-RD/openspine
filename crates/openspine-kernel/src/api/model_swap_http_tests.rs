use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::dispatch_tests::OWNER_CHAT_ID;
use super::tests::{post_action, start_server};
use crate::config::{ProviderAuth, ProviderConfig, ProviderKind};
use crate::model_gateway::ProviderClient;
use crate::pipeline::handle_owner_update;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::{owner_update, test_state_with_telegram};

#[tokio::test]
async fn http_model_swap_propose_persists_allow_before_provider_effect() {
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
    let store = state.store.clone();
    let all_requests_after_allow = Arc::new(AtomicBool::new(true));
    let all_requests_after_allow_for_responder = all_requests_after_allow.clone();
    let provider_request_count = Arc::new(AtomicUsize::new(0));
    let provider_request_count_for_responder = provider_request_count.clone();
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(move |_request: &wiremock::Request| {
            provider_request_count_for_responder.fetch_add(1, Ordering::SeqCst);
            let allowed = store
                .all_audit_event_jsons()
                .unwrap()
                .into_iter()
                .filter_map(|raw| serde_json::from_str::<Value>(&raw).ok())
                .any(|event| {
                    event["kind"] == "action.gated"
                        && event["action"] == "artifact.propose"
                        && event["decision"]["outcome"] == "allow"
                });
            if !allowed {
                all_requests_after_allow_for_responder.store(false, Ordering::SeqCst);
            }
            ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "READY OWNER SAFE"}]
            }))
        })
        .mount(&provider_server)
        .await;
    state.provider_pool.insert(
        provider.id.clone(),
        ProviderClient::from_config(&provider, "new-key".into()),
    );
    state.provider_config_digests.insert(
        provider.id.clone(),
        crate::config::provider_config_digest(&provider),
    );
    let grant = handle_owner_update(&state, &owner_update("propose swap"))
        .await
        .unwrap()
        .unwrap();
    let (addr, handle) = start_server(state).await;
    let response = post_action(
        addr,
        &grant.task_token,
        "artifact.propose",
        Some(json!({
            "kind": "model_swap",
            "yaml": "id: base\nversion: 1\nlifecycle_state: proposed\nrole: base\ntarget_provider_id: swapped-provider\ngolden_set_id: model_swap_default\n"
        })),
    )
    .await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "allow");
    assert!(all_requests_after_allow.load(Ordering::SeqCst));
    assert!(provider_request_count.load(Ordering::SeqCst) > 0);
    handle.abort();
}

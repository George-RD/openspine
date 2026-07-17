use super::*;
use crate::pipeline::handle_owner_update;

#[tokio::test]
async fn paired_promotion_rolls_back_on_counterpart_put_failure() {
    use crate::gmail::GmailConnector;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let token_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"access_token":"ok","expires_in":3600})),
        )
        .mount(&token_server)
        .await;
    let gmail = GmailConnector::new(
        "client-id".into(),
        String::new(),
        String::new(),
        "owner@example.com".into(),
    )
    .with_urls(format!("{}/token", token_server.uri()), token_server.uri());
    let state = crate::test_support::fixtures::test_state_with_gmail(gmail);
    let proof = crate::telegram::VerifiedOwnerContext::test_new();
    assert!(arm(
        &state,
        42,
        state.owner_principal_id,
        &proof,
        SecretMode::Intake,
        "gmail.client_secret"
    )
    .expect("arm"));
    assert_eq!(
        capture(&state, 42, "staged-secret").await.expect("capture"),
        Some(CaptureOutcome::Staged(SecretMode::Intake))
    );
    let meta_before = state
        .store
        .get_kv("secret.stage.gmail.client_secret")
        .unwrap();
    state.secrets.put("gmail.client_secret", b"old-c").unwrap();
    state.secrets.put("gmail.refresh_token", b"old-r").unwrap();
    state.secrets.arm_fault_put("gmail.client_secret");
    assert!(arm(
        &state,
        42,
        state.owner_principal_id,
        &proof,
        SecretMode::Intake,
        "gmail.refresh_token"
    )
    .expect("arm"));
    assert!(capture(&state, 42, "new-refresh").await.is_err());
    assert_eq!(
        state.secrets.get_string("gmail.client_secret").unwrap(),
        Some("old-c".into())
    );
    assert_eq!(
        state.secrets.get_string("gmail.refresh_token").unwrap(),
        Some("old-r".into())
    );
    assert_eq!(
        state
            .secrets
            .get_string("secret.staged.gmail.client_secret")
            .unwrap(),
        Some("staged-secret".into())
    );
    assert_eq!(
        state
            .store
            .get_kv("secret.stage.gmail.client_secret")
            .unwrap(),
        meta_before
    );
}

#[tokio::test]
async fn paired_promotion_rolls_back_on_staged_delete_failure() {
    use crate::gmail::GmailConnector;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let token_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"access_token":"ok","expires_in":3600})),
        )
        .mount(&token_server)
        .await;
    let gmail = GmailConnector::new(
        "client-id".into(),
        String::new(),
        String::new(),
        "owner@example.com".into(),
    )
    .with_urls(format!("{}/token", token_server.uri()), token_server.uri());
    let state = crate::test_support::fixtures::test_state_with_gmail(gmail);
    let proof = crate::telegram::VerifiedOwnerContext::test_new();
    assert!(arm(
        &state,
        42,
        state.owner_principal_id,
        &proof,
        SecretMode::Intake,
        "gmail.client_secret"
    )
    .expect("arm"));
    assert_eq!(
        capture(&state, 42, "staged-secret").await.expect("capture"),
        Some(CaptureOutcome::Staged(SecretMode::Intake))
    );
    let meta_before = state
        .store
        .get_kv("secret.stage.gmail.client_secret")
        .unwrap();
    state.secrets.put("gmail.client_secret", b"old-c").unwrap();
    state.secrets.put("gmail.refresh_token", b"old-r").unwrap();
    state
        .secrets
        .arm_fault_delete("secret.staged.gmail.client_secret");
    assert!(arm(
        &state,
        42,
        state.owner_principal_id,
        &proof,
        SecretMode::Intake,
        "gmail.refresh_token"
    )
    .expect("arm"));
    assert!(capture(&state, 42, "new-refresh").await.is_err());
    assert_eq!(
        state.secrets.get_string("gmail.client_secret").unwrap(),
        Some("old-c".into())
    );
    assert_eq!(
        state.secrets.get_string("gmail.refresh_token").unwrap(),
        Some("old-r".into())
    );
    assert_eq!(
        state
            .secrets
            .get_string("secret.staged.gmail.client_secret")
            .unwrap(),
        Some("staged-secret".into())
    );
    assert_eq!(
        state
            .store
            .get_kv("secret.stage.gmail.client_secret")
            .unwrap(),
        meta_before
    );
}

#[tokio::test]
async fn captured_secret_never_reaches_model_gateway_or_shell_payload() {
    use crate::config::{ProviderAuth, ProviderConfig, ProviderKind};
    use crate::model_gateway::ProviderClient;
    use crate::test_support::fixtures::owner_update;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let sentinel = "super-secret-intake-value";
    let provider = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"content":[{"type":"text","text":"ok"}]})),
        )
        .expect(0)
        .mount(&provider)
        .await;
    let mut state = crate::test_support::fixtures::test_state();
    state.provider_pool.insert(
        "test-provider".to_string(),
        ProviderClient::from_config(
            &ProviderConfig {
                id: "test-provider".into(),
                kind: ProviderKind::Anthropic,
                base_url: Some(provider.uri()),
                model: "test-model".into(),
                auth: ProviderAuth::ApiKey {
                    env: "UNUSED".into(),
                },
            },
            "test-key".into(),
        ),
    );
    let arm_update = owner_update("/secret intake gmail.refresh");
    assert!(handle_owner_update(&state, &arm_update)
        .await
        .unwrap()
        .is_none());
    let secret_update = owner_update(sentinel);
    assert!(handle_owner_update(&state, &secret_update)
        .await
        .unwrap()
        .is_none());
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
    for (key, value) in state.store.all_kv_for_test() {
        assert!(
            !value.contains(sentinel),
            "secret leaked into kv_state key {key}: {value}"
        );
    }
    for event in state.store.all_audit_event_jsons().expect("audit rows") {
        assert!(
            !event.contains(sentinel),
            "secret leaked into audit: {event}"
        );
    }
    for request in provider.received_requests().await.unwrap() {
        assert!(
            !String::from_utf8_lossy(&request.body).contains(sentinel),
            "secret leaked into model-gateway payload: {:?}",
            request.body
        );
    }
}

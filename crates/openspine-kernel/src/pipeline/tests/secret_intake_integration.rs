use super::bot_identity_support::*;
use crate::config::{ProviderAuth, ProviderConfig, ProviderKind};
use crate::model_gateway::ProviderClient;
use crate::pipeline::handle_owner_update;
use crate::test_support::fixtures::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn provider_pointed_at(provider: &MockServer) -> ProviderClient {
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
    )
}

async fn mount_unused_provider(provider: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "content": [{"type": "text", "text": "ok"}]
        })))
        .expect(0)
        .mount(provider)
        .await;
}

#[tokio::test]
async fn production_intake_rotate_connector_call_integration() {
    let tg = MockServer::start().await;
    let provider = MockServer::start().await;
    mount_unused_provider(&provider).await;

    let old_token = "old-token-777-XYZ";
    let intake_token = "intake-token-777-XYZ";
    let rotated_token = "rotated-token-777-XYZ";
    let bot_id = 777;

    for token in &[old_token, intake_token, rotated_token] {
        Mock::given(method("POST"))
            .and(path(format!("/bot{token}/GetMe")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": {
                    "id": bot_id,
                    "is_bot": true,
                    "first_name": "test",
                    "can_join_groups": true,
                    "can_read_all_group_messages": true,
                    "supports_inline_queries": false,
                    "has_main_web_app": false
                }
            })))
            .mount(&tg)
            .await;

        Mock::given(method("POST"))
            .and(path(format!("/bot{token}/SendMessage")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": {
                    "message_id": 1,
                    "date": 123456,
                    "chat": {
                        "id": 555,
                        "type": "private"
                    }
                }
            })))
            .mount(&tg)
            .await;
    }

    let mut state = telegram_state_with_token(&tg, old_token);
    state
        .provider_pool
        .insert("test-provider".to_string(), provider_pointed_at(&provider));
    state
        .store
        .set_kv("telegram.bot_id", &bot_id.to_string())
        .unwrap();

    // 1. Arm intake
    let outcome1 = handle_owner_update(&state, &owner_update("/secret intake telegram.bot_token"))
        .await
        .unwrap();
    assert_eq!(outcome1, None);

    // 2. Capture intake
    let outcome2 = handle_owner_update(&state, &owner_update(intake_token))
        .await
        .unwrap();
    assert_eq!(outcome2, None);

    // Verify intake stored
    assert_eq!(
        state.secrets.get_string("telegram.bot_token").unwrap(),
        Some(intake_token.to_string())
    );

    // 3. Arm rotate
    let outcome3 = handle_owner_update(&state, &owner_update("/secret rotate telegram.bot_token"))
        .await
        .unwrap();
    assert_eq!(outcome3, None);

    // 4. Capture rotate
    let outcome4 = handle_owner_update(&state, &owner_update(rotated_token))
        .await
        .unwrap();
    assert_eq!(outcome4, None);

    // Verify rotated token stored
    assert_eq!(
        state.secrets.get_string("telegram.bot_token").unwrap(),
        Some(rotated_token.to_string())
    );

    // 5. Connector-call using rotated token
    state
        .connectors
        .telegram()
        .send_reply(555, "connector-call-message")
        .await
        .unwrap();

    // Verify SendMessage calls received by tg mock server
    let requests = tg.received_requests().await.unwrap();
    let send_message_requests: Vec<_> = requests
        .into_iter()
        .filter(|r| r.url.path().ends_with("/SendMessage"))
        .collect();

    assert_eq!(send_message_requests.len(), 5);

    // Request 0 (intake arm): path "/bot{old_token}/SendMessage"
    assert!(send_message_requests[0].url.path().contains(old_token));
    let body0: serde_json::Value = serde_json::from_slice(&send_message_requests[0].body).unwrap();
    assert_eq!(
        body0,
        serde_json::json!({
            "chat_id": 555,
            "text": "Secret mode armed; send the value in your next private message."
        })
    );

    // Request 1 (intake stored): path "/bot{intake_token}/SendMessage"
    assert!(send_message_requests[1].url.path().contains(intake_token));
    let body1: serde_json::Value = serde_json::from_slice(&send_message_requests[1].body).unwrap();
    assert_eq!(
        body1,
        serde_json::json!({
            "chat_id": 555,
            "text": "Secret intake completed; value was stored."
        })
    );

    // Request 2 (rotate arm): path "/bot{intake_token}/SendMessage"
    assert!(send_message_requests[2].url.path().contains(intake_token));
    let body2: serde_json::Value = serde_json::from_slice(&send_message_requests[2].body).unwrap();
    assert_eq!(
        body2,
        serde_json::json!({
            "chat_id": 555,
            "text": "Secret mode armed; send the value in your next private message."
        })
    );

    // Request 3 (rotate stored): path "/bot{rotated_token}/SendMessage"
    assert!(send_message_requests[3].url.path().contains(rotated_token));
    let body3: serde_json::Value = serde_json::from_slice(&send_message_requests[3].body).unwrap();
    assert_eq!(
        body3,
        serde_json::json!({
            "chat_id": 555,
            "text": "Secret rotation completed; value was stored."
        })
    );

    // Request 4 (connector-call): path "/bot{rotated_token}/SendMessage"
    assert!(send_message_requests[4].url.path().contains(rotated_token));
    let body4: serde_json::Value = serde_json::from_slice(&send_message_requests[4].body).unwrap();
    assert_eq!(
        body4,
        serde_json::json!({
            "chat_id": 555,
            "text": "connector-call-message"
        })
    );

    // Assert secret redaction: none of the SendMessage body payloads contain the raw secret values
    for (i, req) in send_message_requests.iter().enumerate() {
        let body_str = String::from_utf8(req.body.clone()).unwrap();
        assert!(
            !body_str.contains(intake_token),
            "intake token leaked in message {i} body: {body_str}"
        );
        assert!(
            !body_str.contains(rotated_token),
            "rotated token leaked in message {i} body: {body_str}"
        );
    }

    assert_secret_never_leaks(&state, intake_token, &provider).await;
    assert_secret_never_leaks(&state, rotated_token, &provider).await;
}

#[tokio::test]
async fn mismatched_chat_pending_test() {
    let tg = MockServer::start().await;
    let provider = MockServer::start().await;
    mount_unused_provider(&provider).await;

    let old_token = "old-token-777-XYZ";
    let candidate_token = "candidate-token-777-XYZ";
    let bot_id = 777;

    mount_telegram_with_getme(&tg, old_token, candidate_token, bot_id).await;
    let mut state = telegram_state_with_token(&tg, old_token);
    state
        .provider_pool
        .insert("test-provider".to_string(), provider_pointed_at(&provider));
    state
        .store
        .set_kv("telegram.bot_id", &bot_id.to_string())
        .unwrap();

    // 1. Arm for chat 555
    let outcome1 = handle_owner_update(&state, &owner_update("/secret intake telegram.bot_token"))
        .await
        .unwrap();
    assert_eq!(outcome1, None);

    // 2. Try to capture from a mismatched chat (e.g. 999)
    let mut mismatched = owner_update(candidate_token);
    mismatched.chat_id = 999;

    let outcome2 = handle_owner_update(&state, &mismatched).await.unwrap();
    assert_eq!(outcome2, None);

    // Verify it was rejected (token not updated)
    assert_eq!(
        state.secrets.get_string("telegram.bot_token").unwrap(),
        Some(old_token.to_string())
    );

    // Verify pending key was deleted
    assert!(state
        .store
        .get_kv("secret.intake.pending")
        .unwrap()
        .is_none());

    // Verify a SendMessage was sent to chat 999 with rejection message
    let requests = tg.received_requests().await.unwrap();
    let send_message_requests: Vec<_> = requests
        .into_iter()
        .filter(|r| r.url.path().ends_with("/SendMessage"))
        .collect();

    // We expect 2 SendMessage requests: 1 to chat 555 (armed), 1 to chat 999 (rejected)
    assert_eq!(send_message_requests.len(), 2);

    let body0: serde_json::Value = serde_json::from_slice(&send_message_requests[0].body).unwrap();
    assert_eq!(
        body0,
        serde_json::json!({
            "chat_id": 555,
            "text": "Secret mode armed; send the value in your next private message."
        })
    );

    let body1: serde_json::Value = serde_json::from_slice(&send_message_requests[1].body).unwrap();
    assert_eq!(
        body1,
        serde_json::json!({
            "chat_id": 999,
            "text": "Secret message discarded; intake expired, failed validation, or was not bound to this chat. Retry."
        })
    );

    // Assert secret redaction
    for (i, req) in send_message_requests.iter().enumerate() {
        let body_str = String::from_utf8(req.body.clone()).unwrap();
        assert!(
            !body_str.contains(candidate_token),
            "candidate token leaked in message {i} body: {body_str}"
        );
    }
}

#[tokio::test]
async fn expired_pending_test() {
    let tg = MockServer::start().await;
    let provider = MockServer::start().await;
    mount_unused_provider(&provider).await;

    let old_token = "old-token-777-XYZ";
    let candidate_token = "candidate-token-777-XYZ";
    let bot_id = 777;

    mount_telegram_with_getme(&tg, old_token, candidate_token, bot_id).await;
    let mut state = telegram_state_with_token(&tg, old_token);
    state
        .provider_pool
        .insert("test-provider".to_string(), provider_pointed_at(&provider));
    state
        .store
        .set_kv("telegram.bot_id", &bot_id.to_string())
        .unwrap();

    // 1. Arm
    let outcome1 = handle_owner_update(&state, &owner_update("/secret intake telegram.bot_token"))
        .await
        .unwrap();
    assert_eq!(outcome1, None);

    // 2. Manipulate the pending state to set it as expired
    let raw_pending = state
        .store
        .get_kv("secret.intake.pending")
        .unwrap()
        .unwrap();
    let mut pending_val: serde_json::Value = serde_json::from_str(&raw_pending).unwrap();
    pending_val["expires_at"] = serde_json::json!("2020-01-01T00:00:00Z");
    let expired_pending = serde_json::to_string(&pending_val).unwrap();
    state
        .store
        .set_kv("secret.intake.pending", &expired_pending)
        .unwrap();

    // 3. Send token
    let outcome2 = handle_owner_update(&state, &owner_update(candidate_token))
        .await
        .unwrap();
    assert_eq!(outcome2, None);

    // Verify it was rejected (token not updated)
    assert_eq!(
        state.secrets.get_string("telegram.bot_token").unwrap(),
        Some(old_token.to_string())
    );

    // Verify pending key was deleted
    assert!(state
        .store
        .get_kv("secret.intake.pending")
        .unwrap()
        .is_none());

    // Verify a SendMessage was sent to chat 555 with rejection message
    let requests = tg.received_requests().await.unwrap();
    let send_message_requests: Vec<_> = requests
        .into_iter()
        .filter(|r| r.url.path().ends_with("/SendMessage"))
        .collect();

    // We expect 2 SendMessage requests: 1 armed, 1 rejected
    assert_eq!(send_message_requests.len(), 2);

    let body0: serde_json::Value = serde_json::from_slice(&send_message_requests[0].body).unwrap();
    assert_eq!(
        body0,
        serde_json::json!({
            "chat_id": 555,
            "text": "Secret mode armed; send the value in your next private message."
        })
    );

    let body1: serde_json::Value = serde_json::from_slice(&send_message_requests[1].body).unwrap();
    assert_eq!(
        body1,
        serde_json::json!({
            "chat_id": 555,
            "text": "Secret message discarded; intake expired, failed validation, or was not bound to this chat. Retry."
        })
    );

    // Assert secret redaction
    for (i, req) in send_message_requests.iter().enumerate() {
        let body_str = String::from_utf8(req.body.clone()).unwrap();
        assert!(
            !body_str.contains(candidate_token),
            "candidate token leaked in message {i} body: {body_str}"
        );
    }
}

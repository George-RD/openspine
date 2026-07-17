//! Token capture/rotation boundary tests. Drive the real production capture
//! path (`handle_owner_update` → `secret_intake::arm` → `capture`, with mocked
//! `getMe` validating the candidate) and the real post-promotion `poll_once`
//! over the vault-backed connector, proving the consumed offset is preserved
//! for a same-bot rotation and a fresh namespace starts low for a different bot.

use super::bot_identity_support::*;
use crate::config::{ProviderAuth, ProviderConfig, ProviderKind};
use crate::model_gateway::ProviderClient;
use crate::pipeline::handle_owner_update;
use crate::secret_intake::{arm, SecretMode};
use crate::telegram::{VerifiedOwnerContext, BOT_TOKEN_SLOT};
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
async fn same_bot_token_rotation_preserves_namespaced_offset() {
    let tg = MockServer::start().await;
    let provider = MockServer::start().await;
    mount_unused_provider(&provider).await;
    let old_token = "old-token-777-XYZ";
    let new_token = "new-token-777-XYZ";
    mount_telegram_with_getme(&tg, old_token, new_token, 777).await;
    let mut state = telegram_state_with_token(&tg, old_token);
    state
        .provider_pool
        .insert("test-provider".to_string(), provider_pointed_at(&provider));
    // Already-initialized bot 777 with a consumed namespaced offset.
    state.store.set_kv("telegram.bot_id", "777").unwrap();
    state
        .store
        .set_kv("last_telegram_update_id.777", "100")
        .unwrap();

    let proof = VerifiedOwnerContext::test_new();
    assert!(arm(
        &state,
        555,
        state.owner_principal_id,
        &proof,
        SecretMode::Rotate,
        BOT_TOKEN_SLOT,
    )
    .expect("arm"));
    // Production capture path validates the candidate via mocked getMe.
    let outcome = handle_owner_update(&state, &owner_update(new_token))
        .await
        .unwrap();
    assert_eq!(outcome, None);

    // Same bot id: token promoted, namespaced offset preserved (not reset).
    assert_eq!(
        state.store.get_kv("telegram.bot_id").unwrap(),
        Some("777".into())
    );
    assert_eq!(
        state.secrets.get_string(BOT_TOKEN_SLOT).unwrap(),
        Some(new_token.into())
    );
    assert_eq!(
        state.store.get_kv("last_telegram_update_id.777").unwrap(),
        Some("100".into())
    );

    mount_getupdates(
        &tg,
        new_token,
        &[(100, "consumed-rotate-update"), (101, "hello lyra")],
    )
    .await;
    // The consumed update (id 100) never reaches the pipeline/model.
    crate::pipeline::poll_telegram_once_for_test(&state)
        .await
        .expect("poll must succeed");
    assert_poll_offset(&tg, Some(101)).await;
    assert_eq!(state.store.count_task_grants().unwrap(), 1);
    assert_secret_never_leaks(&state, new_token, &provider).await;
    assert_secret_never_leaks(&state, old_token, &provider).await;
}

#[tokio::test]
async fn different_bot_token_rotation_starts_fresh_namespace() {
    let tg = MockServer::start().await;
    let provider = MockServer::start().await;
    mount_unused_provider(&provider).await;
    let old_token = "old-token-777-XYZ";
    let new_token = "new-token-888-XYZ";
    mount_telegram_with_getme(&tg, old_token, new_token, 888).await;
    let mut state = telegram_state_with_token(&tg, old_token);
    state
        .provider_pool
        .insert("test-provider".to_string(), provider_pointed_at(&provider));
    // Old bot 777 has a consumed namespaced offset; must NOT be copied.
    state.store.set_kv("telegram.bot_id", "777").unwrap();
    state
        .store
        .set_kv("last_telegram_update_id.777", "100")
        .unwrap();

    let proof = VerifiedOwnerContext::test_new();
    assert!(arm(
        &state,
        555,
        state.owner_principal_id,
        &proof,
        SecretMode::Rotate,
        BOT_TOKEN_SLOT,
    )
    .expect("arm"));
    let outcome = handle_owner_update(&state, &owner_update(new_token))
        .await
        .unwrap();
    assert_eq!(outcome, None);

    // Different bot id: fresh namespace, old offset never copied.
    assert_eq!(
        state.store.get_kv("telegram.bot_id").unwrap(),
        Some("888".into())
    );
    assert_eq!(
        state.secrets.get_string(BOT_TOKEN_SLOT).unwrap(),
        Some(new_token.into())
    );
    assert_eq!(
        state.store.get_kv("last_telegram_update_id.888").unwrap(),
        None
    );
    assert_eq!(
        state.store.get_kv("last_telegram_update_id.777").unwrap(),
        Some("100".into())
    );
    // Fresh namespace (no offset): a low owner-control update is consumed
    // (dispatched) rather than dropped; polling omits the offset query.
    mount_getupdates(&tg, new_token, &[(50, "hello lyra")]).await;

    crate::pipeline::poll_telegram_once_for_test(&state)
        .await
        .expect("poll must succeed");
    assert_poll_offset(&tg, None).await;
    assert_eq!(state.store.count_task_grants().unwrap(), 1);
    assert_eq!(
        state.store.get_kv("last_telegram_update_id.888").unwrap(),
        Some("50".into())
    );
    assert_secret_never_leaks(&state, new_token, &provider).await;
    assert_secret_never_leaks(&state, old_token, &provider).await;
}

//! Bot-identity startup boundary tests: first-boot legacy migration, the
//! one-shot init-transaction rollback, cross-backend crash reconciliation
//! (vault on bot B, SQLite on bot A), same-id preservation, and transient
//! getMe retry. All drive REAL production functions (`initialize_telegram_bot_id`,
//! `initialize_telegram_bot_id_until_ready`, real `poll_once` over mocked wire),
//! not resolver helpers.

use super::bot_identity_support::*;
use crate::config::{ProviderAuth, ProviderConfig, ProviderKind};
use crate::model_gateway::ProviderClient;
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
async fn startup_migrates_old_token_and_legacy_offset_before_first_poll() {
    let tg = MockServer::start().await;
    let provider = MockServer::start().await;
    mount_unused_provider(&provider).await;
    let old_token = "old-token-ABCDEFG";
    mount_telegram_with_getme(&tg, old_token, old_token, 777).await;
    let mut state = telegram_state_with_token(&tg, old_token);
    state
        .provider_pool
        .insert("test-provider".to_string(), provider_pointed_at(&provider));
    // Pre-existing legacy offset, no bot_id yet.
    state
        .store
        .set_kv("last_telegram_update_id", "100")
        .unwrap();

    // Production startup boundary: initialize runs before the first poll.
    crate::pipeline::initialize_telegram_bot_id(&state)
        .await
        .expect("startup init must succeed");

    // Legacy offset migrated atomically into the bot-id namespace.
    assert_eq!(
        state.store.get_kv("telegram.bot_id").unwrap(),
        Some("777".into())
    );
    assert_eq!(state.store.get_kv("last_telegram_update_id").unwrap(), None);
    assert_eq!(
        state.store.get_kv("last_telegram_update_id.777").unwrap(),
        Some("100".into())
    );

    // First poll drives the real wire: Telegram offset = migrated 100 + 1.
    mount_getupdates(
        &tg,
        old_token,
        &[(100, "consumed-legacy-update"), (101, "hello lyra")],
    )
    .await;
    crate::pipeline::poll_telegram_once_for_test(&state)
        .await
        .expect("first poll must succeed");
    assert_poll_offset(&tg, Some(101)).await;

    // The consumed legacy update (id 100) never reached the pipeline; only
    // the fresh update (101) was dispatched.
    assert_eq!(state.store.count_task_grants().unwrap(), 1);
    assert_eq!(
        state.store.get_kv("last_telegram_update_id.777").unwrap(),
        Some("101".into())
    );
    assert_secret_never_leaks(&state, old_token, &provider).await;
}

#[tokio::test]
async fn initialization_transaction_failure_rolls_back_then_retry_succeeds() {
    let tg = MockServer::start().await;
    let old_token = "old-token-ROLLBACK";
    mount_telegram_with_getme(&tg, old_token, old_token, 777).await;
    let state = telegram_state_with_token(&tg, old_token);
    // Legacy offset present; bot_id absent (so init runs).
    state
        .store
        .set_kv("last_telegram_update_id", "100")
        .unwrap();

    // Inject a one-shot initialization-transaction failure.
    state.store.arm_fault_init_tx_for_test();

    let first = crate::pipeline::initialize_telegram_bot_id(&state).await;
    assert!(first.is_err(), "initialization must fail on tx fault");

    // Both the bot_id write and the legacy migration rolled back: bot_id is
    // still absent and the legacy offset is intact (not deleted/copied).
    assert_eq!(state.store.get_kv("telegram.bot_id").unwrap(), None);
    assert_eq!(
        state.store.get_kv("last_telegram_update_id").unwrap(),
        Some("100".into())
    );
    assert_eq!(
        state.store.get_kv("last_telegram_update_id.777").unwrap(),
        None
    );

    // Retry (fault consumed) succeeds and completes the migration.
    crate::pipeline::initialize_telegram_bot_id(&state)
        .await
        .expect("retry must succeed");
    assert_eq!(
        state.store.get_kv("telegram.bot_id").unwrap(),
        Some("777".into())
    );
    assert_eq!(state.store.get_kv("last_telegram_update_id").unwrap(), None);
    assert_eq!(
        state.store.get_kv("last_telegram_update_id.777").unwrap(),
        Some("100".into())
    );
}

#[tokio::test]
async fn startup_reconciles_vault_token_when_persisted_bot_id_differs() {
    // Crash-state recovery: the vault already holds bot B's token but SQLite
    // still records bot A (process death between a token `put` and the
    // `telegram.bot_id` update). Startup must getMe the vault token, detect the
    // mismatch, and switch to B's FRESH namespace without inheriting A's (or a
    // stale prior B) consumed offset.
    let tg = MockServer::start().await;
    let b_token = "vault-token-bot-888";
    mount_telegram_with_getme(&tg, b_token, b_token, 888).await;
    let state = telegram_state_with_token(&tg, b_token);
    // SQLite still on bot A; A and a stale prior-B offset both present.
    state.store.set_kv("telegram.bot_id", "777").unwrap();
    state
        .store
        .set_kv("last_telegram_update_id.777", "100")
        .unwrap();
    state
        .store
        .set_kv("last_telegram_update_id.888", "500")
        .unwrap();

    crate::pipeline::initialize_telegram_bot_id(&state)
        .await
        .expect("startup reconciliation must succeed");

    // Persisted identity switched to the actual bot B.
    assert_eq!(
        state.store.get_kv("telegram.bot_id").unwrap(),
        Some("888".into())
    );
    // B starts fresh/low: neither A's offset nor a stale prior-B offset is
    // inherited.
    assert_eq!(
        state.store.get_kv("last_telegram_update_id.888").unwrap(),
        None
    );
    // A's namespace is untouched (not deleted; simply unused).
    assert_eq!(
        state.store.get_kv("last_telegram_update_id.777").unwrap(),
        Some("100".into())
    );
    assert_eq!(state.store.get_kv("last_telegram_update_id").unwrap(), None);

    // Prove B actually polls fresh/low: mount a low owner update under B and
    // drive the real poll helper. A fresh namespace omits the offset, the low
    // update is consumed (not dropped as already-processed), and it persists
    // under B's `.888` namespace — never under A's `.777`.
    mount_getupdates(&tg, b_token, &[(50, "hello lyra")]).await;
    crate::pipeline::poll_telegram_once_for_test(&state)
        .await
        .expect("post-reconciliation poll must succeed");
    assert_poll_offset(&tg, None).await;
    assert_eq!(state.store.count_task_grants().unwrap(), 1);
    assert_eq!(
        state.store.get_kv("last_telegram_update_id.888").unwrap(),
        Some("50".into())
    );
    assert_eq!(
        state.store.get_kv("last_telegram_update_id.777").unwrap(),
        Some("100".into())
    );
}

#[tokio::test]
async fn startup_preserves_offset_when_vault_token_matches_persisted() {
    // Happy reconcile: vault token is still bot A and SQLite agrees — the
    // consumed offset namespace is preserved, no fresh namespace is created.
    let tg = MockServer::start().await;
    let a_token = "vault-token-bot-777";
    mount_telegram_with_getme(&tg, a_token, a_token, 777).await;
    let state = telegram_state_with_token(&tg, a_token);
    state.store.set_kv("telegram.bot_id", "777").unwrap();
    state
        .store
        .set_kv("last_telegram_update_id.777", "100")
        .unwrap();

    crate::pipeline::initialize_telegram_bot_id(&state)
        .await
        .expect("startup reconciliation must succeed");

    assert_eq!(
        state.store.get_kv("telegram.bot_id").unwrap(),
        Some("777".into())
    );
    assert_eq!(
        state.store.get_kv("last_telegram_update_id.777").unwrap(),
        Some("100".into())
    );
}

#[tokio::test]
async fn startup_retries_getme_on_transient_failure() {
    // A transient getMe failure on first upgraded startup must not terminate
    // the poll loop (and thus the kernel). The retry helper re-attempts under
    // backoff; here backoff is ZERO so the test stays fast.
    let tg = MockServer::start().await;
    let token = "vault-token-bot-777";
    // One-shot failure (mounted first so it takes precedence for the first
    // request, then is exhausted). A malformed 200 body avoids teloxide's
    // 10-second backoff on 5xx server errors while still failing validation.
    Mock::given(method("POST"))
        .and(path(format!("/bot{token}/GetMe")))
        .respond_with(ResponseTemplate::new(200).set_body_string("not-valid-json"))
        .up_to_n_times(1)
        .mount(&tg)
        .await;
    // Success responder (mounted second; serves after the one-shot is exhausted).
    Mock::given(method("POST"))
        .and(path(format!("/bot{token}/GetMe")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "result": {
                "id": 777, "is_bot": true, "first_name": "t",
                "can_join_groups": true, "can_read_all_group_messages": true,
                "supports_inline_queries": false, "has_main_web_app": false
            }
        })))
        .mount(&tg)
        .await;
    let state = telegram_state_with_token(&tg, token);
    state
        .store
        .set_kv("last_telegram_update_id", "100")
        .unwrap();

    crate::pipeline::initialize_telegram_bot_id_until_ready(&state, std::time::Duration::ZERO)
        .await;

    // Exactly two getMe calls (one failed, one succeeded) and the migration
    // completed before any getUpdates poll.
    let getme_calls = tg
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().ends_with("/GetMe"))
        .count();
    assert_eq!(getme_calls, 2);
    assert_eq!(
        state.store.get_kv("telegram.bot_id").unwrap(),
        Some("777".into())
    );
    assert_eq!(state.store.get_kv("last_telegram_update_id").unwrap(), None);
    assert_eq!(
        state.store.get_kv("last_telegram_update_id.777").unwrap(),
        Some("100".into())
    );
}

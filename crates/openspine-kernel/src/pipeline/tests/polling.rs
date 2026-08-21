//! Poll-loop error-branch coverage. `run_telegram_poll_loop` delegates each
//! iteration to `poll_telegram_iteration` -> `poll_telegram_once`; these tests
//! drive that exact production loop body directly, proving a connector failure
//! is surfaced to the owner digest and backed off (never fatal), then recovers
//! on the next iteration.

use super::bot_identity_support::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn poll_iteration_surfaces_connector_failure_then_recovers() {
    let tg = MockServer::start().await;
    let token = "poll-error-token-777";
    mount_telegram_with_getme(&tg, token, token, 777).await;
    let state = telegram_state_with_token(&tg, token);

    // Establish the bot-id namespace before polling (fresh namespace: no
    // legacy offset, so `.777` starts absent).
    crate::pipeline::offset::initialize_telegram_bot_id(&state)
        .await
        .expect("identity init must succeed");

    // First getUpdates fails: a malformed 200 body (not 5xx) so teloxide does
    // not apply its own multi-second server-error backoff. Exhausted after one
    // call (mounted first => takes precedence), then the success responder
    // below serves the valid batch on the next iteration.
    Mock::given(method("POST"))
        .and(path(format!("/bot{token}/GetUpdates")))
        .respond_with(ResponseTemplate::new(200).set_body_string("not-valid-json"))
        .up_to_n_times(1)
        .mount(&tg)
        .await;
    mount_getupdates(&tg, token, &[(50, "hello lyra")]).await;

    // Iteration 1: poll_once errors -> the connector failure is surfaced to the
    // owner digest and the branch backs off (ZERO here). Never propagates.
    crate::pipeline::polling::poll_telegram_iteration(&state, std::time::Duration::ZERO)
        .await
        .expect("a handled poll failure must not propagate");
    assert_eq!(
        state
            .store
            .owner_digest_items()
            .expect("digest items")
            .len(),
        1,
        "the connector failure must be surfaced to the owner digest"
    );
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
    assert_eq!(
        state.store.get_kv("last_telegram_update_id.777").unwrap(),
        None,
        "a failed poll must not advance the consumed offset"
    );

    // Iteration 2: the success responder serves the valid batch; the update is
    // dispatched and the offset advances.
    crate::pipeline::polling::poll_telegram_iteration(&state, std::time::Duration::ZERO)
        .await
        .expect("the recovered poll must succeed");
    assert_eq!(state.store.count_task_grants().unwrap(), 1);
    assert_eq!(
        state.store.get_kv("last_telegram_update_id.777").unwrap(),
        Some("50".into()),
        "the dispatched update must advance the consumed offset"
    );
}

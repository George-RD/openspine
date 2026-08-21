//! Fault-injection coverage for `pipeline::artifact_activation`: the overlay
//! temp file staged and fsynced *before* the durable commit (AD-070 crash
//! ordering) must not be leaked when the commit itself fails.
//!
//! The plain overlay path stages `*.tmp.<ulid>` and, unlike the model-swap
//! `*.pending` path (which `reconcile_model_swap_overlay` sweeps on restart),
//! has no separate recovery step — so a commit error that skips cleanup
//! accumulates orphaned temp files in the overlay directory without bound.
//! This drives a route activation to that exact seam via the store's
//! `fail_next_activation_tx_for_test` hook and asserts the directory is clean.

use openspine_schemas::action::ActionId;
use serde_json::json;
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::artifact_activation_tests::approve_callback_update;
use super::artifact_propose::dispatch_artifact_propose;
use super::artifact_propose_tests::route_yaml;
use super::dispatch_tests::OWNER_CHAT_ID;
use crate::pipeline::handle_owner_update;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::{owner_update, seed_owner_history, test_state_with_telegram};

#[tokio::test]
async fn activation_tx_failure_removes_staged_overlay_temp_file() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {"message_id": 1, "date": 0, "chat": {"id": OWNER_CHAT_ID, "type": "private"}, "text": "sent"}
        })))
        .mount(&server)
        .await;

    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".to_string(),
        server.uri().parse().unwrap(),
    ));
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    seed_owner_history(&state, &grant);

    let payload = json!({"kind": "route", "yaml": route_yaml("leaky_route", "proposed")});
    let result = dispatch_artifact_propose(
        &state,
        &grant,
        &ActionId::new("artifact.propose"),
        &crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID),
        Some(&payload),
    )
    .await
    .expect("a well-formed proposal must be accepted");
    let action_request_id: Ulid = result["action_request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    // The overlay temp file is created and fsynced before the activation
    // commit; inject a commit failure so activation returns on its error path.
    state.store.fail_next_activation_tx_for_test();
    let callback = handle_owner_update(&state, &approve_callback_update(action_request_id)).await;
    assert!(
        callback.is_err(),
        "the injected activation transaction failure must surface as an error"
    );

    // The route never activated, so no final overlay file exists — but the
    // pre-commit `*.tmp.<ulid>` staging file must not survive the error path.
    let routes_dir = state.overlay_dir.join("routes");
    let leaked: Vec<String> = std::fs::read_dir(&routes_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp."))
        .collect();
    assert!(
        leaked.is_empty(),
        "activation failure must not leak staged overlay temp files, found: {leaked:?}"
    );
}

//! Full propose→approve→activate flow tests (5d/5h): the digest-bound
//! approval spec.md's "Owner approves a proposal" scenario describes, and
//! the duplicate guard once an artifact is genuinely `active` (as opposed
//! to `artifact_propose_tests::artifact_propose_rejects_duplicate_id_version`'s
//! "still pending" case). Split out of `artifact_propose_tests.rs` purely
//! to keep both files under the 500-line gate, mirroring the
//! `dispatch_tests.rs` / `preview_tests.rs` split.
//!
//! Both tests drive `dispatch_artifact_propose` (the dispatch entry
//! point, `pub(super)` within `api`) and then `handle_owner_update` with a
//! synthesised "Approve" callback update (the same `VerifiedUpdate::OwnerCallback`
//! routing production traffic goes through) against the *same* `AppState`
//! — see `artifact_propose_tests`'s module doc for why these tests call
//! the dispatch function directly rather than going through the HTTP
//! router: an HTTP round trip would consume `AppState` into an `Arc`
//! neither the approval-callback step nor the final registry/overlay
//! assertions could then reach.

use serde_json::{json, Value};
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use openspine_schemas::artifact::Lifecycle;

use super::actions::DispatchError;
use super::artifact_propose::dispatch_artifact_propose;
use super::artifact_propose_tests::route_yaml;
use super::dispatch_tests::OWNER_CHAT_ID;
use crate::pipeline::handle_owner_update;
use crate::telegram::{CallbackQueryUpdate, TelegramConnector, TelegramUpdate};
use crate::test_support::fixtures::{owner_update, seed_owner_history, test_state_with_telegram};

/// A verified owner tap on the "Approve" button for `action_request_id` —
/// same shape as `pipeline::tests::approval`'s private helper of the same
/// name, redefined here since that one isn't reachable from `api`.
pub(super) fn approve_callback_update(action_request_id: Ulid) -> TelegramUpdate {
    let mut update = owner_update("");
    update.text = None;
    update.callback_query = Some(CallbackQueryUpdate {
        id: "cb-1".to_string(),
        data: Some(format!("approve_draft:{action_request_id}")),
    });
    update
}

/// Mount an unconditional-success `SendMessage` mock with no call-count
/// assertion. Both tests below trigger two real Telegram sends (the
/// approval button, then the post-activation "now active" notification) —
/// neither send's wire shape is what these tests are about;
/// `artifact_propose_tests::artifact_propose_persists_and_sends_approval_button`
/// already covers the approval button's exact shape.
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
    Mock::given(method("POST"))
        .and(path("/bottest-token/AnswerCallbackQuery"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(server)
        .await;
}

#[tokio::test]
async fn approved_artifact_activates_into_registry_and_overlay() {
    let server = MockServer::start().await;
    mount_send_message_ok(&server).await;

    let state = test_state_with_telegram(telegram_stub(&server));
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    seed_owner_history(&state, &grant);

    let payload = json!({"kind": "route", "yaml": route_yaml("newly_proposed_route", "proposed")});
    let result = dispatch_artifact_propose(&state, &grant, OWNER_CHAT_ID, Some(&payload))
        .await
        .expect("a well-formed proposal must be accepted");
    let action_request_id: Ulid = result["action_request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    handle_owner_update(&state, &approve_callback_update(action_request_id))
        .await
        .expect("the approval callback must run cleanly");

    {
        let registry = state.registry.read();
        let activated = registry
            .routes
            .iter()
            .find(|r| r.id == "newly_proposed_route" && r.version == 1)
            .expect(
                "the approved route must be inserted into the live registry, participating \
                 in composition exactly like a fixture-loaded route",
            );
        assert_eq!(activated.lifecycle_state, Lifecycle::Active);
    }

    let overlay_path = state
        .overlay_dir
        .join("routes")
        .join("newly_proposed_route-v1.yaml");
    let overlay_text = std::fs::read_to_string(&overlay_path)
        .expect("an activated artifact must be persisted to the on-disk overlay");
    assert!(overlay_text.contains("lifecycle_state: active"));
    assert!(!overlay_path.with_extension("pending").exists());

    let row = state
        .store
        .find_proposed_artifact_by_action_request(action_request_id)
        .unwrap()
        .expect("the proposed_artifacts row must still exist after activation");
    assert_eq!(row.state, Lifecycle::Active);

    // D-055.1: Path 3 is gate-mediated (preceding gate() Allow) and audited
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("artifact.activated")
            .unwrap(),
        1,
        "Expected exactly one artifact.activated audit event"
    );
}

#[tokio::test]
async fn activation_with_mutated_payload_is_denied() {
    // Name is task-mandated (tasks.md §7). What it actually proves is the
    // "no duplicate after activation" half of spec.md's digest-binding
    // requirement ("A duplicate proposal for an already-active id and
    // version is rejected") — the digest-*mismatch* denial itself is
    // `openspine_gate::gate`'s `approved_but_payload_changed_since_is_denied_not_reasked`,
    // exercised once at the shared `gate()` level and deliberately not
    // re-derived per action here.
    let server = MockServer::start().await;
    mount_send_message_ok(&server).await;

    let state = test_state_with_telegram(telegram_stub(&server));
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    seed_owner_history(&state, &grant);

    let payload = json!({"kind": "route", "yaml": route_yaml("already_active_route", "proposed")});
    let result = dispatch_artifact_propose(&state, &grant, OWNER_CHAT_ID, Some(&payload))
        .await
        .expect("a well-formed proposal must be accepted");
    let action_request_id: Ulid = result["action_request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    handle_owner_update(&state, &approve_callback_update(action_request_id))
        .await
        .expect("the approval callback must run cleanly");

    // Sanity: the artifact really is active before re-proposing it.
    assert!(state
        .registry
        .read()
        .routes
        .iter()
        .any(|r| r.id == "already_active_route" && r.version == 1));

    let second_payload =
        json!({"kind": "route", "yaml": route_yaml("already_active_route", "proposed")});
    let err = dispatch_artifact_propose(&state, &grant, OWNER_CHAT_ID, Some(&second_payload))
        .await
        .unwrap_err();
    match err {
        DispatchError::BadRequest(msg) => {
            assert!(msg.contains("already exists"), "unexpected message: {msg}")
        }
        DispatchError::Resource(_) | DispatchError::Connector(_) => panic!(
            "a re-proposal of an already-active id/version must be a BadRequest, not Internal \
             — an Internal result here would mean the duplicate guard was bypassed and the \
             attempt instead hit the store's UNIQUE constraint"
        ),
    }
}

#[tokio::test]
async fn model_swap_ceremony_switches_real_generate_provider() {
    use std::sync::Arc;

    use crate::config::{ProviderAuth, ProviderConfig, ProviderKind};
    use crate::model_gateway::ProviderClient;
    use axum::serve;

    let telegram_server = MockServer::start().await;
    mount_send_message_ok(&telegram_server).await;
    let old_provider_server = MockServer::start().await;
    let new_provider_server = MockServer::start().await;
    let anthropic_response = |text: &str| {
        ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": text}],
        }))
    };
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(anthropic_response("OLD PROVIDER"))
        .mount(&old_provider_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(anthropic_response("READY OWNER SAFE"))
        .mount(&new_provider_server)
        .await;

    let mut state = test_state_with_telegram(telegram_stub(&telegram_server));
    let old_config = ProviderConfig {
        id: "test-provider".to_string(),
        kind: ProviderKind::Anthropic,
        base_url: Some(old_provider_server.uri()),
        model: "old-model".to_string(),
        auth: ProviderAuth::ApiKey {
            env: "UNUSED".to_string(),
        },
    };
    let new_config = ProviderConfig {
        id: "swapped-provider".to_string(),
        kind: ProviderKind::Anthropic,
        base_url: Some(new_provider_server.uri()),
        model: "new-model".to_string(),
        auth: ProviderAuth::ApiKey {
            env: "UNUSED".to_string(),
        },
    };
    state.provider_pool.insert(
        old_config.id.clone(),
        ProviderClient::from_config(&old_config, "old-key".to_string()),
    );
    state.provider_pool.insert(
        new_config.id.clone(),
        ProviderClient::from_config(&new_config, "new-key".to_string()),
    );
    state.provider_config_digests.insert(
        old_config.id.clone(),
        crate::config::provider_config_digest(&old_config),
    );
    state.provider_config_digests.insert(
        new_config.id.clone(),
        crate::config::provider_config_digest(&new_config),
    );

    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    let history_ref = state.artifacts.put(b"owner history").unwrap();
    state
        .store
        .append_conversation_message(grant.id, "user", &history_ref.digest)
        .unwrap();

    let forged = json!({
        "kind": "model_swap",
        "yaml": "id: base\nversion: 1\nlifecycle_state: proposed\nrole: base\ntarget_provider_id: swapped-provider\ngolden_set_id: model_swap_default\ngolden_set_result:\n  golden_set_id: model_swap_default\n  golden_set_digest: sha256:0000000000000000000000000000000000000000000000000000000000000000\n  provider_config_digest: sha256:0000000000000000000000000000000000000000000000000000000000000000\n  cases: []\n",
    });
    assert!(
        dispatch_artifact_propose(&state, &grant, OWNER_CHAT_ID, Some(&forged))
            .await
            .is_err(),
        "proposer-supplied evidence must be rejected before approval"
    );

    let valid = json!({
        "kind": "model_swap",
        "yaml": "id: base\nversion: 1\nlifecycle_state: proposed\nrole: base\ntarget_provider_id: swapped-provider\ngolden_set_id: model_swap_default\n",
    });
    let result = dispatch_artifact_propose(&state, &grant, OWNER_CHAT_ID, Some(&valid))
        .await
        .expect("kernel must enrich and propose a valid model swap");
    let action_request_id: Ulid = result["action_request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let row = state
        .store
        .find_proposed_artifact_by_action_request(action_request_id)
        .unwrap()
        .expect("enriched swap must persist a proposal");
    assert_eq!(row.state, Lifecycle::ReviewRequired);
    let payload = state
        .artifacts
        .get(&openspine_schemas::artifact::ArtifactRef {
            digest: openspine_schemas::digest::Digest::parse(row.yaml_digest).unwrap(),
            schema_version: 1,
        })
        .unwrap();
    assert!(String::from_utf8_lossy(&payload).contains("golden_set_result"));

    let gated_before = state
        .store
        .all_audit_event_jsons()
        .unwrap()
        .into_iter()
        .filter_map(|raw| serde_json::from_str::<Value>(&raw).ok())
        .filter(|event| {
            event["kind"] == "action.gated"
                && event["action"] == "artifact.activate"
                && event["decision"]["outcome"] == "allow"
        })
        .count();
    handle_owner_update(&state, &approve_callback_update(action_request_id))
        .await
        .expect("owner approval callback must activate the swap");
    assert_eq!(
        state
            .active_model_providers
            .read()
            .get(&openspine_schemas::model_swap::ModelRole::Base)
            .map(String::as_str),
        Some("swapped-provider")
    );
    let approval_gates = state
        .store
        .all_audit_event_jsons()
        .unwrap()
        .into_iter()
        .filter_map(|raw| serde_json::from_str::<Value>(&raw).ok())
        .filter(|event| {
            event["kind"] == "action.gated"
                && event["action"] == "artifact.activate"
                && event["decision"]["outcome"] == "allow"
        })
        .count();
    assert_eq!(approval_gates, gated_before + 1);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("artifact.activated")
            .unwrap(),
        1
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = crate::api::router(Arc::new(state));
    let server = tokio::spawn(async move {
        serve(listener, app).await.unwrap();
    });
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v1/model/generate"))
        .header("Authorization", format!("Bearer {}", grant.task_token))
        .json(&json!({
            "purpose": "owner_control",
            "user_message": "Which provider is active?",
            "max_tokens": 64,
        }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let raw = response.text().await.unwrap();
    assert!(
        status.is_success(),
        "generate failed: {status} {raw}; old={} new={}",
        old_provider_server.received_requests().await.unwrap().len(),
        new_provider_server.received_requests().await.unwrap().len()
    );
    let body: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(body["decision"]["outcome"], "allow");
    assert_eq!(body["text"], "READY OWNER SAFE");
    server.abort();
}

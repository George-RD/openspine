//! Dispatch-level tests for `artifact.propose` (5c/5h): schema validation,
//! the `proposed`-only lifecycle-state gate, and the id/version duplicate
//! guard against both the live registry and pending proposals.
//!
//! Unlike `dispatch_tests.rs`/`preview_tests.rs`, these call
//! `dispatch_artifact_propose` directly rather than going through the HTTP
//! router: every test here also needs to assert straight against
//! `state.store`'s `proposed_artifacts` table (and, for the full-flow
//! tests in the sibling `artifact_activation_tests` module, the live
//! registry) after the call — an HTTP round trip through `start_server`
//! consumes `AppState` into an `Arc` the caller never gets back, which
//! would make that introspection unreachable. `dispatch_artifact_propose`
//! is `pub(super)` precisely so this module (nested inside `api`, like
//! every other test module here) can call it directly.

use openspine_schemas::action::ActionId;
use serde_json::{json, Value};
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use jiff::Timestamp;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::event::DataClassification;
use openspine_schemas::grant::{GrantLimits, GrantMode, TaskGrant};
use openspine_schemas::policy::Constraints;
use openspine_schemas::reflection_miner::{
    AuditTrailEntry, CorrectionObservation, MinerBriefcase, OrdinaryMinerGrant, ReflectionMiner,
    ReflectionProposalBody, ReflectionProvenance,
};

use super::actions::DispatchError;
use super::artifact_propose::dispatch_artifact_propose;
use super::dispatch_tests::OWNER_CHAT_ID;
use crate::pipeline::handle_owner_update;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::{
    owner_update, seed_owner_history, test_state, test_state_with_telegram,
};

/// Minimal, always-schema-valid `route` proposal YAML (every field
/// `Route::deserialize` requires without a `#[serde(default)]`).
/// `pub(super)` so the sibling `artifact_activation_tests` module can
/// reuse it rather than re-deriving its own copy.
pub(super) fn route_yaml(id: &str, lifecycle_state: &str) -> String {
    format!(
        "id: {id}\n\
         schema_version: 1\n\
         lifecycle_state: {lifecycle_state}\n\
         priority: 100\n\
         agent: main_assistant_agent\n\
         workflow: owner_control_conversation\n\
         capability_pack: owner_control_basic_pack\n"
    )
}

#[tokio::test]
async fn artifact_propose_persists_and_sends_approval_button() {
    let server = MockServer::start().await;
    let token = "test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{token}/SendMessage")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": OWNER_CHAT_ID, "type": "private"},
                "text": "sent",
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let connector =
        TelegramConnector::with_api_url(token.to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    seed_owner_history(&state, &grant);

    let payload = json!({"kind": "route", "yaml": route_yaml("dark_mode_route", "proposed")});
    let result = dispatch_artifact_propose(
        &state,
        &grant,
        &ActionId::new("artifact.propose"),
        OWNER_CHAT_ID,
        Some(&payload),
    )
    .await
    .expect("a well-formed, non-duplicate proposal must be accepted");
    assert_eq!(result["proposed"], true);

    let action_request_id: Ulid = result["action_request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let row = state
        .store
        .find_proposed_artifact_by_action_request(action_request_id)
        .unwrap()
        .expect("dispatch must persist a proposed_artifacts row");
    assert_eq!(row.kind, "route");
    assert_eq!(row.artifact_id, "dark_mode_route");
    assert_eq!(row.version, 1);
    assert_eq!(row.state, Lifecycle::ReviewRequired);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let request_body: Value = requests[0].body_json().unwrap();
    assert_eq!(request_body["chat_id"], OWNER_CHAT_ID);
    let text = request_body["text"].as_str().unwrap();
    assert!(text.contains("Kind: route"));
    assert!(text.contains("Id: dark_mode_route v1"));
    assert_eq!(
        request_body["reply_markup"]["inline_keyboard"][0][0]["text"],
        "Approve"
    );
    assert_eq!(
        request_body["reply_markup"]["inline_keyboard"][0][0]["callback_data"],
        format!("approve_draft:{action_request_id}")
    );
}

#[tokio::test]
async fn artifact_propose_rejects_malformed_yaml() {
    let server = MockServer::start().await;
    let token = "test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{token}/SendMessage")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(0)
        .mount(&server)
        .await;

    let connector =
        TelegramConnector::with_api_url(token.to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");

    // Missing the required `lifecycle_state` field — fails `route`'s
    // `deny_unknown_fields` schema before anything is persisted.
    let payload = json!({
        "kind": "route",
        "yaml": "id: malformed_route_test\nschema_version: 1\npriority: 100\n\
                 agent: main_assistant_agent\nworkflow: owner_control_conversation\n\
                 capability_pack: owner_control_basic_pack\n",
    });
    let err = dispatch_artifact_propose(
        &state,
        &grant,
        &ActionId::new("artifact.propose"),
        OWNER_CHAT_ID,
        Some(&payload),
    )
    .await
    .unwrap_err();
    match err {
        DispatchError::BadRequest(msg) => {
            assert!(msg.contains("failed to parse"), "unexpected message: {msg}")
        }
        DispatchError::Resource(_)
        | DispatchError::Connector(_)
        | DispatchError::ConnectorUnavailable(_)
        | DispatchError::DeliveryUnknown(_) => {
            panic!("malformed YAML must be a BadRequest, not infrastructure failure")
        }
    }
    assert!(!state
        .store
        .proposed_artifact_exists("route", "malformed_route_test", 1)
        .unwrap());
    // The mock's `.expect(0)` above is verified on drop — any Telegram
    // send at all would fail this test outright.
}

#[tokio::test]
async fn artifact_propose_rejects_unknown_kind() {
    let state = test_state();
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");

    // Kind rejection happens before any parsing, so the YAML body itself
    // is irrelevant here.
    let payload = json!({"kind": "widget", "yaml": "irrelevant: true\n"});
    let err = dispatch_artifact_propose(
        &state,
        &grant,
        &ActionId::new("artifact.propose"),
        OWNER_CHAT_ID,
        Some(&payload),
    )
    .await
    .unwrap_err();
    match err {
        DispatchError::BadRequest(msg) => assert!(
            msg.contains("route|agent|workflow|pack|policy"),
            "unexpected message: {msg}"
        ),
        DispatchError::Resource(_)
        | DispatchError::Connector(_)
        | DispatchError::ConnectorUnavailable(_)
        | DispatchError::DeliveryUnknown(_) => {
            panic!("unknown kind must be a BadRequest, not infrastructure failure")
        }
    }
}

#[tokio::test]
async fn artifact_propose_rejects_template_kind() {
    let state = test_state();
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");

    // D-048: prompt templates remain fixture-only — a chat can never
    // propose one, even though `template` is a real artifact kind
    // elsewhere in the system. This currently shares `dispatch_artifact_propose`'s
    // kind-allowlist check with `artifact_propose_rejects_unknown_kind`,
    // but it is its own spec-traceable scenario ("A template proposal is
    // rejected") and must keep failing if that ever changes.
    let payload = json!({
        "kind": "template",
        "yaml": "id: injected_template\nschema_version: 1\n",
    });
    let err = dispatch_artifact_propose(
        &state,
        &grant,
        &ActionId::new("artifact.propose"),
        OWNER_CHAT_ID,
        Some(&payload),
    )
    .await
    .unwrap_err();
    match err {
        DispatchError::BadRequest(msg) => assert!(
            msg.contains("route|agent|workflow|pack|policy"),
            "unexpected message: {msg}"
        ),
        DispatchError::Resource(_)
        | DispatchError::Connector(_)
        | DispatchError::ConnectorUnavailable(_)
        | DispatchError::DeliveryUnknown(_) => {
            panic!("template kind must be a BadRequest, not infrastructure failure")
        }
    }
    assert!(!state
        .store
        .proposed_artifact_exists("template", "injected_template", 1)
        .unwrap());
}

#[tokio::test]
async fn artifact_propose_rejects_non_proposed_persona_kind() {
    let state = test_state();
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");

    let payload = json!({
        "kind": "persona",
        "yaml": "id: injected_persona\nschema_version: 1\nversion: 1\nlifecycle_state: active\nguidance: injected\n",
    });
    let err = dispatch_artifact_propose(
        &state,
        &grant,
        &ActionId::new("artifact.propose"),
        OWNER_CHAT_ID,
        Some(&payload),
    )
    .await
    .unwrap_err();
    match err {
        DispatchError::BadRequest(msg) => {
            assert!(msg.contains("lifecycle_state"), "unexpected message: {msg}")
        }
        DispatchError::Resource(_)
        | DispatchError::Connector(_)
        | DispatchError::ConnectorUnavailable(_)
        | DispatchError::DeliveryUnknown(_) => {
            panic!("invalid persona lifecycle must be a BadRequest")
        }
    }
    assert!(!state
        .store
        .proposed_artifact_exists("persona", "injected_persona", 1)
        .unwrap());
}

#[tokio::test]
async fn artifact_propose_rejects_duplicate_id_version() {
    let server = MockServer::start().await;
    let token = "test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{token}/SendMessage")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": OWNER_CHAT_ID, "type": "private"},
                "text": "sent",
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let connector =
        TelegramConnector::with_api_url(token.to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    seed_owner_history(&state, &grant);

    let payload = json!({"kind": "route", "yaml": route_yaml("dup_route", "proposed")});
    dispatch_artifact_propose(
        &state,
        &grant,
        &ActionId::new("artifact.propose"),
        OWNER_CHAT_ID,
        Some(&payload),
    )
    .await
    .expect("the first proposal must succeed");

    let err = dispatch_artifact_propose(
        &state,
        &grant,
        &ActionId::new("artifact.propose"),
        OWNER_CHAT_ID,
        Some(&payload),
    )
    .await
    .unwrap_err();
    match err {
        DispatchError::BadRequest(msg) => {
            assert!(msg.contains("already exists"), "unexpected message: {msg}")
        }
        DispatchError::Resource(_)
        | DispatchError::Connector(_)
        | DispatchError::ConnectorUnavailable(_)
        | DispatchError::DeliveryUnknown(_) => {
            panic!("a pending-duplicate proposal must be a BadRequest, not Internal")
        }
    }
    // Exactly one Telegram approval button was ever sent for the two
    // dispatch calls above — the mock's `.expect(1)` is verified on drop,
    // so a second send from the rejected duplicate would fail this test.
}

#[tokio::test]
async fn artifact_propose_rejects_non_proposed_lifecycle() {
    let state = test_state();
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");

    let payload = json!({"kind": "route", "yaml": route_yaml("preactivate_route", "active")});
    let err = dispatch_artifact_propose(
        &state,
        &grant,
        &ActionId::new("artifact.propose"),
        OWNER_CHAT_ID,
        Some(&payload),
    )
    .await
    .unwrap_err();
    match err {
        DispatchError::BadRequest(msg) => assert!(
            msg.contains("lifecycle_state must be proposed"),
            "unexpected message: {msg}"
        ),
        DispatchError::Resource(_)
        | DispatchError::Connector(_)
        | DispatchError::ConnectorUnavailable(_)
        | DispatchError::DeliveryUnknown(_) => {
            panic!("a pre-activation attempt must be a BadRequest, not Internal")
        }
    }
    assert!(!state
        .store
        .proposed_artifact_exists("route", "preactivate_route", 1)
        .unwrap());
}
#[path = "artifact_propose_miner_tests.rs"]
mod miner_tests;

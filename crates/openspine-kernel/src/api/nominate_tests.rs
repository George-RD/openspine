//! Tests for explicit upstream nomination opt-in (AD-071).

use jiff::Timestamp;
use openspine_schemas::artifact::{ArtifactNamespace, ArtifactRef};
use openspine_schemas::digest::digest_of_bytes;
use serde_json::json;
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::actions::DispatchError;
use super::artifact_nominate::dispatch_artifact_nominate;
use super::dispatch_tests::OWNER_CHAT_ID;
use crate::pipeline::handle_owner_update;
use crate::store::learned_artifacts::{
    CompatibilityStatus, LearnedArtifact, NominationStatus, Provenance,
};
use crate::telegram::{CallbackQueryUpdate, TelegramConnector, TelegramUpdate};
use crate::test_support::fixtures::{owner_update, test_state_with_telegram};
fn learned_route() -> LearnedArtifact {
    LearnedArtifact {
        kind: "route".into(),
        artifact_id: "nominate-route".into(),
        version: 1,
        namespace: ArtifactNamespace::Overlay,
        provenance: Provenance::ProducedBy {
            source_event_id: Ulid::new(),
            source_exchange: ArtifactRef {
                digest: digest_of_bytes(b"exchange"),
                schema_version: 1,
            },
        },
        accepted_via: None,
        learned_at: Timestamp::now(),
        compatibility: CompatibilityStatus::Compatible,
        nomination: NominationStatus::None,
        pending_reconfirmation_id: None,
        pending_yaml_digest: None,
        accepted_dependency_fingerprint: None,
        source_path: None,
        accepted_base_epoch: None,
    }
}

fn approve_callback(id: Ulid) -> TelegramUpdate {
    let mut update = owner_update("");
    update.text = None;
    update.callback_query = Some(CallbackQueryUpdate {
        id: "cb-nominate".into(),
        data: Some(format!("approve_draft:{id}")),
    });
    update
}

async fn telegram_ok(server: &MockServer) {
    Mock::given(method("POST")).and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"result":{"message_id":1,"date":0,"chat":{"id":OWNER_CHAT_ID,"type":"private"},"text":"sent"}})))
        .mount(server).await;
}

#[tokio::test]
async fn nomination_requires_explicit_depersonalized_assertion() {
    let server = MockServer::start().await;
    telegram_ok(&server).await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".into(),
        server.uri().parse().unwrap(),
    ));
    state
        .store
        .record_learned_artifact(&learned_route())
        .unwrap();
    let routes_dir = state.overlay_dir.join("routes");
    std::fs::create_dir_all(&routes_dir).unwrap();
    std::fs::write(
        routes_dir.join(crate::artifact_loader::overlay_filename(
            "nominate-route",
            1,
        )),
        b"id: nominate-route\nschema_version: 1\nlifecycle_state: active\n",
    )
    .unwrap();
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner grant composed");
    let err = dispatch_artifact_nominate(&state, &grant, OWNER_CHAT_ID, Some(&json!({"kind":"route","artifact_id":"nominate-route","version":1,"depersonalized":false}))).await.unwrap_err();
    assert!(
        matches!(err, DispatchError::BadRequest(message) if message.contains("depersonalized"))
    );
}

#[tokio::test]
async fn nomination_owner_tap_persists_nominated_status_and_audit() {
    let server = MockServer::start().await;
    telegram_ok(&server).await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".into(),
        server.uri().parse().unwrap(),
    ));
    state
        .store
        .record_learned_artifact(&learned_route())
        .unwrap();
    let routes_dir = state.overlay_dir.join("routes");
    std::fs::create_dir_all(&routes_dir).unwrap();
    std::fs::write(
        routes_dir.join(crate::artifact_loader::overlay_filename(
            "nominate-route",
            1,
        )),
        b"id: nominate-route\nschema_version: 1\nlifecycle_state: active\n",
    )
    .unwrap();
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner grant composed");
    let result = dispatch_artifact_nominate(&state, &grant, OWNER_CHAT_ID, Some(&json!({"kind":"route","artifact_id":"nominate-route","version":1,"depersonalized":true}))).await.unwrap();
    let request_id: Ulid = result["action_request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    handle_owner_update(&state, &approve_callback(request_id))
        .await
        .unwrap();
    assert_eq!(
        state.store.list_learned_artifacts().unwrap()[0].nomination,
        NominationStatus::Nominated
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("artifact.nominated")
            .unwrap(),
        1
    );
}

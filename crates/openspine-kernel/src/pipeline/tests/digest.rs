use crate::artifact_store::ArtifactStore;
use crate::failure_surfacing::{batch_failure, FailureClass};
use crate::pipeline::handle_owner_update;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::{owner_update, test_state_with_telegram};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use serde_json::{json, Value};
use std::collections::HashSet;
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TEST_GRANT_KEY: &str = "openspine-test-grant-hmac-key-v1";

async fn last_sent_text(server: &MockServer) -> String {
    let requests = server.received_requests().await.expect("requests");
    let body: Value = serde_json::from_slice(&requests.last().expect("request").body).unwrap();
    body["text"].as_str().unwrap().to_string()
}

fn telegram(token: &str, server: &MockServer) -> TelegramConnector {
    TelegramConnector::with_api_url(token.to_string(), server.uri().parse().unwrap())
}

async fn mount_send_ok(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {"message_id": 1, "date": 0, "chat": {"id": 555, "type": "private"}, "text": "sent"}
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn digest_pages_drain_only_delivered_whole_items() {
    std::env::set_var("OPENSPINE_GRANT_HMAC_KEY", TEST_GRANT_KEY);
    let telegram_server = MockServer::start().await;
    mount_send_ok(&telegram_server).await;
    let state = test_state_with_telegram(telegram("test-token", &telegram_server));
    let mut all_ids = HashSet::new();
    for i in 0..9 {
        let summary = format!("digest-summary-{i}-{}", "x".repeat(1_500));
        let id = state
            .store
            .insert_legacy_digest_failure("connector", &summary)
            .unwrap();
        all_ids.insert(id);
    }

    let mut delivered = HashSet::new();
    for _ in 0..10 {
        if state.store.owner_digest_items().unwrap().is_empty() {
            break;
        }
        handle_owner_update(&state, &owner_update("/digest"))
            .await
            .unwrap();
        let text = last_sent_text(&telegram_server).await;
        assert!(text.len() <= 4096);
        for id in &all_ids {
            if text.contains(&id.to_string()) {
                delivered.insert(*id);
            }
        }
    }
    assert_eq!(delivered, all_ids);
    assert!(state.store.owner_digest_items().unwrap().is_empty());
}

#[tokio::test]
async fn encrypted_detail_roundtrip_has_no_plaintext_db_or_audit_leak() {
    std::env::set_var("OPENSPINE_GRANT_HMAC_KEY", TEST_GRANT_KEY);
    let server = MockServer::start().await;
    mount_send_ok(&server).await;
    let state = test_state_with_telegram(telegram("test-token", &server));
    let secret = "SECRET_LEAK_MARKER_detail_only";
    batch_failure(&state, FailureClass::Connector, "connector failure", secret).unwrap();
    let item = state.store.owner_digest_items().unwrap().remove(0);
    assert!(!item.summary.contains(secret));
    let audits = state.store.all_audit_event_jsons().unwrap().join("\n");
    assert!(!audits.contains(secret));
    let digest = Digest::parse(item.text_ref.as_deref().unwrap()).unwrap();
    let bytes = state
        .artifacts
        .get(&ArtifactRef {
            digest,
            schema_version: 1,
        })
        .unwrap();
    assert_eq!(bytes, secret.as_bytes());

    handle_owner_update(&state, &owner_update(&format!("/digest {}", item.id)))
        .await
        .unwrap();
    let text = last_sent_text(&server).await;
    assert!(text.contains(secret));
    assert!(text.contains(&item.id.to_string()));
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("failure.digest_detail_viewed")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn digest_detail_not_found_is_truthful() {
    std::env::set_var("OPENSPINE_GRANT_HMAC_KEY", TEST_GRANT_KEY);
    let server = MockServer::start().await;
    mount_send_ok(&server).await;
    let state = test_state_with_telegram(telegram("test-token", &server));
    let id = Ulid::new();
    handle_owner_update(&state, &owner_update(&format!("/digest {id}")))
        .await
        .unwrap();
    assert!(last_sent_text(&server)
        .await
        .contains("No failure record found"));
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("failure.digest_detail_viewed")
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn digest_detail_viewed_audit_requires_delivery() {
    std::env::set_var("OPENSPINE_GRANT_HMAC_KEY", TEST_GRANT_KEY);
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let state = test_state_with_telegram(telegram("test-token", &server));
    batch_failure(
        &state,
        FailureClass::Connector,
        "connector failure",
        "private detail",
    )
    .unwrap();
    let id = state.store.owner_digest_items().unwrap()[0].id;
    handle_owner_update(&state, &owner_update(&format!("/digest {id}")))
        .await
        .unwrap();
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("failure.digest_detail_viewed")
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn digest_detail_corrupt_blob_surfaces_resource_without_leak() {
    std::env::set_var("OPENSPINE_GRANT_HMAC_KEY", TEST_GRANT_KEY);
    let server = MockServer::start().await;
    mount_send_ok(&server).await;
    let state = test_state_with_telegram(telegram("test-token", &server));
    let secret = "CORRUPT_BLOB_SECRET";
    batch_failure(&state, FailureClass::Connector, "connector failure", secret).unwrap();
    let item = state.store.owner_digest_items().unwrap().remove(0);
    let artifact_ref = ArtifactRef {
        digest: Digest::parse(item.text_ref.as_deref().unwrap()).unwrap(),
        schema_version: 1,
    };
    std::fs::write(
        state.artifacts.blob_path_for_test(&artifact_ref),
        b"corrupt",
    )
    .unwrap();
    handle_owner_update(&state, &owner_update(&format!("/digest {}", item.id)))
        .await
        .unwrap();
    let text = last_sent_text(&server).await;
    assert!(!text.contains(secret));
    assert!(state
        .store
        .owner_digest_items()
        .unwrap()
        .iter()
        .any(|i| i.class == "resource"));
}

#[tokio::test]
async fn digest_detail_missing_blob_surfaces_resource_without_leak() {
    std::env::set_var("OPENSPINE_GRANT_HMAC_KEY", TEST_GRANT_KEY);
    let server = MockServer::start().await;
    mount_send_ok(&server).await;
    let state = test_state_with_telegram(telegram("test-token", &server));
    let secret = "MISSING_BLOB_SECRET";
    batch_failure(&state, FailureClass::Connector, "connector failure", secret).unwrap();
    let item = state.store.owner_digest_items().unwrap().remove(0);
    let artifact_ref = ArtifactRef {
        digest: Digest::parse(item.text_ref.as_deref().unwrap()).unwrap(),
        schema_version: 1,
    };
    std::fs::remove_file(state.artifacts.blob_path_for_test(&artifact_ref)).unwrap();
    handle_owner_update(&state, &owner_update(&format!("/digest {}", item.id)))
        .await
        .unwrap();
    let text = last_sent_text(&server).await;
    assert!(!text.contains(secret));
    assert!(state
        .store
        .owner_digest_items()
        .unwrap()
        .iter()
        .any(|i| i.class == "resource"));
}

#[tokio::test]
async fn digest_detail_wrong_key_surfaces_resource_without_leak() {
    std::env::set_var("OPENSPINE_GRANT_HMAC_KEY", TEST_GRANT_KEY);
    let server = MockServer::start().await;
    mount_send_ok(&server).await;
    let mut state = test_state_with_telegram(telegram("test-token", &server));
    let secret = "WRONG_KEY_SECRET";
    batch_failure(&state, FailureClass::Connector, "connector failure", secret).unwrap();
    let item = state.store.owner_digest_items().unwrap().remove(0);
    let artifact_ref = ArtifactRef {
        digest: Digest::parse(item.text_ref.as_deref().unwrap()).unwrap(),
        schema_version: 1,
    };
    let root = state
        .artifacts
        .blob_path_for_test(&artifact_ref)
        .parent()
        .unwrap()
        .to_path_buf();
    state.artifacts = ArtifactStore::open(root, [9u8; 32]).unwrap();
    handle_owner_update(&state, &owner_update(&format!("/digest {}", item.id)))
        .await
        .unwrap();
    let text = last_sent_text(&server).await;
    assert!(!text.contains(secret));
    assert!(state
        .store
        .owner_digest_items()
        .unwrap()
        .iter()
        .any(|i| i.class == "resource"));
}

#[tokio::test]
async fn oversized_detail_is_byte_bounded() {
    std::env::set_var("OPENSPINE_GRANT_HMAC_KEY", TEST_GRANT_KEY);
    let server = MockServer::start().await;
    mount_send_ok(&server).await;
    let state = test_state_with_telegram(telegram("test-token", &server));
    let detail = format!("OVERSIZED_DETAIL_MARKER_{}", "y".repeat(10_000));
    batch_failure(
        &state,
        FailureClass::Connector,
        "connector failure",
        &detail,
    )
    .unwrap();
    let id = state.store.owner_digest_items().unwrap()[0].id;
    handle_owner_update(&state, &owner_update(&format!("/digest {id}")))
        .await
        .unwrap();
    let text = last_sent_text(&server).await;
    assert!(text.len() <= 4096);
    assert!(text.contains("OVERSIZED_DETAIL_MARKER"));
    assert!(!text.contains(&detail));
}

#[tokio::test]
async fn non_owner_digest_detail_is_ignored() {
    std::env::set_var("OPENSPINE_GRANT_HMAC_KEY", TEST_GRANT_KEY);
    let server = MockServer::start().await;
    mount_send_ok(&server).await;
    let state = test_state_with_telegram(telegram("test-token", &server));
    batch_failure(
        &state,
        FailureClass::Connector,
        "connector failure",
        "private detail",
    )
    .unwrap();
    let id = state.store.owner_digest_items().unwrap()[0].id;
    let mut update = owner_update(&format!("/digest {id}"));
    update.sender_user_id = Some(999);
    handle_owner_update(&state, &update).await.unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("failure.digest_detail_viewed")
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn digest_detail_retry_emits_viewed_receipt_exactly_once() {
    std::env::set_var("OPENSPINE_GRANT_HMAC_KEY", TEST_GRANT_KEY);
    // First delivery fails; the dead-letter carries the detail metadata so a
    // later retry can reconstruct the contract-specific receipt.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    let state = test_state_with_telegram(telegram("test-token", &server));
    batch_failure(
        &state,
        FailureClass::Connector,
        "connector failure",
        "private detail",
    )
    .unwrap();
    let id = state.store.owner_digest_items().unwrap()[0].id;
    handle_owner_update(&state, &owner_update(&format!("/digest {id}")))
        .await
        .unwrap();
    // No receipt yet: the send failed, so the detail delivery is dead-lettered
    // with its semantic metadata.
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("failure.digest_detail_viewed")
            .unwrap(),
        0
    );
    let dl = state.store.pending_dead_letters().unwrap();
    assert_eq!(dl.len(), 1);
    assert_eq!(dl[0].semantic_kind.as_deref(), Some("digest_detail"));
    assert_eq!(dl[0].availability_outcome.as_deref(), Some("available"));
    assert!(dl[0].detail_ref.is_some());
    // Retry succeeds: exactly one contract-specific receipt is appended.
    mount_send_ok(&server).await;
    crate::failure_surfacing::retry_worker::retry_due_notifications(&state)
        .await
        .unwrap();
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("failure.digest_detail_viewed")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner.notified")
            .unwrap(),
        1
    );
    assert!(state.store.pending_dead_letters().unwrap().is_empty());
}

#[tokio::test]
async fn digest_detail_immediate_success_outcome_audit_failure_is_delivery_unknown_and_no_retry() {
    std::env::set_var("OPENSPINE_GRANT_HMAC_KEY", TEST_GRANT_KEY);
    let server = MockServer::start().await;
    mount_send_ok(&server).await;
    let state = test_state_with_telegram(telegram("test-token", &server));
    batch_failure(
        &state,
        FailureClass::Connector,
        "connector failure",
        "private detail",
    )
    .unwrap();
    let id = state.store.owner_digest_items().unwrap()[0].id;

    // Inject database audit failure for `owner.notified`.
    state
        .store
        .install_audit_append_failure_for_kind("owner.notified")
        .unwrap();

    // Drive immediate detail command. It will send successfully via Telegram but
    // the database transaction will fail.
    let res = handle_owner_update(&state, &owner_update(&format!("/digest {id}"))).await;
    assert!(
        res.is_err(),
        "audit failure must propagate as delivery-unknown, not command success"
    );

    // Telegram send actually happened (delivery-unknown truth).
    assert_eq!(server.received_requests().await.unwrap().len(), 1);

    // No viewed or notified receipts are written because the transaction rolled back.
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("failure.digest_detail_viewed")
            .unwrap(),
        0
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner.notified")
            .unwrap(),
        0
    );

    // Critical: NO DLQ row is enqueued (so we don't retry and duplicate the send).
    assert!(state.store.pending_dead_letters().unwrap().is_empty());
}

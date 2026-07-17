use super::*;
use crate::test_support::fixtures::*;

pub(crate) mod approval;
mod bot_identity;
mod bot_identity_support;
mod callback_ack;
mod concurrency;
mod digest;
mod draft;
mod driver;
mod effect_paths;
mod offset;
mod plan;
mod secret_intake_integration;
mod task_board;
mod token_rotation;
pub(crate) use approval::approval_fixture_grant;

#[tokio::test]
async fn non_owner_update_is_ignored_and_audited_without_a_grant() {
    let state = test_state();
    let mut update = owner_update("hi");
    update.sender_user_id = Some(999);
    handle_owner_update(&state, &update).await.unwrap();
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
}

#[tokio::test]
async fn malformed_secret_command_is_reserved_from_normal_pipeline() {
    let state = test_state();
    let update = owner_update("/secret intake gmail.refresh extra");
    assert!(handle_owner_update(&state, &update)
        .await
        .unwrap()
        .is_none());
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("event.received")
            .unwrap(),
        0
    );
}
#[tokio::test]
async fn owner_update_composes_authority_and_persists_a_grant_bound_to_the_chat() {
    let state = test_state();
    let update = owner_update("hello lyra");
    // ProcessDriver spawning a real shell binary will fail in this test
    // environment (no `openspine-shell` on PATH) — that's fine, the
    // pipeline still must reach `insert_task_grant` before the spawn
    // attempt, which is what this test asserts by inspecting the
    // returned grant and the store directly.
    let grant = handle_owner_update(&state, &update)
        .await
        .unwrap()
        .expect("owner message must compose a grant");
    assert_eq!(grant.agent_id, "main_assistant_agent");
    assert_eq!(grant.workflow_id, "owner_control_conversation");
    assert_eq!(grant.route_id, "owner_telegram_main_assistant");

    let (stored_grant, pending_ref, bound_chat_id) = state
        .store
        .find_task_grant_by_token(&grant.task_token)
        .unwrap()
        .expect("grant must be persisted");
    // D-047: the persisted grant's task_token is redacted, never round-tripped.
    let mut expected = grant.clone();
    expected.task_token = String::new();
    assert_eq!(stored_grant, expected);
    assert_eq!(bound_chat_id, 555);
    assert_eq!(state.artifacts.get(&pending_ref).unwrap(), b"hello lyra");
    assert!(state.store.verify_audit_chain().unwrap());
}

/// Returns the `payload_refs` digest strings for every audit event of
/// `kind`, in append order. Used to pin that an audited grant ref equals the
/// persisted pending-task ref — a behavior-preserving refactor must not
/// relabel which artifact ref an audit event carries.
pub(super) fn audit_payload_refs(store: &Store, kind: &str) -> Vec<String> {
    store
        .all_audit_event_jsons()
        .unwrap()
        .into_iter()
        .filter_map(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .filter(|v| v.get("kind").and_then(|k| k.as_str()) == Some(kind))
        .flat_map(|v| {
            v.get("payload_refs")
                .and_then(|p| p.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|r| {
                            r.get("digest").and_then(|d| d.as_str()).map(str::to_string)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .collect()
}

pub(super) async fn gmail_state_with_real_thread(
) -> (AppState, wiremock::MockServer, wiremock::MockServer) {
    use crate::gmail::GmailConnector;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "test-token",
            "expires_in": 3600,
        })))
        .mount(&token_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .and(query_param("format", "full"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "messages": [{"payload": {"mimeType": "text/plain", "headers": [], "body": {"data": "aGk"}}}],
        })))
        .mount(&api_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .and(query_param("format", "minimal"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "messages": [{"id": "message-1"}],
        })))
        .mount(&api_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/message-1"))
        .and(query_param("format", "metadata"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "payload": {
                "headers": [{"name": "From", "value": "alice@example.com"}],
            },
        })))
        .mount(&api_server)
        .await;

    let gmail = GmailConnector::new(
        "id".to_string(),
        "secret".to_string(),
        "refresh".to_string(),
        "owner@example.com".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), api_server.uri());
    let mut state = test_state_with_gmail(gmail);
    // D-025: opt in so the containment guard passes and the email-preview
    // lane reaches grant composition (the guard itself is pinned separately).
    state.unsafe_allow_uncontained_private_data = true;
    (state, token_server, api_server)
}

#[tokio::test]
async fn owner_update_grant_pins_original_message_raw_ref_through_to_audit() {
    let state = test_state();
    let update = owner_update("hello lyra");
    let grant = handle_owner_update(&state, &update)
        .await
        .unwrap()
        .expect("owner message must compose a grant");

    // Pin: authority purpose and workflow are the owner-control conversation.
    assert_eq!(grant.purpose, "owner_control_conversation");
    assert_eq!(grant.workflow_id, "owner_control_conversation");

    // Pin: the pending task input persisted with the grant is the ORIGINAL
    // owner message (raw_ref) — the owner-control lane never derives a
    // synthetic pending message.
    let (_stored_grant, pending_ref, _chat) = state
        .store
        .find_task_grant_by_token(&grant.task_token)
        .unwrap()
        .expect("grant must be persisted");
    assert_eq!(state.artifacts.get(&pending_ref).unwrap(), b"hello lyra");

    // Pin: the SAME original-message ref is carried by BOTH the event
    // envelope audit and the authority.granted audit. Collapsing these onto
    // a derived/synthetic ref would silently break the owner-control lane.
    let received_refs = audit_payload_refs(&state.store, "event.received");
    assert_eq!(received_refs, vec![pending_ref.digest.to_string()]);

    let granted_refs = audit_payload_refs(&state.store, "authority.granted");
    assert_eq!(granted_refs, vec![pending_ref.digest.to_string()]);

    assert!(state.store.verify_audit_chain().unwrap());
}

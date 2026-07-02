use super::*;
use crate::test_support::fixtures::*;

#[tokio::test]
async fn non_owner_update_is_ignored_and_audited_without_a_grant() {
    let state = test_state();
    let mut update = owner_update("hi");
    update.sender_user_id = Some(999);
    handle_owner_update(&state, &update).await.unwrap();
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
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
    assert_eq!(stored_grant, grant);
    assert_eq!(bound_chat_id, 555);
    assert_eq!(state.artifacts.get(&pending_ref).unwrap(), b"hello lyra");
    assert!(state.store.verify_audit_chain().unwrap());
}

#[tokio::test]
async fn draft_command_without_gmail_configured_is_a_no_op() {
    let state = test_state();
    let update = owner_update("/draft thread-1");
    let result = handle_owner_update(&state, &update).await.unwrap();
    assert!(result.is_none());
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
}

#[tokio::test]
async fn draft_command_for_a_missing_thread_mints_no_grant() {
    use crate::gmail::GmailConnector;
    use wiremock::matchers::{method, path};
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
        .and(path("/gmail/v1/users/me/threads/missing"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(serde_json::json!({"error": "not found"})),
        )
        .mount(&api_server)
        .await;

    let gmail = GmailConnector::new(
        "id".to_string(),
        "secret".to_string(),
        "refresh".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), api_server.uri());
    let state = test_state_with_gmail(gmail);
    let update = owner_update("/draft missing");
    let result = handle_owner_update(&state, &update).await.unwrap();
    assert!(result.is_none());
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
}

#[tokio::test]
async fn draft_command_for_a_real_thread_composes_a_bound_selection_grant() {
    use crate::gmail::GmailConnector;
    use wiremock::matchers::{method, path};
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
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "messages": [{"payload": {"mimeType": "text/plain", "headers": [], "body": {"data": "aGk"}}}],
        })))
        .mount(&api_server)
        .await;

    let gmail = GmailConnector::new(
        "id".to_string(),
        "secret".to_string(),
        "refresh".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), api_server.uri());
    let mut state = test_state_with_gmail(gmail);
    // D-025: `external_communication` events are refused under
    // `ProcessDriver` unless explicitly overridden — this test is
    // about selection-token/grant composition, not the containment
    // guard (covered separately), so it opts in like the real dev
    // config would for a Process-driver deployment.
    state.unsafe_allow_uncontained_private_data = true;
    let update = owner_update("/draft thread-1");
    let grant = handle_owner_update(&state, &update)
        .await
        .unwrap()
        .expect("a real thread must compose a grant");

    assert_eq!(grant.agent_id, "email_reply_drafter");
    assert_eq!(grant.workflow_id, "selected_thread_email_reply_draft");
    assert_eq!(grant.route_id, "owner_email_selected_thread");
    assert_eq!(grant.selection_tokens.len(), 1);

    let token = state
        .store
        .find_selection_token(grant.selection_tokens[0])
        .unwrap()
        .expect("selection token must be persisted");
    assert_eq!(token.target_id, "thread-1");
    assert!(token.single_use);
    assert!(!token.scope.attachments_allowed);

    let (_, _, bound_chat_id) = state
        .store
        .find_task_grant_by_token(&grant.task_token)
        .unwrap()
        .expect("grant must be persisted");
    assert_eq!(bound_chat_id, 555);
    assert!(state.store.verify_audit_chain().unwrap());
}

#[tokio::test]
async fn draft_command_is_refused_without_the_unsafe_flag_under_process_driver() {
    use crate::gmail::GmailConnector;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "test-token",
            "expires_in": 3600,
        })))
        .expect(0)
        .mount(&token_server)
        .await;
    // The containment guard must refuse before Gmail is ever contacted
    // (D-025) — `.expect(0)` fails the test if `thread_exists` reaches
    // this mock at all, catching a regression in the check's ordering.
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "messages": [{"payload": {"mimeType": "text/plain", "headers": [], "body": {"data": "aGk"}}}],
        })))
        .expect(0)
        .mount(&api_server)
        .await;

    let gmail = GmailConnector::new(
        "id".to_string(),
        "secret".to_string(),
        "refresh".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), api_server.uri());
    // D-025 / O-003: `unsafe_allow_uncontained_private_data` stays at
    // `test_state`'s default `false` here — the containment guard must
    // still refuse an `external_communication` grant under
    // `ProcessDriver`, even though a real thread was found and a
    // selection token could otherwise have been minted.
    let state = test_state_with_gmail(gmail);
    let update = owner_update("/draft thread-1");
    let result = handle_owner_update(&state, &update).await.unwrap();
    assert!(result.is_none());
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
}

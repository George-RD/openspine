use super::*;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn draft_command_without_gmail_configured_is_a_no_op() {
    let state = test_state();
    let update = owner_update("/draft thread-1");
    let result = handle_owner_update(&state, &update).await.unwrap();
    assert!(result.is_none());
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
    // Preflight-failure contract (refactor-pipeline-driver): the Gmail-not
    // configured path must audit `selection.gmail_not_configured` and MUST
    // NOT emit `event.received` — no event envelope is built before the
    // preflight check fails.
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("selection.gmail_not_configured")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("event.received")
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn draft_command_for_a_missing_thread_mints_no_grant() {
    use crate::gmail::GmailConnector;

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
        "owner@example.com".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), api_server.uri());
    let mut state = test_state_with_gmail(gmail);
    // Opt in so the containment guard passes and this test actually reaches
    // the `selection.thread_not_found` preflight path it is named for
    // (without opt-in the guard refuses first as `route.refused_uncontained`).
    state.unsafe_allow_uncontained_private_data = true;
    let update = owner_update("/draft missing");
    let result = handle_owner_update(&state, &update).await.unwrap();
    assert!(result.is_none());
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
    // Preflight-failure contract (refactor-pipeline-driver): thread-not-found
    // must audit `selection.thread_not_found` and MUST NOT emit
    // `event.received` — no event envelope is built before the preflight
    // check fails.
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("selection.thread_not_found")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("event.received")
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn draft_command_for_a_real_thread_composes_a_bound_selection_grant() {
    use crate::gmail::GmailConnector;

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
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "payload": {"headers": [{"name": "From", "value": "sender@example.com"}]},
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
        "owner@example.com".to_string(),
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
    // Preflight-failure contract (refactor-pipeline-driver): the containment
    // refusal must audit `route.refused_uncontained` and MUST NOT emit
    // `event.received` — no event envelope is built before the preflight
    // check fails.
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("route.refused_uncontained")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("event.received")
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn draft_command_composes_email_preview_grant_whose_pending_ref_is_derived_message() {
    let (state, _token_server, _api_server) = gmail_state_with_real_thread().await;
    let update = owner_update("/draft thread-1");
    let grant = handle_owner_update(&state, &update)
        .await
        .unwrap()
        .expect("a real thread must compose a grant");

    // Pin: the pending task input is the DERIVED draft prompt, NOT the raw
    // "/draft thread-1" command text the owner typed.
    let (_stored_grant, pending_ref, _chat) = state
        .store
        .find_task_grant_by_token(&grant.task_token)
        .unwrap()
        .expect("grant must be persisted");
    let pending_bytes = state.artifacts.get(&pending_ref).unwrap();
    assert_ne!(pending_bytes, b"/draft thread-1");

    // Pin: authority.granted audits the DERIVED pending_ref.
    let granted_refs = audit_payload_refs(&state.store, "authority.granted");
    assert_eq!(granted_refs, vec![pending_ref.digest.to_string()]);

    // Pin: event.received audits the ORIGINAL message (raw_ref), preserving the
    // audit surface's complete envelope capture — the Event stage and composition
    // divergence is exactly what the refactor must not collapse.
    let received_refs = audit_payload_refs(&state.store, "event.received");
    assert_eq!(received_refs.len(), 1);
    assert_ne!(received_refs[0], pending_ref.digest.to_string());

    assert!(state.store.verify_audit_chain().unwrap());
}

#[tokio::test]
async fn draft_command_with_gmail_api_error_audits_no_event_received() {
    use crate::gmail::GmailConnector;

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
    // Any non-success, non-404 status makes `thread_exists` return Err,
    // exercising the `selection.gmail_error` preflight path.
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&api_server)
        .await;

    let gmail = GmailConnector::new(
        "id".to_string(),
        "secret".to_string(),
        "refresh".to_string(),
        "owner@example.com".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), api_server.uri());
    // Opt in so the containment guard passes and the flow reaches the Gmail
    // call that errors (otherwise it would be refused_uncontained instead).
    let mut state = test_state_with_gmail(gmail);
    state.unsafe_allow_uncontained_private_data = true;
    let update = owner_update("/draft thread-1");
    let result = handle_owner_update(&state, &update).await.unwrap();
    assert!(result.is_none());
    assert_eq!(state.store.count_task_grants().unwrap(), 0);

    // Preflight-failure contract: a Gmail API error audits
    // `selection.gmail_error` and emits no `event.received`.
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("selection.gmail_error")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("event.received")
            .unwrap(),
        0
    );
}

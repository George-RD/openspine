use super::*;
use crate::test_support::fixtures::*;

mod approval;

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
    // D-047: the persisted grant's task_token is redacted, never round-tripped.
    let mut expected = grant.clone();
    expected.task_token = String::new();
    assert_eq!(stored_grant, expected);
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
    // Preflight-failure contract (refactor-pipeline-driver): the Gmail-not
    // configured path must audit `selection.gmail_not_configured` and MUST
    // NOT emit `event.received` — no event envelope is built before the
    // preflight check fails.
    assert_eq!(
        state.store.count_audit_events_of_kind("selection.gmail_not_configured").unwrap(),
        1
    );
    assert_eq!(state.store.count_audit_events_of_kind("event.received").unwrap(), 0);
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
        state.store.count_audit_events_of_kind("selection.thread_not_found").unwrap(),
        1
    );
    assert_eq!(state.store.count_audit_events_of_kind("event.received").unwrap(), 0);
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
        state.store.count_audit_events_of_kind("route.refused_uncontained").unwrap(),
        1
    );
    assert_eq!(state.store.count_audit_events_of_kind("event.received").unwrap(), 0);
}
/// Returns the `payload_refs` digest strings for every audit event of
/// `kind`, in append order. Used to pin that an audited grant ref equals the
/// persisted pending-task ref — a behavior-preserving refactor must not
/// relabel which artifact ref an audit event carries.
fn audit_payload_refs(store: &Store, kind: &str) -> Vec<String> {
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
                        .filter_map(|r| r.get("digest").and_then(|d| d.as_str()).map(str::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .collect()
}

/// Builds a state wired to a Gmail connector whose `thread_exists("thread-1")`
/// returns `Ok(true)`, with `unsafe_allow_uncontained_private_data` opted in
/// (so the containment guard passes and the selection flow reaches
/// composition). Shared by the email-preview lane characterization tests so
/// the wiremock wiring is declared once.
async fn gmail_state_with_real_thread() -> AppState {
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
        "owner@example.com".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), api_server.uri());
    let mut state = test_state_with_gmail(gmail);
    // D-025: opt in so the containment guard passes and the email-preview
    // lane reaches grant composition (the guard itself is pinned separately).
    state.unsafe_allow_uncontained_private_data = true;
    state
}

#[tokio::test]
async fn owner_update_grant_pins_original_message_raw_ref_through_to_audit() {
    let state = test_state();
    let update = owner_update("hello lyra");
    let grant = handle_owner_update(&state, &update)
        .await
        .unwrap()
        .expect("owner message must compose a grant");

    // Pin: authority purpose is the owner-control conversation workflow.
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

#[tokio::test]
async fn draft_command_composes_email_preview_grant_whose_pending_ref_is_derived_message() {
    let state = gmail_state_with_real_thread().await;
    let update = owner_update("/draft thread-1");
    let grant = handle_owner_update(&state, &update)
        .await
        .unwrap()
        .expect("a real thread must compose a grant");

    // Pin: authority purpose is the selected-thread email reply draft.
    assert_eq!(grant.workflow_id, "selected_thread_email_reply_draft");

    // Pin: the pending task input is the DERIVED draft prompt, NOT the raw
    // "/draft thread-1" command text the owner typed.
    let (_stored_grant, pending_ref, _chat) = state
        .store
        .find_task_grant_by_token(&grant.task_token)
        .unwrap()
        .expect("grant must be persisted");
    let pending_bytes = state.artifacts.get(&pending_ref).unwrap();
    assert!(
        pending_bytes.starts_with(b"Draft a reply to Gmail thread"),
        "pending message must be the derived draft prompt, got: {pending_bytes:?}"
    );
    assert_ne!(pending_bytes, b"/draft thread-1");

    // Pin: authority.granted audits the DERIVED pending_ref.
    let granted_refs = audit_payload_refs(&state.store, "authority.granted");
    assert_eq!(granted_refs, vec![pending_ref.digest.to_string()]);

    // Pin: event.received audits the RAW thread ref (thread-id bytes), which
    // is a DIFFERENT ref from the derived pending message. This owner-vs-email
    // divergence is exactly what the refactor must not collapse.
    let received_refs = audit_payload_refs(&state.store, "event.received");
    assert_eq!(received_refs.len(), 1);
    assert_ne!(received_refs[0], pending_ref.digest.to_string());

    assert!(state.store.verify_audit_chain().unwrap());
}

#[tokio::test]
async fn draft_command_with_gmail_api_error_audits_no_event_received() {
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
        state.store.count_audit_events_of_kind("selection.gmail_error").unwrap(),
        1
    );
    assert_eq!(state.store.count_audit_events_of_kind("event.received").unwrap(), 0);
}

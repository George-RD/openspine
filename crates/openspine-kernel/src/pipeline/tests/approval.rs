use super::*;
use crate::gmail::GmailConnector;
use crate::telegram::TelegramConnector;
use openspine_schemas::action::{ActionId, ActionRequest};
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::digest::digest_of;
use openspine_schemas::event::{TargetRef, TargetRefKind};
use openspine_schemas::grant::GrantLimits;
use serde_json::json;
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A `TaskGrant` for `email_reply_drafter`, bound to chat 555, with
/// `email.create_draft` approval-required — matching
/// `selected_thread_email_draft_pack.yaml`'s real capability pack (PRD
/// §11.2) rather than re-deriving it through the full `/draft` +
/// `lyra.ui.preview` HTTP flow, which these tests have no need to
/// exercise end-to-end.
pub(crate) fn approval_fixture_grant() -> TaskGrant {
    let issued_at = Timestamp::now();
    let mut grant = TaskGrant {
        id: Ulid::new(),
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user: "owner".to_string(),
        purpose: "test".to_string(),
        issued_by: "kernel".to_string(),
        issued_at,
        expires_at: issued_at + std::time::Duration::from_secs(120),
        event_id: Ulid::new(),
        route_id: "owner_email_selected_thread".to_string(),
        agent_id: "email_reply_drafter".to_string(),
        workflow_id: "selected_thread_email_reply_draft".to_string(),
        capability_pack_id: "selected_thread_email_draft_pack".to_string(),
        authority_sources: vec![],
        selection_tokens: vec![],
        allowed_actions: vec![],
        approval_required_actions: vec![ActionId::new("email.create_draft")],
        denied_actions: vec![],
        allowed_egress_classes: vec![],
        output_channels: vec![],
        limits: GrantLimits {
            max_model_calls: 8,
            max_artifacts: 20,
            max_runtime_seconds: 120,
        },
        task_token: "a".repeat(64),
        root_grant_id: Ulid::nil(),
        parent_grant_id: None,
        mode: openspine_schemas::grant::GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
    };
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    grant
}

/// A pending `email.create_draft` request bound to `grant_id`, targeting
/// `thread-1`, approved-against `approved_recipient` (D-041's target
/// digest). `subject`/`body` become the protected payload artifact.
pub(crate) fn approval_fixture_request(
    state: &AppState,
    grant_id: Ulid,
    subject: &str,
    body: &str,
    approved_recipient: &str,
) -> ActionRequest {
    let payload_ref = state
        .artifacts
        .put(
            serde_json::to_vec(&json!({"subject": subject, "body": body}))
                .unwrap()
                .as_slice(),
        )
        .unwrap();
    let target_digest = digest_of(&json!({
        "thread_id": "thread-1",
        "connector": "gmail_primary",
        "account_role": "owner_mailbox",
        "recipients": [approved_recipient],
    }));
    ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant_id,
        action: ActionId::new("email.create_draft"),
        target_ref: Some(TargetRef {
            kind: TargetRefKind::EmailThread,
            id: Some("thread-1".to_string()),
        }),
        payload_ref: Some(payload_ref),
        target_digest: Some(target_digest),
        selection_token_id: None,
        requested_at: Timestamp::now(),
        schema_version: 1,
    }
}

/// A verified owner tap on the "Approve" button for `request_id`.
fn approve_callback_update(request_id: Ulid) -> crate::telegram::TelegramUpdate {
    let mut update = owner_update("");
    update.text = None;
    update.callback_query = Some(crate::telegram::CallbackQueryUpdate {
        id: "cb-1".to_string(),
        data: Some(format!("approve_draft:{request_id}")),
    });
    update
}

/// Mount the Gmail OAuth token endpoint and return a connector pointed at
/// both mock servers. Every approval test needs a token; only the
/// thread-fetch and draft-create mocks vary per test.
async fn gmail_with_token_mock(
    token_server: &MockServer,
    api_server: &MockServer,
) -> GmailConnector {
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "test-token",
            "expires_in": 3600,
        })))
        .mount(token_server)
        .await;
    GmailConnector::new(
        "id".to_string(),
        "secret".to_string(),
        "refresh".to_string(),
        "owner@example.com".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), api_server.uri())
}

fn thread_with_sender(sender: &str) -> serde_json::Value {
    json!({
        "messages": [{
            "payload": {
                "mimeType": "text/plain",
                "headers": [{"name": "From", "value": sender}],
                "body": {"data": "aGk"},
            },
        }],
    })
}

#[tokio::test]
async fn a_double_tap_on_approve_creates_only_one_gmail_draft() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(thread_with_sender("alice@example.com")),
        )
        .mount(&api_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "draft-1"})))
        .expect(1)
        .mount(&api_server)
        .await;

    let gmail = gmail_with_token_mock(&token_server, &api_server).await;
    let telegram_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/AnswerCallbackQuery"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(&telegram_server)
        .await;
    let state = test_state_with_gmail_and_telegram(
        gmail,
        TelegramConnector::with_api_url(
            "test-token".to_string(),
            telegram_server.uri().parse().unwrap(),
        ),
    );
    let grant = approval_fixture_grant();
    let pending_ref = state.artifacts.put(b"hi").unwrap();
    state
        .store
        .insert_task_grant(&grant, &pending_ref, 555)
        .unwrap();
    // Must match exactly what `create_approved_draft` recomputes after
    // fetching the mocked thread above (D-041): the newest non-owner
    // sender is alice@example.com.
    let request = approval_fixture_request(
        &state,
        grant.id,
        "Re: invoice",
        "sounds good",
        "alice@example.com",
    );
    state.store.insert_action_request(&request).unwrap();
    let update = approve_callback_update(request.id);

    // First tap: approves and creates the draft.
    assert!(handle_owner_update(&state, &update)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.created")
            .unwrap(),
        1
    );
    assert!(state
        .store
        .find_approval_for_request(request.id)
        .unwrap()
        .is_some());

    // Second tap on the same (still-live) button: must be a no-op, not a
    // second draft.
    assert!(handle_owner_update(&state, &update)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.created")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.approval_already_handled")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn recipient_mutation_since_approval_is_denied_and_creates_no_draft() {
    // D-041/D-042: the target digest must be re-derived fresh from a live
    // Gmail fetch at approval time and compared byte-for-byte against
    // what was approved — a thread that gained a new message from a
    // different sender between proposal and approval must never let the
    // approved draft go out to the wrong recipient.
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    // The thread now shows bob@example.com as the newest non-owner
    // sender — the approval below was granted for alice@example.com.
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(thread_with_sender("bob@example.com")),
        )
        .mount(&api_server)
        .await;
    // No draft may ever be created for a mutated target.
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "draft-1"})))
        .expect(0)
        .mount(&api_server)
        .await;

    let gmail = gmail_with_token_mock(&token_server, &api_server).await;
    let telegram_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/AnswerCallbackQuery"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(&telegram_server)
        .await;
    let state = test_state_with_gmail_and_telegram(
        gmail,
        TelegramConnector::with_api_url(
            "test-token".to_string(),
            telegram_server.uri().parse().unwrap(),
        ),
    );
    let grant = approval_fixture_grant();
    let pending_ref = state.artifacts.put(b"hi").unwrap();
    state
        .store
        .insert_task_grant(&grant, &pending_ref, 555)
        .unwrap();
    let request = approval_fixture_request(
        &state,
        grant.id,
        "Re: invoice",
        "sounds good",
        "alice@example.com",
    );
    state.store.insert_action_request(&request).unwrap();
    let update = approve_callback_update(request.id);

    assert!(handle_owner_update(&state, &update)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.target_mutated_since_approval")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.created")
            .unwrap(),
        0
    );
    // The approval itself was still recorded (the owner did approve what
    // they were shown) — only the resulting draft creation is blocked.
    assert!(state
        .store
        .find_approval_for_request(request.id)
        .unwrap()
        .is_some());

    // The wiremock `.expect(0)` on the drafts endpoint above is verified
    // on drop when `api_server` goes out of scope at the end of this test.
}

#[tokio::test]
async fn approval_audit_never_contains_the_plaintext_draft_body() {
    // PRD §18 / D-011: private payloads must be stored as encrypted
    // artifact refs, never written directly into the audit event.
    const SUBJECT: &str = "Re: a rather distinctive invoice subject";
    const BODY: &str = "a rather distinctive draft body sentence";

    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(thread_with_sender("alice@example.com")),
        )
        .mount(&api_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "draft-1"})))
        .mount(&api_server)
        .await;

    let gmail = gmail_with_token_mock(&token_server, &api_server).await;
    let telegram_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/AnswerCallbackQuery"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(&telegram_server)
        .await;
    let state = test_state_with_gmail_and_telegram(
        gmail,
        TelegramConnector::with_api_url(
            "test-token".to_string(),
            telegram_server.uri().parse().unwrap(),
        ),
    );
    let grant = approval_fixture_grant();
    let pending_ref = state.artifacts.put(b"hi").unwrap();
    state
        .store
        .insert_task_grant(&grant, &pending_ref, 555)
        .unwrap();
    let request = approval_fixture_request(&state, grant.id, SUBJECT, BODY, "alice@example.com");
    state.store.insert_action_request(&request).unwrap();
    let update = approve_callback_update(request.id);
    assert!(handle_owner_update(&state, &update)
        .await
        .unwrap()
        .is_none());

    let events = state.store.all_audit_event_jsons().unwrap();
    assert!(!events.is_empty());
    for event in &events {
        assert!(
            !event.contains(SUBJECT),
            "audit event leaked the plaintext subject: {event}"
        );
        assert!(
            !event.contains(BODY),
            "audit event leaked the plaintext body: {event}"
        );
    }
}

#[tokio::test]
async fn payload_mutated_since_approval_is_denied_and_creates_no_draft() {
    // D-055.4: the approved draft payload is content-addressed by digest.
    // `create_approved_draft` re-reads the payload from the artifact store
    // and verifies the bytes still hash to the approved digest. A mismatch
    // means tampering/corruption since approval, so no Gmail draft may be
    // created — only the `draft.payload_mutated_since_approval` audit (and a
    // best-effort owner notification) is produced. The Telegram endpoint is
    // mocked so the notification never touches the real network.
    let tg = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "result": {"message_id": 1, "date": 0, "chat": {"id": 555, "type": "private"}, "from": {"id": 1, "is_bot": true, "first_name": "bot"}, "text": "ok"}})))
        .mount(&tg)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".to_string(),
        tg.uri().parse().unwrap(),
    ));
    let grant = approval_fixture_grant();
    // A valid payload ref whose on-disk blob we then overwrite with bytes
    // that do NOT hash to `pending_ref.digest` (D-055.4).
    let pending_ref = state.artifacts.put(b"approved payload").unwrap();
    state
        .artifacts
        .put_tampered_for_test(&pending_ref.digest, b"tampered payload bytes")
        .unwrap();
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: ActionId::new("email.create_draft"),
        target_ref: None,
        payload_ref: Some(pending_ref.clone()),
        target_digest: None,
        selection_token_id: None,
        requested_at: Timestamp::now(),
        schema_version: 1,
    };

    // The mismatch is caught before any Gmail draft creation is attempted.
    crate::pipeline::approval::create_approved_draft(&state, &grant, &request, 555)
        .await
        .unwrap();
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.payload_mutated_since_approval")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.created")
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn owner_notify_routes_through_gate_and_audits() {
    let tg = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "result": {"message_id": 1, "date": 0, "chat": {"id": 555, "type": "private"}, "from": {"id": 1, "is_bot": true, "first_name": "bot"}, "text": "ok"}})))
        .mount(&tg)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".to_string(),
        tg.uri().parse().unwrap(),
    ));
    crate::pipeline::notify_owner_best_effort(&state, 555, "pipeline failure detail").await;
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner.notified")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn activate_approved_artifact_audits_failure_when_no_row() {
    // D-055.1: Path 3 `activate_approved_artifact` is a post-gate-approved-effect.
    // When invoked, if no proposed artifact matches the request ID, it audits
    // `artifact.activation_failed` and exits.
    let state = test_state();
    let grant = approval_fixture_grant();
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: ActionId::new("artifact.activate"),
        target_ref: None,
        payload_ref: None,
        target_digest: None,
        selection_token_id: None,
        requested_at: Timestamp::now(),
        schema_version: 1,
    };

    crate::pipeline::artifact_activation::activate_approved_artifact(&state, &grant, &request, 555)
        .await
        .unwrap();

    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("artifact.activation_failed")
            .unwrap(),
        1
    );
}

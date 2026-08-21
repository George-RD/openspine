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
        user: Ulid::new().into(),
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
        persona_id: None,
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
        params: std::collections::BTreeMap::new(),
        skill_attribution: None,
        requested_at: Timestamp::now(),
        schema_version: 1,
    }
}

/// A verified owner tap on the "Approve" button for `request_id`.
pub(crate) fn approve_callback_update(request_id: Ulid) -> crate::telegram::TelegramUpdate {
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
pub(crate) async fn gmail_with_token_mock(
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

pub(crate) fn thread_with_sender(sender: &str) -> serde_json::Value {
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
        .insert_task_grant(
            &grant,
            &pending_ref,
            &crate::test_support::owner_surface(&state),
        )
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
    // Draft-write pending-evidence success path (D-071 precedent, candidate):
    // a confirmed draft write resolves its pending row.
    assert_eq!(state.store.count_pending_draft_writes().unwrap(), 0);

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
async fn draft_write_timeout_is_delivery_unknown_and_retry_is_fenced() {
    // A timed-out provider write remains delivery-unknown and leaves one
    // durable pending row. A later owner callback for the same protected
    // request is fenced before the Gmail write, rather than claiming
    // exactly-once delivery or automatically resending.
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(thread_with_sender("alice@example.com")),
        )
        .mount(&api_server)
        .await;
    // Draft POST always delays far longer than the connector call budget.
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id": "draft-x"}))
                .set_delay(std::time::Duration::from_millis(500)),
        )
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
    let mut state = test_state_with_gmail_and_telegram(
        gmail,
        TelegramConnector::with_api_url(
            "test-token".to_string(),
            telegram_server.uri().parse().unwrap(),
        ),
    );
    // Tight call budget so the 500ms draft POST always times out.
    state.connector_call_timeout = std::time::Duration::from_millis(50);

    let grant = approval_fixture_grant();
    let pending_ref = state.artifacts.put(b"hi").unwrap();
    state
        .store
        .insert_task_grant(
            &grant,
            &pending_ref,
            &crate::test_support::owner_surface(&state),
        )
        .unwrap();
    let request = approval_fixture_request(
        &state,
        grant.id,
        "Re: invoice",
        "sounds good",
        "alice@example.com",
    );
    let request_fingerprint = crate::store::draft_request_fingerprint(
        request.action.as_str(),
        request
            .target_ref
            .as_ref()
            .and_then(|target| target.id.as_deref())
            .unwrap(),
        request.target_digest.as_ref().unwrap(),
        &request.payload_ref.as_ref().unwrap().digest,
    );
    state.store.insert_action_request(&request).unwrap();
    let update = approve_callback_update(request.id);
    // Times out -> DeliveryUnknown; the callback returns normally while the
    // durable fence remains open for manual reconciliation.
    assert!(handle_owner_update(&state, &update)
        .await
        .unwrap()
        .is_none());

    let retry = approval_fixture_request(
        &state,
        grant.id,
        "Re: invoice",
        "sounds good",
        "alice@example.com",
    );
    state.store.insert_action_request(&retry).unwrap();
    let retry_update = approve_callback_update(retry.id);
    // A second owner callback for the same protected request still fetches
    // the thread but never reaches the Gmail write.
    assert!(handle_owner_update(&state, &retry_update)
        .await
        .unwrap()
        .is_none());

    assert_eq!(state.store.count_pending_draft_writes().unwrap(), 1);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.created")
            .unwrap(),
        0
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.creation_failed")
            .unwrap(),
        0
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.delivery_unknown")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.pending_write_fenced")
            .unwrap(),
        1
    );

    let pending_id: Ulid = state
        .store
        .with_conn_for_test(|conn| {
            conn.query_row(
                "SELECT id FROM pending_draft_writes WHERE state = 'pending'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        })
        .parse()
        .unwrap();
    state.store.resolve_pending_draft_write(pending_id).unwrap();
    assert!(!state
        .store
        .has_pending_draft_write(&request_fingerprint)
        .unwrap());
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
        .insert_task_grant(
            &grant,
            &pending_ref,
            &crate::test_support::owner_surface(&state),
        )
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

#[path = "approval_draft_reconcile_tests.rs"]
mod approval_draft_reconcile_tests;

#[path = "approval_draft_admission_tests.rs"]
mod approval_draft_admission_tests;

#[path = "write_admission_ordering_tests.rs"]
mod write_admission_ordering_tests;

#[path = "approval_actor_tests.rs"]
mod approval_actor_tests;

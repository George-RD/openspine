//! Split from `approval.rs` to keep that file under the 500-line gate.
//! These are submodule tests of the `approval` module, so `super::*`
//! brings the shared fixtures (`approval_fixture_grant`, `test_state_with_*`).
use super::*;
use crate::api::effect_executors::EffectOutcome;

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
        params: std::collections::BTreeMap::new(),
        skill_attribution: None,
        requested_at: Timestamp::now(),
        schema_version: 1,
    };

    let outcome = crate::pipeline::approval::create_approved_draft(
        &state,
        &grant,
        &request,
        &crate::test_support::owner_surface(&state),
    )
    .await
    .unwrap();
    assert_eq!(outcome, EffectOutcome::RefusedPreEffect);
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
async fn target_mutated_since_approval_is_refused_without_a_draft() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(thread_with_sender("alice@example.com")),
        )
        .mount(&api_server)
        .await;
    let gmail = gmail_with_token_mock(&token_server, &api_server).await;
    let telegram_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
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
    let request = approval_fixture_request(
        &state,
        grant.id,
        "Re: invoice",
        "sounds good",
        "bob@example.com",
    );

    let outcome = crate::pipeline::approval::create_approved_draft(
        &state,
        &grant,
        &request,
        &crate::test_support::owner_surface(&state),
    )
    .await
    .unwrap();

    assert_eq!(outcome, EffectOutcome::RefusedPreEffect);
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
}

#[tokio::test]
async fn draft_write_timeout_is_delivery_unknown_and_leaves_pending_row() {
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
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id": "draft-timeout"}))
                .set_delay(std::time::Duration::from_millis(500)),
        )
        .mount(&api_server)
        .await;
    let gmail = gmail_with_token_mock(&token_server, &api_server).await;
    let mut state = test_state_with_gmail(gmail);
    state.connector_call_timeout = std::time::Duration::from_millis(50);
    let grant = approval_fixture_grant();
    let request = approval_fixture_request(
        &state,
        grant.id,
        "Re: invoice",
        "sounds good",
        "alice@example.com",
    );

    let outcome = crate::pipeline::approval::create_approved_draft(
        &state,
        &grant,
        &request,
        &crate::test_support::owner_surface(&state),
    )
    .await
    .unwrap();

    assert_eq!(outcome, EffectOutcome::DeliveryUnknown);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.delivery_unknown")
            .unwrap(),
        1
    );
    assert_eq!(state.store.count_pending_draft_writes().unwrap(), 1);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.created")
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn successful_draft_write_is_executed_and_resolves_pending_row() {
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
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "draft-success"})))
        .mount(&api_server)
        .await;
    let gmail = gmail_with_token_mock(&token_server, &api_server).await;
    let telegram_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
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
    let request = approval_fixture_request(
        &state,
        grant.id,
        "Re: invoice",
        "sounds good",
        "alice@example.com",
    );

    let outcome = crate::pipeline::approval::create_approved_draft(
        &state,
        &grant,
        &request,
        &crate::test_support::owner_surface(&state),
    )
    .await
    .unwrap();

    assert_eq!(outcome, EffectOutcome::Executed);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.created")
            .unwrap(),
        1
    );
    assert_eq!(state.store.count_pending_draft_writes().unwrap(), 0);
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
    crate::pipeline::notify_owner_best_effort(
        &state,
        &crate::test_support::owner_surface(&state),
        "pipeline failure detail",
    )
    .await;
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
        params: std::collections::BTreeMap::new(),
        skill_attribution: None,
        requested_at: Timestamp::now(),
        schema_version: 1,
    };

    crate::pipeline::artifact_activation::activate_approved_artifact(
        &state,
        &grant,
        &request,
        &crate::test_support::owner_surface(&state),
        None,
    )
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

/// Total rows in the fence table, resolved or not. `count_pending_draft_writes`
/// counts only `state = 'pending'`, so it cannot see a row that was inserted
/// and then resolved — which is exactly what the ordering assertion below
/// needs to rule out.
pub(crate) fn total_pending_draft_write_rows(state: &AppState) -> i64 {
    state
        .store
        .conn
        .lock()
        .query_row("SELECT COUNT(*) FROM pending_draft_writes", [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[tokio::test]
async fn unavailable_gmail_connector_refuses_before_any_fence_row() {
    // An Open breaker blocks the executor at its FIRST connector call — the
    // live thread fetch — so it exits before the write is ever admitted and no
    // pending-write fence row is recorded. Deliberately scoped to that: it
    // cannot pin write-admission ordering, because a breaker open enough to
    // reject the write also rejects the preceding fetch. The write-admission
    // ordering is pinned by the rate-limit test below.
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    // No mocks mounted: the Open breaker must block before any Gmail call.
    let gmail = gmail_with_token_mock(&token_server, &api_server).await;
    let state = test_state_with_gmail(gmail);
    let grant = approval_fixture_grant();
    let request = approval_fixture_request(
        &state,
        grant.id,
        "Re: invoice",
        "sounds good",
        "alice@example.com",
    );
    // Trip the gmail breaker (default failure threshold is 3).
    for _ in 0..3 {
        state.connectors.record_connector_outcome("gmail", false);
    }

    let result = crate::pipeline::approval::create_approved_draft(
        &state,
        &grant,
        &request,
        &crate::test_support::owner_surface(&state),
    )
    .await;

    assert!(
        result.is_err(),
        "an unavailable gmail connector propagates as an error, not an outcome: {result:?}"
    );
    assert_eq!(
        total_pending_draft_write_rows(&state),
        0,
        "refusing at the thread fetch must record no pending-write fence"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.created")
            .unwrap(),
        0
    );
}

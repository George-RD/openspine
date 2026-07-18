//! Split from `approval.rs` to keep that file under the 500-line gate.
//! These are submodule tests of the `approval` module, so `super::*`
//! brings the shared fixtures (`approval_fixture_grant`, `test_state_with_*`).
use super::*;

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

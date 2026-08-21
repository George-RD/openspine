//! Split from `approval.rs` to keep that file under the 500-line gate.
//! These are submodule tests of the `approval` module, so `super::*`
//! brings the shared fixtures (`approval_fixture_grant`, `gmail_with_token_mock`,
//! `test_state_with_gmail_and_telegram`, ...).
use super::*;

#[tokio::test]
async fn approval_records_verified_surface_principal_as_approver_and_audit_actor() {
    // D-004/D-003 (spec #197): `approved_by` and the `approval.recorded`
    // audit actor are sourced from the verified owner surface principal, not
    // from the raw Telegram `owner_user_id` config scalar.
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

    let expected =
        openspine_schemas::ids::PrincipalId::from(state.telegram_owner_surface().principal_id());
    // The approver of record is the verified principal, not the config scalar.
    assert_ne!(
        expected.to_string(),
        state.owner.telegram_binding().to_string(),
        "the verified principal must not be the raw Telegram owner user id"
    );
    let approval = state
        .store
        .find_approval_for_request(request.id)
        .unwrap()
        .expect("approval recorded");
    assert_eq!(approval.approved_by, expected);

    // The owner-authored `approval.recorded` audit event carries the actor.
    let recorded = state
        .store
        .all_audit_event_jsons()
        .unwrap()
        .into_iter()
        .filter_map(|json| serde_json::from_str::<openspine_schemas::audit::AuditEvent>(&json).ok())
        .find(|event| event.kind.as_str() == "approval.recorded")
        .expect("approval.recorded audit event exists");
    assert_eq!(recorded.actor, Some(expected));
}

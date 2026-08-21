//! Channel binding is enforced on the typed owner surface, not on a chat
//! integer (add-channel-neutral-responsibility-review, #129).
//!
//! Before this change the approval callback compared `bound_chat_id != chat_id`
//! — two Telegram-shaped integers threaded through generic kernel seams. The
//! comparison is now between whole `OwnerSurfaceRef` values, which also catches
//! a *cross-channel* replay an integer comparison could never see: a grant
//! bound to the authenticated local terminal cannot be approved by a Telegram
//! tap, even though the terminal lane used to persist the configured owner's
//! chat id as its binding.

use super::approval::{approval_fixture_grant, approval_fixture_request};
use super::*;
use openspine_schemas::owner_surface::OwnerSurfaceRef;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn state_with_callback_ack() -> (AppState, MockServer) {
    let telegram_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/AnswerCallbackQuery"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&telegram_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 1,
                "chat": {"id": 555, "type": "private"},
                "text": "ok"
            }
        })))
        .mount(&telegram_server)
        .await;
    let connector = crate::telegram::TelegramConnector::with_api_url(
        "test-token".to_string(),
        telegram_server.uri().parse().unwrap(),
    );
    (test_state_with_telegram(connector), telegram_server)
}

fn approve_callback(request_id: Ulid) -> crate::telegram::TelegramUpdate {
    let mut update = owner_update("");
    update.text = None;
    update.callback_query = Some(crate::telegram::CallbackQueryUpdate {
        id: "cb-surface".to_string(),
        data: Some(format!("approve_draft:{request_id}")),
    });
    update
}

/// Persist an approvable draft request under a grant bound to `surface`, then
/// drive a real Telegram approve callback through `handle_owner_update`.
async fn approve_via_telegram_against(state: &AppState, surface: &OwnerSurfaceRef) -> Ulid {
    let grant = approval_fixture_grant();
    let pending_ref = state.artifacts.put(b"hi").unwrap();
    state
        .store
        .insert_task_grant(&grant, &pending_ref, surface)
        .unwrap();
    let request = approval_fixture_request(
        state,
        grant.id,
        "Re: invoice",
        "sounds good",
        "alice@example.com",
    );
    state.store.insert_action_request(&request).unwrap();
    handle_owner_update(state, &approve_callback(request.id))
        .await
        .unwrap();
    request.id
}

/// A Telegram tap on a grant bound to a *different* Telegram chat is refused,
/// and the single-use action request is deliberately left unconsumed so the
/// real owner's button still works.
#[tokio::test]
async fn telegram_approval_from_a_foreign_chat_is_refused_without_burning_the_request() {
    let (state, _server) = state_with_callback_ack().await;
    let foreign = crate::test_support::owner_surface_for(&state, 999);
    let request_id = approve_via_telegram_against(&state, &foreign).await;

    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.approval_channel_mismatch")
            .unwrap(),
        1,
        "a foreign owner surface must audit a channel mismatch"
    );
    assert!(
        state
            .store
            .find_approval_for_request(request_id)
            .unwrap()
            .is_none(),
        "a mismatched surface must not mint an approval"
    );
    assert!(
        state.store.try_consume_action_request(request_id).unwrap(),
        "a refused callback must leave the single-use request unconsumed"
    );
}

/// Cross-channel replay: a grant bound to the authenticated local terminal is a
/// different surface from the Telegram owner chat — same principal, different
/// channel — so a Telegram tap cannot approve it. A `bound_chat_id: i64`
/// comparison structurally could not detect this, because the terminal lane
/// persisted the configured owner's chat id as its binding.
#[tokio::test]
async fn telegram_cannot_approve_a_grant_bound_to_the_terminal_surface() {
    let (state, _server) = state_with_callback_ack().await;
    let terminal = OwnerSurfaceRef::authenticated_terminal(state.owner.principal_id.as_ulid());
    assert_eq!(
        terminal.principal_id(),
        state.telegram_owner_surface().principal_id(),
        "the two surfaces differ by channel, not by principal"
    );
    assert_ne!(terminal, state.telegram_owner_surface());

    let request_id = approve_via_telegram_against(&state, &terminal).await;
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.approval_channel_mismatch")
            .unwrap(),
        1,
        "a terminal-bound grant must refuse a Telegram approval"
    );
    assert!(
        state
            .store
            .find_approval_for_request(request_id)
            .unwrap()
            .is_none(),
        "no approval may be minted across owner surfaces"
    );
}

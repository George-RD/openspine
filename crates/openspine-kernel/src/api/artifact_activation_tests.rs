//! Full propose→approve→activate flow tests (5d/5h): the digest-bound
//! approval spec.md's "Owner approves a proposal" scenario describes, and
//! the duplicate guard once an artifact is genuinely `active` (as opposed
//! to `artifact_propose_tests::artifact_propose_rejects_duplicate_id_version`'s
//! "still pending" case). Split out of `artifact_propose_tests.rs` purely
//! to keep both files under the 500-line gate, mirroring the
//! `dispatch_tests.rs` / `preview_tests.rs` split.
//!
//! Both tests drive `dispatch_artifact_propose` (the dispatch entry
//! point, `pub(super)` within `api`) and then `handle_owner_update` with a
//! synthesised "Approve" callback update (the same `VerifiedUpdate::OwnerCallback`
//! routing production traffic goes through) against the *same* `AppState`
//! — see `artifact_propose_tests`'s module doc for why these tests call
//! the dispatch function directly rather than going through the HTTP
//! router: an HTTP round trip would consume `AppState` into an `Arc`
//! neither the approval-callback step nor the final registry/overlay
//! assertions could then reach.

use serde_json::json;
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use openspine_schemas::artifact::Lifecycle;

use super::actions::DispatchError;
use super::artifact_propose::dispatch_artifact_propose;
use super::artifact_propose_tests::route_yaml;
use super::dispatch_tests::OWNER_CHAT_ID;
use crate::pipeline::handle_owner_update;
use crate::telegram::{CallbackQueryUpdate, TelegramConnector, TelegramUpdate};
use crate::test_support::fixtures::{owner_update, test_state_with_telegram};

/// A verified owner tap on the "Approve" button for `action_request_id` —
/// same shape as `pipeline::tests::approval`'s private helper of the same
/// name, redefined here since that one isn't reachable from `api`.
fn approve_callback_update(action_request_id: Ulid) -> TelegramUpdate {
    let mut update = owner_update("");
    update.text = None;
    update.callback_query = Some(CallbackQueryUpdate {
        id: "cb-1".to_string(),
        data: Some(format!("approve_draft:{action_request_id}")),
    });
    update
}

/// Mount an unconditional-success `SendMessage` mock with no call-count
/// assertion. Both tests below trigger two real Telegram sends (the
/// approval button, then the post-activation "now active" notification) —
/// neither send's wire shape is what these tests are about;
/// `artifact_propose_tests::artifact_propose_persists_and_sends_approval_button`
/// already covers the approval button's exact shape.
fn telegram_stub(server: &MockServer) -> TelegramConnector {
    TelegramConnector::with_api_url("test-token".to_string(), server.uri().parse().unwrap())
}

async fn mount_send_message_ok(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": OWNER_CHAT_ID, "type": "private"},
                "text": "sent",
            }
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn approved_artifact_activates_into_registry_and_overlay() {
    let server = MockServer::start().await;
    mount_send_message_ok(&server).await;

    let state = test_state_with_telegram(telegram_stub(&server));
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");

    let payload = json!({"kind": "route", "yaml": route_yaml("newly_proposed_route", "proposed")});
    let result = dispatch_artifact_propose(&state, &grant, OWNER_CHAT_ID, Some(&payload))
        .await
        .expect("a well-formed proposal must be accepted");
    let action_request_id: Ulid = result["action_request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    handle_owner_update(&state, &approve_callback_update(action_request_id))
        .await
        .expect("the approval callback must run cleanly");

    {
        let registry = state.registry.read();
        let activated = registry
            .routes
            .iter()
            .find(|r| r.id == "newly_proposed_route" && r.version == 1)
            .expect(
                "the approved route must be inserted into the live registry, participating \
                 in composition exactly like a fixture-loaded route",
            );
        assert_eq!(activated.lifecycle_state, Lifecycle::Active);
    }

    let overlay_path = state
        .overlay_dir
        .join("routes")
        .join("newly_proposed_route-v1.yaml");
    let overlay_text = std::fs::read_to_string(&overlay_path)
        .expect("an activated artifact must be persisted to the on-disk overlay");
    assert!(overlay_text.contains("lifecycle_state: active"));

    let row = state
        .store
        .find_proposed_artifact_by_action_request(action_request_id)
        .unwrap()
        .expect("the proposed_artifacts row must still exist after activation");
    assert_eq!(row.state, Lifecycle::Active);

    // D-055.1: Path 3 is gate-mediated (preceding gate() Allow) and audited
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("artifact.activated")
            .unwrap(),
        1,
        "Expected exactly one artifact.activated audit event"
    );
}

#[tokio::test]
async fn activation_with_mutated_payload_is_denied() {
    // Name is task-mandated (tasks.md §7). What it actually proves is the
    // "no duplicate after activation" half of spec.md's digest-binding
    // requirement ("A duplicate proposal for an already-active id and
    // version is rejected") — the digest-*mismatch* denial itself is
    // `openspine_gate::gate`'s `approved_but_payload_changed_since_is_denied_not_reasked`,
    // exercised once at the shared `gate()` level and deliberately not
    // re-derived per action here.
    let server = MockServer::start().await;
    mount_send_message_ok(&server).await;

    let state = test_state_with_telegram(telegram_stub(&server));
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");

    let payload = json!({"kind": "route", "yaml": route_yaml("already_active_route", "proposed")});
    let result = dispatch_artifact_propose(&state, &grant, OWNER_CHAT_ID, Some(&payload))
        .await
        .expect("a well-formed proposal must be accepted");
    let action_request_id: Ulid = result["action_request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    handle_owner_update(&state, &approve_callback_update(action_request_id))
        .await
        .expect("the approval callback must run cleanly");

    // Sanity: the artifact really is active before re-proposing it.
    assert!(state
        .registry
        .read()
        .routes
        .iter()
        .any(|r| r.id == "already_active_route" && r.version == 1));

    let second_payload =
        json!({"kind": "route", "yaml": route_yaml("already_active_route", "proposed")});
    let err = dispatch_artifact_propose(&state, &grant, OWNER_CHAT_ID, Some(&second_payload))
        .await
        .unwrap_err();
    match err {
        DispatchError::BadRequest(msg) => {
            assert!(msg.contains("already exists"), "unexpected message: {msg}")
        }
        DispatchError::Internal(_) => panic!(
            "a re-proposal of an already-active id/version must be a BadRequest, not Internal \
             — an Internal result here would mean the duplicate guard was bypassed and the \
             attempt instead hit the store's UNIQUE constraint"
        ),
    }
}

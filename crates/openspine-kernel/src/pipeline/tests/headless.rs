use super::*;
use crate::pipeline::handle_owner_update;
use crate::pipeline::headless::{run_headless_hook, HeadlessHookOutcome, HeadlessHookRequest};
use crate::sandbox::{ProcessDriver, Sandbox};
use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::event::{ChannelTrust, EventType, Lane, Source};
use openspine_schemas::pack::AppliesTo;
use openspine_schemas::persona::PersonaElement;
use openspine_schemas::route::{Route, RouteEffect, RouteWhen};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::telegram::CallbackQueryUpdate;
#[test]
fn production_owner_route_binds_seeded_persona() {
    let state = test_state();
    let persona = crate::store::personality_seed::seed_definitions()
        .into_iter()
        .find(|persona| persona.id == "honest_counsel_with_recommendation")
        .expect("D-095 seeded persona definition");
    state
        .registry
        .write()
        .personas
        .insert(persona.id.clone(), persona);
    let registry = state.registry.read();
    let route = registry
        .routes
        .iter()
        .find(|route| route.id == "owner_telegram_main_assistant")
        .expect("production owner-control route");
    assert_eq!(
        route.persona.as_deref(),
        Some("honest_counsel_with_recommendation")
    );
    assert_eq!(
        openspine_authority::resolve_persona(
            "owner-telegram-main",
            Some(openspine_schemas::identity::RelationshipKind::Owner),
            route,
            &registry.personas,
        )
        .as_deref(),
        Some("honest_counsel_with_recommendation")
    );
}

const HOOK_ID: &str = "github-main";
const WEBHOOK_PACK: &str = "headless_webhook_pack";

fn install_headless_route(
    state: &AppState,
    action: &str,
    approval_required: bool,
    persona: Option<&str>,
) {
    let mut registry = state.registry.write();
    let mut pack = registry
        .packs
        .get("owner_control_basic_pack")
        .expect("fixture basic pack")
        .clone();
    pack.id = WEBHOOK_PACK.to_string();
    pack.applies_to = AppliesTo {
        event_type: Some(EventType::WebhookReceived),
        channel_trust: Some(ChannelTrust::VerifiedContact),
        verified_source: Some(true),
        lane: Some(Lane::BusinessWorkflow),
        ..Default::default()
    };
    let action_id = ActionId::new(action);
    pack.candidate_allowed_actions = vec![action_id.clone()];
    pack.approval_required = if approval_required {
        vec![action_id]
    } else {
        vec![]
    };
    pack.denied_actions.clear();
    registry.packs.insert(WEBHOOK_PACK.to_string(), pack);
    registry
        .routes
        .retain(|route| route.id != "headless_webhook_route");
    registry.routes.push(Route {
        id: "headless_webhook_route".to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        priority: Some(10_000),
        effect: RouteEffect::Allow,
        when: RouteWhen {
            source: Some(Source::Webhook),
            event_type: Some(EventType::WebhookReceived),
            verified_source: Some(true),
            lane: Some(Lane::BusinessWorkflow),
            channel_account: Some(HOOK_ID.to_string()),
            ..Default::default()
        },
        agent: Some("main_assistant_agent".to_string()),
        workflow: Some("owner_control_conversation".to_string()),
        capability_pack: Some(WEBHOOK_PACK.to_string()),
        persona: persona.map(str::to_string),
    });
}

fn signed_request(
    state: &AppState,
    now: Timestamp,
    key: &str,
    action: &str,
) -> HeadlessHookRequest {
    let payload = br#"{"event":"push"}"#.to_vec();
    let signed_at = now - Duration::from_secs(1);
    let signature = state
        .webhook_verifier
        .signature_bound(signed_at, key, HOOK_ID, action, &payload);
    HeadlessHookRequest {
        payload,
        signature,
        idempotency_key: key.to_string(),
        signed_at,
        channel_account: HOOK_ID.to_string(),
        action: ActionId::new(action),
    }
}

#[tokio::test]
async fn verified_hook_no_approval_completes_without_owner_conversation_and_records_one_digest() {
    let mut state = test_state();
    let probe = tempfile::tempdir().expect("shell probe tempdir");
    let marker = probe.path().join("run-task-called");
    let script = probe.path().join("shell-probe.sh");
    std::fs::write(&script, format!("#!/bin/sh\ntouch {}\n", marker.display()))
        .expect("shell probe script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("shell probe executable");
    }
    state.sandbox = Sandbox::Process(ProcessDriver {
        shell_binary: script,
        scratch_root: probe.path().join("scratch"),
    });
    install_headless_route(&state, "openspine.status.read", false, None);
    let now = Timestamp::now();
    let outcome = run_headless_hook(
        &state,
        signed_request(&state, now, "headless-no-approval", "openspine.status.read"),
        now,
    )
    .await
    .expect("headless hook completes");

    assert!(matches!(outcome, HeadlessHookOutcome::Completed(_)));
    let digest = state.store.owner_digest_items().expect("digest rows");
    assert_eq!(
        digest.len(),
        1,
        "silent completion creates exactly one digest row"
    );
    assert_eq!(digest[0].class, "headless");
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner.notify_attempted")
            .unwrap(),
        0,
        "no-approval hook never creates an owner conversation"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("headless.hook_completed")
            .unwrap(),
        1
    );
    assert_eq!(state.store.count_task_grants().unwrap(), 1);
    assert!(
        !marker.exists(),
        "headless no-approval execution never invokes the conversational shell"
    );
}

#[tokio::test]
async fn headless_owner_route_binds_owner_persona_before_grant() {
    let state = test_state();
    state.registry.write().personas.insert(
        "owner-facing".to_string(),
        PersonaElement {
            id: "owner-facing".to_string(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Active,
            guidance: "owner-facing".to_string(),
        },
    );
    install_headless_route(&state, "openspine.status.read", false, Some("owner-facing"));
    let now = Timestamp::now();
    let outcome = run_headless_hook(
        &state,
        signed_request(&state, now, "headless-persona", "openspine.status.read"),
        now,
    )
    .await
    .expect("persona-bound hook completes");
    let grant_id = match outcome {
        HeadlessHookOutcome::Completed(id) => id,
        other => panic!("unexpected outcome: {other:?}"),
    };
    let (grant, _, _) = state
        .store
        .find_task_grant_by_id(grant_id)
        .unwrap()
        .expect("persisted headless grant");
    assert_eq!(grant.persona_id.as_deref(), Some("owner-facing"));
}

#[tokio::test]
async fn verified_hook_approval_required_escalates_through_owner_surface() {
    let telegram_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "result": {"message_id": 1, "date": 0, "chat": {"id": 42, "type": "private"}, "text": "escalated"}
        })))
        .mount(&telegram_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/AnswerCallbackQuery"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(2)
        .mount(&telegram_server)
        .await;
    let telegram = crate::telegram::TelegramConnector::with_api_url(
        "test-token".to_string(),
        telegram_server.uri().parse().unwrap(),
    );
    let state = test_state_with_telegram(telegram);
    install_headless_route(&state, "openspine.status.read", true, None);
    let now = Timestamp::now();
    let outcome = run_headless_hook(
        &state,
        signed_request(&state, now, "headless-approval", "openspine.status.read"),
        now,
    )
    .await
    .expect("approval-required hook escalates");

    assert_eq!(state.store.owner_digest_items().unwrap().len(), 1);
    assert!(matches!(outcome, HeadlessHookOutcome::Escalated(_)));
    let request_id = state
        .store
        .latest_action_request()
        .unwrap()
        .expect("headless approval request")
        .id;
    let mut first_tap = owner_update("");
    first_tap.text = None;
    first_tap.callback_query = Some(CallbackQueryUpdate {
        id: "headless-approve-1".to_string(),
        data: Some(format!("approve_draft:{request_id}")),
    });
    first_tap.chat_id = state.owner_user_id;
    handle_owner_update(&state, &first_tap)
        .await
        .expect("headless approval callback dispatches");
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("headless.approved_dispatched")
            .unwrap(),
        1
    );

    let mut second_tap = owner_update("");
    second_tap.text = None;
    second_tap.callback_query = Some(CallbackQueryUpdate {
        id: "headless-approve-2".to_string(),
        data: Some(format!("approve_draft:{request_id}")),
    });
    second_tap.chat_id = state.owner_user_id;
    handle_owner_update(&state, &second_tap)
        .await
        .expect("duplicate headless approval callback is handled");
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("headless.approved_dispatched")
            .unwrap(),
        1,
        "second tap cannot redispatch the headless action"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.approval_already_handled")
            .unwrap(),
        1
    );
    assert!(
        !state.store.owner_digest_items().unwrap().is_empty(),
        "headless escalation remains visible in the owner digest"
    );
}

#[tokio::test]
async fn webhook_signature_binds_channel_account_and_rejects_route_retargeting() {
    let state = test_state();
    let now = Timestamp::now();
    let mut request = signed_request(&state, now, "headless-retarget", "openspine.status.read");
    request.channel_account = "other-registered-hook".to_string();
    let outcome = run_headless_hook(&state, request, now)
        .await
        .expect("retargeted webhook is a handled drop");

    assert!(matches!(
        outcome,
        HeadlessHookOutcome::Rejected(reason) if reason.contains("signature is invalid")
    ));
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
}

#[tokio::test]
async fn webhook_action_retargeting_is_rejected_by_bound_mac() {
    let state = test_state();
    let now = Timestamp::now();
    let mut request = signed_request(
        &state,
        now,
        "headless-action-retarget",
        "openspine.status.read",
    );
    request.action = ActionId::new("connector.enable");
    let outcome = run_headless_hook(&state, request, now)
        .await
        .expect("retargeted action is a handled rejection");
    assert!(matches!(
        outcome,
        HeadlessHookOutcome::Rejected(reason) if reason.contains("signature is invalid")
    ));
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
}

#[tokio::test]
async fn invalid_webhook_is_dropped_with_rejection_audit_and_no_grant() {
    let state = test_state();
    let now = Timestamp::now();
    let mut request = signed_request(&state, now, "headless-invalid", "openspine.status.read");
    request.signature = "sha256=00".to_string();
    let outcome = run_headless_hook(&state, request, now)
        .await
        .expect("invalid webhook is a handled drop");

    assert!(matches!(outcome, HeadlessHookOutcome::Rejected(_)));
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("webhook.rejected")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn replayed_webhook_is_dropped_with_rejection_audit() {
    let state = test_state();
    install_headless_route(&state, "openspine.status.read", false, None);
    let now = Timestamp::now();
    let request = signed_request(&state, now, "headless-replay", "openspine.status.read");
    let first = run_headless_hook(&state, request.clone(), now)
        .await
        .expect("first delivery");
    let second = run_headless_hook(&state, request, now)
        .await
        .expect("replayed delivery is a handled drop");

    assert!(matches!(first, HeadlessHookOutcome::Completed(_)));
    assert!(
        matches!(second, HeadlessHookOutcome::Rejected(reason) if reason.contains("already consumed"))
    );
    assert_eq!(state.store.count_task_grants().unwrap(), 1);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("webhook.rejected")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn stale_webhook_outside_replay_window_is_rejected_without_grant() {
    let state = test_state();
    install_headless_route(&state, "openspine.status.read", false, None);
    let now = Timestamp::now();
    // Signature is valid for its own timestamp, but the timestamp is
    // far outside the verifier replay window.
    let payload = br#"{"event":"push"}"#.to_vec();
    let signed_at = now - Duration::from_secs(400);
    let signature = state.webhook_verifier.signature_bound(
        signed_at,
        "headless-stale",
        HOOK_ID,
        "openspine.status.read",
        &payload,
    );
    let request = HeadlessHookRequest {
        payload,
        signature,
        idempotency_key: "headless-stale".to_string(),
        signed_at,
        channel_account: HOOK_ID.to_string(),
        action: ActionId::new("openspine.status.read"),
    };
    let outcome = run_headless_hook(&state, request, now)
        .await
        .expect("stale webhook is a handled drop");

    assert!(matches!(
        outcome,
        HeadlessHookOutcome::Rejected(reason) if reason.contains("replay window")
    ));
    assert_eq!(
        state.store.count_task_grants().unwrap(),
        0,
        "a rejected stale webhook must never mint a grant"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("webhook.rejected")
            .unwrap(),
        1
    );
}

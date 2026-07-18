// openspine:allow-large-module reason: standing-rule mediation regressions cover the shared production boundary and durable lifecycle semantics
//! API-layer regression tests for the standing-rule mediation boundary
//! (AD-010/AD-106/AD-012). These drive the shared `mediate_and_dispatch_action`
//! boundary and the production `artifact.revoke` dispatch route, asserting
//! durable budget state and audit outcomes — not just helper return values.

use jiff::Timestamp;
use openspine_schemas::action::{ActionId, GateDecision};
use openspine_schemas::digest::canonical_json;
use openspine_schemas::standing_rule::{BudgetWindow, DarkWindowConfig, DarkWindowDefault};
use serde_json::json;
use std::time::Duration;

use crate::api::actions::{mediate_and_dispatch_action, DispatchError, FailureSurface};
use crate::api::dispatch_tests::{mint_grant_with_selection_token, OWNER_CHAT_ID};
use crate::pipeline::handle_owner_update;
use crate::store::standing_rules::standing_rule_fingerprint;
use crate::store::standing_rules_tests::manifest;
use crate::store::Store;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::{owner_update, test_state, test_state_with_telegram};
use rusqlite::params;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn reserved_usage_count(store: &Store, rule_id: &str) -> i64 {
    store
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(DISTINCT reservation_id) FROM standing_rule_usage WHERE rule_id = ?1 AND status = 'reserved'",
            params![rule_id],
            |r| r.get(0),
        )
        .unwrap()
}

fn committed_usage_count(store: &Store, rule_id: &str) -> i64 {
    store
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(DISTINCT reservation_id) FROM standing_rule_usage WHERE rule_id = ?1 AND status = 'committed'",
            params![rule_id],
            |r| r.get(0),
        )
        .unwrap()
}

fn pending_token_consumed_at(store: &Store, pending_id: &str) -> Option<i64> {
    store
        .conn
        .lock()
        .query_row(
            "SELECT token_consumed_at FROM standing_rule_pending_actions WHERE pending_id = ?1",
            params![pending_id],
            |r| r.get(0),
        )
        .unwrap()
}

#[tokio::test]
async fn standing_rule_full_mediate_flow_with_activated_rule() {
    // Full `mediate_and_dispatch_action` flow with an activated standing rule:
    // an otherwise-approval-required action is admitted within budget, the
    // effect runs, the reservation is finalized, and headroom is returned.
    let state = test_state();
    let store = state.store.clone();
    let now = Timestamp::now();
    let m = manifest(
        "rule-mediate",
        "connector.enable",
        3600,
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 60,
        },
        None,
    );
    store.activate_standing_rule(&m, None, now).unwrap();
    let grant = handle_owner_update(&state, &owner_update("enable something"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");

    let (decision, _deferral, result, budget) = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("connector.enable"),
        OWNER_CHAT_ID,
        None,
        FailureSurface::DirectResponse,
        None,
    )
    .await
    .expect("mediation must succeed for a budgeted standing rule");

    assert!(
        matches!(decision, GateDecision::Allow),
        "standing rule admits the action without fresh owner approval"
    );
    assert!(result.is_some(), "the admitted effect produced a result");
    let budget = budget.expect("headroom is returned on allow");
    assert_eq!(
        (budget.quota_remaining, budget.rate_remaining),
        (4, 4),
        "headroom reflects one reserved-then-committed unit"
    );
    assert_eq!(
        committed_usage_count(&store, "rule-mediate"),
        1,
        "exactly one unit was finalized against the budget"
    );
}

#[tokio::test]
async fn standing_rule_effective_allow_audit_failure_cancels_reservation() {
    // Normal-path fault regression: when the effective-Allow audit append
    // fails, the reserved budget is cancelled and the action is not executed
    // (no leaked reservation, budget unchanged after failure).
    let state = test_state();
    let store = state.store.clone();
    let now = Timestamp::now();
    let m = manifest(
        "rule-allow-audit-fail",
        "connector.enable",
        3600,
        BudgetWindow {
            max: 3,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 3,
            window_secs: 60,
        },
        None,
    );
    store.activate_standing_rule(&m, None, now).unwrap();
    let grant = handle_owner_update(&state, &owner_update("enable something"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");

    store.fail_next_effective_allow_audit_for_test();
    let result = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("connector.enable"),
        OWNER_CHAT_ID,
        None,
        FailureSurface::DirectResponse,
        None,
    )
    .await;
    assert!(
        matches!(result, Err(DispatchError::Resource(_))),
        "a failed effective-Allow audit must surface as an error"
    );
    assert_eq!(
        store
            .standing_rule_remaining("rule-allow-audit-fail", now)
            .unwrap(),
        (3, 3),
        "the reserved budget was cancelled — no leaked headroom after failure"
    );
    assert_eq!(
        committed_usage_count(&store, "rule-allow-audit-fail"),
        0,
        "no budget was consumed"
    );
}

#[tokio::test]
async fn standing_rule_normal_deny_exposes_no_headroom() {
    // A normal consult that ends in DENY (rate saturated while quota remains)
    // must not expose remaining-capacity/headroom metadata — headroom is only
    // returned on an authorized Allow (AD-013/AD-106 calibration is Allow-only).
    let state = test_state();
    let store = state.store.clone();
    let now = Timestamp::now();
    let m = manifest(
        "rule-deny-no-headroom",
        "connector.enable",
        3600,
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 1,
            window_secs: 60,
        },
        None,
    );
    store.activate_standing_rule(&m, None, now).unwrap();
    let grant = handle_owner_update(&state, &owner_update("enable something"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");

    // First consult consumes the single rate unit and is admitted.
    let (decision1, _, _, budget1) = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("connector.enable"),
        OWNER_CHAT_ID,
        None,
        FailureSurface::DirectResponse,
        None,
    )
    .await
    .unwrap();
    assert!(matches!(decision1, GateDecision::Allow));
    let b1 = budget1.unwrap();
    assert_eq!(
        (b1.quota_remaining, b1.rate_remaining),
        (4, 0),
        "after the single rate unit, quota still has headroom"
    );

    // Second consult is denied (rate saturated) with quota still at 4.
    let (decision2, _, _, budget2) = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("connector.enable"),
        OWNER_CHAT_ID,
        None,
        FailureSurface::DirectResponse,
        None,
    )
    .await
    .unwrap();
    assert!(
        matches!(decision2, GateDecision::ApprovalRequired { .. }),
        "rate-saturated consult falls back to normal approval"
    );
    assert!(
        budget2.is_none(),
        "a normal denial must not expose any remaining-capacity/headroom field"
    );
}

#[tokio::test]
async fn standing_rule_delivery_unknown_finalizes_live_reservation() {
    // A mediated Telegram write timeout is ambiguous: the provider may have
    // accepted the message, so the standing-rule unit must remain consumed.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(100)))
        .mount(&server)
        .await;
    let mut state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".into(),
        server.uri().parse().unwrap(),
    ));
    let store = state.store.clone();
    state.connector_call_timeout = Duration::from_millis(5);
    let now = Timestamp::now();
    let rule = manifest(
        "rule-delivery-unknown",
        "telegram.reply:owner_channel",
        3600,
        BudgetWindow {
            max: 1,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 1,
            window_secs: 3600,
        },
        Some(DarkWindowConfig {
            timeout_secs: 60,
            default: DarkWindowDefault::Allow,
        }),
    );
    store.activate_standing_rule(&rule, None, now).unwrap();
    let (mut grant, _) = mint_grant_with_selection_token(
        &state,
        &["telegram.reply:owner_channel"],
        now + Duration::from_secs(120),
    );
    grant.allowed_actions.clear();
    grant.approval_required_actions = vec![ActionId::new("telegram.reply:owner_channel")];
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    let result = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("telegram.reply:owner_channel"),
        OWNER_CHAT_ID,
        Some(&json!({"text": "ambiguous delivery"})),
        FailureSurface::DirectResponse,
        None,
    )
    .await;
    assert!(
        matches!(result, Err(DispatchError::DeliveryUnknown(_))),
        "timeout must remain delivery-unknown: {result:?}"
    );
    assert_eq!(committed_usage_count(&store, "rule-delivery-unknown"), 1);
    assert_eq!(reserved_usage_count(&store, "rule-delivery-unknown"), 0);
    assert_eq!(
        store
            .standing_rule_remaining("rule-delivery-unknown", Timestamp::now())
            .unwrap(),
        (0, 0)
    );

    // Fire a dark-window Allow waiver for the same action. Its one-use token
    // must remain consumed when the retried effect is delivery-unknown.
    let active = store
        .active_standing_rule_for_action(&ActionId::new("telegram.reply:owner_channel"), now)
        .unwrap()
        .unwrap();
    let payload = json!({"text": "fired ambiguous delivery"});
    let payload_ref = Some(
        state
            .artifacts
            .put(canonical_json(&payload).as_bytes())
            .unwrap(),
    );
    let fingerprint =
        standing_rule_fingerprint(&active.action_id, grant.id, OWNER_CHAT_ID, &payload_ref);
    let timer_id = store
        .schedule_standing_rule_dark_window(
            &active,
            grant.id,
            OWNER_CHAT_ID,
            payload_ref.clone(),
            &fingerprint,
            now + Duration::from_secs(60),
            now,
        )
        .unwrap()
        .unwrap();
    let pending = store
        .claim_standing_rule_dark_window(&timer_id, now + Duration::from_secs(60))
        .unwrap()
        .unwrap();
    let fired = mediate_and_dispatch_action(
        &state,
        &grant,
        active.action_id.clone(),
        OWNER_CHAT_ID,
        Some(&payload),
        FailureSurface::DirectResponse,
        Some(&pending.pending_id),
    )
    .await;
    assert!(matches!(fired, Err(DispatchError::DeliveryUnknown(_))));
    assert!(pending_token_consumed_at(&store, &pending.pending_id).is_some());
    let dispatch_state: String = store
        .conn
        .lock()
        .query_row(
            "SELECT dispatch_state FROM standing_rule_pending_actions WHERE pending_id = ?1",
            params![pending.pending_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(dispatch_state, "dispatched");

    // A separate, provably pre-effect BadRequest still releases its unit.
    let bad_rule = manifest(
        "rule-delivery-bad-request",
        "telegram.reply:owner_channel",
        3600,
        BudgetWindow {
            max: 1,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 1,
            window_secs: 3600,
        },
        None,
    );
    store
        .activate_standing_rule(&bad_rule, None, Timestamp::now())
        .unwrap();
    let bad = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("telegram.reply:owner_channel"),
        OWNER_CHAT_ID,
        None,
        FailureSurface::DirectResponse,
        None,
    )
    .await;
    assert!(matches!(bad, Err(DispatchError::BadRequest(_))));
    assert_eq!(
        store
            .standing_rule_remaining("rule-delivery-bad-request", Timestamp::now())
            .unwrap(),
        (1, 1)
    );
}

#[tokio::test]
async fn artifact_revoke_dispatch_removes_rule_from_live_consultation() {
    // Evidence regression: the PRODUCTION `artifact.revoke` dispatch route
    // (not a direct store call) removes a standing rule from live consultation.
    let state = test_state();
    let store = state.store.clone();
    let now = Timestamp::now();
    let m = manifest(
        "rule-revoke-dispatch",
        "connector.enable",
        3600,
        BudgetWindow {
            max: 3,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 3,
            window_secs: 60,
        },
        None,
    );
    store.activate_standing_rule(&m, None, now).unwrap();
    assert!(
        store
            .active_standing_rule_for_action(&ActionId::new("connector.enable"), now)
            .unwrap()
            .is_some(),
        "rule is live before revoke"
    );
    let (grant, _token) = mint_grant_with_selection_token(
        &state,
        &["artifact.revoke"],
        now + std::time::Duration::from_secs(120),
    );

    let (_decision, _deferral, result, _budget) = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("artifact.revoke"),
        OWNER_CHAT_ID,
        Some(&json!({ "rule_id": "rule-revoke-dispatch" })),
        FailureSurface::DirectResponse,
        None,
    )
    .await
    .expect("artifact.revoke dispatch must succeed");
    let result = result.expect("artifact.revoke returns a result");
    assert_eq!(result["revoked"], json!(true), "revocation reports success");

    assert!(
        store
            .active_standing_rule_for_action(&ActionId::new("connector.enable"), now)
            .unwrap()
            .is_none(),
        "the production revoke route removes the rule from live consultation"
    );
}

#[tokio::test]
async fn standing_rule_fired_path_audit_failure_rearms_token_once() {
    // Fired-path fault regression: when the effective-Allow audit fails after a
    // fired dark-window default is consumed, the fired reservation is cancelled
    // and the one-use token is rearmed (retryable exactly once). A subsequent
    // successful redispatch consumes the token once with no leaked reservation.
    let state = test_state();
    let store = state.store.clone();
    let now = Timestamp::now();
    let m = manifest(
        "rule-fired-rearm",
        "connector.enable",
        3600,
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 60,
        },
        Some(DarkWindowConfig {
            timeout_secs: 60,
            default: DarkWindowDefault::Allow,
        }),
    );
    store.activate_standing_rule(&m, None, now).unwrap();
    let rule = store
        .active_standing_rule_for_action(&ActionId::new("connector.enable"), now)
        .unwrap()
        .unwrap();
    let grant = handle_owner_update(&state, &owner_update("enable something"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    let grant_id = grant.id;
    let payload_ref = None;
    let fingerprint = crate::store::standing_rules::standing_rule_fingerprint(
        &rule.action_id,
        grant_id,
        OWNER_CHAT_ID,
        &payload_ref,
    );
    let timer_id = store
        .schedule_standing_rule_dark_window(
            &rule,
            grant_id,
            OWNER_CHAT_ID,
            payload_ref.clone(),
            &fingerprint,
            now + std::time::Duration::from_secs(60),
            now,
        )
        .unwrap()
        .expect("timer scheduled");
    let pending = store
        .claim_standing_rule_dark_window(&timer_id, now + std::time::Duration::from_secs(60))
        .unwrap()
        .expect("timer fired as Allow default");

    // First redispatch: the effective-Allow audit fails → fired reservation
    // cancelled and token rearmed.
    store.fail_next_effective_allow_audit_for_test();
    let first = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("connector.enable"),
        OWNER_CHAT_ID,
        None,
        FailureSurface::Detached,
        Some(&pending.pending_id),
    )
    .await;
    assert!(
        matches!(first, Err(DispatchError::Resource(_))),
        "fired-path effective-Allow audit failure must surface as an error"
    );
    assert_eq!(
        reserved_usage_count(&store, &rule.rule_id),
        0,
        "the fired reservation was cancelled on failure (no leak)"
    );
    assert!(
        pending_token_consumed_at(&store, &pending.pending_id).is_none(),
        "the fired one-use token was rearmed (retryable)"
    );

    // Second redispatch: succeeds, consumes the token exactly once.
    let second = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("connector.enable"),
        OWNER_CHAT_ID,
        None,
        FailureSurface::Detached,
        Some(&pending.pending_id),
    )
    .await
    .expect("redispatch must succeed after rearm");
    assert!(
        matches!(second.0, GateDecision::Allow),
        "the rearmed token admits the action"
    );
    assert_eq!(
        committed_usage_count(&store, &rule.rule_id),
        1,
        "the token was consumed exactly once — no leaked/duplicate reservation"
    );
    assert!(
        pending_token_consumed_at(&store, &pending.pending_id).is_some(),
        "the token is consumed after the successful redispatch"
    );
}

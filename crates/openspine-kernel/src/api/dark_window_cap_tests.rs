//! Production-path proof that owner silence cannot be amplified (#135).
//!
//! Store-level tests pin the cap primitive; this drives the real
//! `mediate_and_dispatch_action` boundary, because the amplification the
//! change exists to stop is reachable from there: a worker whose quota is
//! exhausted varies its payload and, before the cap, received a fresh pending
//! exception — and eventually a fresh silence-based Allow — for each variation.

use jiff::Timestamp;
use openspine_schemas::action::{ActionId, GateDecision};
use openspine_schemas::standing_rule::{BudgetWindow, DarkWindowConfig, DarkWindowDefault};
use rusqlite::params;
use serde_json::json;
use std::time::Duration;

use crate::api::actions::{mediate_and_dispatch_action, FailureSurface};
use crate::api::dispatch_tests::{mint_grant_with_selection_token, OWNER_CHAT_ID};
use crate::pipeline::AppState;
use crate::store::standing_rules_tests::manifest;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::test_state_with_telegram;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

const ACTION: &str = "telegram.reply:owner_channel";

fn open_pending_count(state: &AppState) -> i64 {
    state
        .store
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM standing_rule_pending_actions WHERE resolved_at IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn timer_count(state: &AppState) -> i64 {
    state
        .store
        .conn
        .lock()
        .query_row("SELECT COUNT(*) FROM workflow_timers", [], |row| row.get(0))
        .unwrap()
}

fn audit_count(state: &AppState, kind: &str) -> i64 {
    state
        .store
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE kind = ?1",
            params![kind],
            |row| row.get(0),
        )
        .unwrap()
}

/// Acceptance: many distinct over-budget requests through the production
/// admission path create at most the reviewed pending allowance, and every
/// refusal stays an ordinary owner approval that consumes nothing.
#[tokio::test]
async fn varying_the_payload_cannot_amplify_owner_silence() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": OWNER_CHAT_ID, "type": "private"},
                "text": "ok"
            }
        })))
        .mount(&server)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".into(),
        server.uri().parse().unwrap(),
    ));
    let now = Timestamp::now();
    let rule = manifest(
        "rule-silence-cap",
        ACTION,
        7 * 24 * 3600,
        // One unit of quota: the second request onward is over budget and
        // reaches the dark-window scheduler.
        BudgetWindow {
            max: 1,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        Some(DarkWindowConfig {
            timeout_secs: 60,
            default: DarkWindowDefault::Allow,
            max_pending_exceptions: 1,
        }),
    );
    crate::store::standing_rules_tests::activate_or_install_legacy_allow(&state.store, &rule, now);
    let (mut grant, _) =
        mint_grant_with_selection_token(&state, &[ACTION], now + Duration::from_secs(300));
    grant.allowed_actions.clear();
    grant.approval_required_actions = vec![ActionId::new(ACTION)];
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");

    // First request is inside the reviewed budget and is admitted.
    let (first, _, _, budget) = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new(ACTION),
        &crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID),
        Some(&json!({"text": "in budget"})),
        FailureSurface::DirectResponse,
        None,
    )
    .await
    .expect("in-budget admission succeeds");
    assert!(matches!(first, GateDecision::Allow));
    assert!(budget.is_some(), "an authorized Allow reports headroom");

    // Every later request is over budget. Each carries a different payload, so
    // each has a different request fingerprint — the exact amplification the
    // cap exists to refuse.
    for i in 0..25 {
        let (decision, _, _, budget) = mediate_and_dispatch_action(
            &state,
            &grant,
            ActionId::new(ACTION),
            &crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID),
            Some(&json!({ "text": format!("over budget {i}") })),
            FailureSurface::DirectResponse,
            None,
        )
        .await
        .expect("an over-budget request is an ordinary approval, not an error");
        assert!(
            matches!(decision, GateDecision::ApprovalRequired { .. }),
            "an over-budget request stays at ordinary owner approval"
        );
        assert!(
            budget.is_none(),
            "a denial never exposes remaining-capacity metadata"
        );
    }

    assert_eq!(
        open_pending_count(&state),
        1,
        "25 distinct over-budget requests leave exactly one outstanding exception"
    );
    assert_eq!(
        timer_count(&state),
        1,
        "one pending exception means one timer, so silence can fire at most once"
    );
    assert_eq!(
        audit_count(&state, "standing_rule.exception_suppressed_at_cap"),
        24,
        "each refusal leaves durable owner-actionable evidence"
    );
    // Notification pressure is bounded by the cap, not by a separate
    // aggregation surface: the owner card is sent only for a schedule that
    // actually happened. Count only the standing-rule button notification, not
    // the admitted `telegram.reply` effect that shares this mock server.
    let button_sends = server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|req| {
            let body = String::from_utf8_lossy(&req.body);
            body.contains("Standing-rule budget is exhausted")
        })
        .count();
    assert_eq!(
        button_sends, 1,
        "exactly one pending exception means exactly one owner card, under 25 refused schedules"
    );
}

/// Acceptance: a suppressed dark window consumes no budget. The rule's
/// remaining quota after the storm is exactly what the one admitted request
/// left, not one unit per refused schedule.
#[tokio::test]
async fn suppressed_schedules_consume_no_budget() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": OWNER_CHAT_ID, "type": "private"},
                "text": "ok"
            }
        })))
        .mount(&server)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".into(),
        server.uri().parse().unwrap(),
    ));
    let now = Timestamp::now();
    let rule = manifest(
        "rule-silence-budget",
        ACTION,
        7 * 24 * 3600,
        BudgetWindow {
            max: 2,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        Some(DarkWindowConfig {
            timeout_secs: 60,
            default: DarkWindowDefault::Allow,
            max_pending_exceptions: 1,
        }),
    );
    crate::store::standing_rules_tests::activate_or_install_legacy_allow(&state.store, &rule, now);
    let (mut grant, _) =
        mint_grant_with_selection_token(&state, &[ACTION], now + Duration::from_secs(300));
    grant.allowed_actions.clear();
    grant.approval_required_actions = vec![ActionId::new(ACTION)];
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");

    for i in 0..8 {
        let _ = mediate_and_dispatch_action(
            &state,
            &grant,
            ActionId::new(ACTION),
            &crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID),
            Some(&json!({ "text": format!("request {i}") })),
            FailureSurface::DirectResponse,
            None,
        )
        .await;
    }

    let (quota_remaining, _rate_remaining) = state
        .store
        .standing_rule_remaining("rule-silence-budget", Timestamp::now())
        .unwrap();
    assert_eq!(
        quota_remaining, 0,
        "exactly the two reviewed quota units were spent by the two admitted requests"
    );
    assert_eq!(open_pending_count(&state), 1);
}

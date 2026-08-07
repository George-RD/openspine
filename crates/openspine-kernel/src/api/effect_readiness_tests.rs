use crate::api::actions::{mediate_and_dispatch_action, DispatchError, FailureSurface};
use crate::api::dispatch_tests::{mint_grant_with_selection_token, OWNER_CHAT_ID};
use crate::store::standing_rules_tests::manifest;
use crate::test_support::fixtures::test_state;
use jiff::Timestamp;
use openspine_schemas::action::{
    ActionCatalog, ActionId, ActionImplementationDescriptor, ActionImplementationId,
};
use openspine_schemas::digest::canonical_json;
use openspine_schemas::standing_rule::{BudgetWindow, DarkWindowConfig, DarkWindowDefault};
use serde_json::json;
use std::time::Duration;

fn reserved_usage_count(store: &crate::store::Store, rule_id: &str) -> i64 {
    store
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(DISTINCT reservation_id) FROM standing_rule_usage WHERE rule_id = ?1 AND status = 'reserved'",
            rusqlite::params![rule_id],
            |row| row.get(0),
        )
        .unwrap()
}

fn committed_usage_count(store: &crate::store::Store, rule_id: &str) -> i64 {
    store
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(DISTINCT reservation_id) FROM standing_rule_usage WHERE rule_id = ?1 AND status = 'committed'",
            rusqlite::params![rule_id],
            |row| row.get(0),
        )
        .unwrap()
}

fn pending_token_consumed_at(store: &crate::store::Store, pending_id: &str) -> Option<i64> {
    store
        .conn
        .lock()
        .query_row(
            "SELECT token_consumed_at FROM standing_rule_pending_actions WHERE pending_id = ?1",
            rusqlite::params![pending_id],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn is_execution_backed_requires_descriptor_and_registered_executor() {
    let mut state = test_state();
    let descriptor =
        |action: &str, implementation_id: &str, executor_id: &str| ActionImplementationDescriptor {
            schema_version: 1,
            implementation_version: 1,
            action_id: ActionId::new(action),
            implementation_id: ActionImplementationId::new(implementation_id),
            connector_kind: "gmail".to_string(),
            executor_id: executor_id.to_string(),
            executor_version: 1,
            resolver_id: "gmail.thread_recipient".to_string(),
            resolver_version: 1,
        };
    state.action_catalog = ActionCatalog::new([
        ActionId::new("email.create_draft"),
        ActionId::new("email.read_inbox"),
        ActionId::new("email.send"),
    ])
    .with_implementation_descriptors([
        descriptor("email.create_draft", "gmail.draft.v1", "gmail.create_draft"),
        descriptor("email.read_inbox", "gmail.inbox.v1", "gmail.read_inbox"),
    ]);

    assert!(state.is_execution_backed(&ActionId::new("email.create_draft")));
    assert!(!state.is_execution_backed(&ActionId::new("email.read_inbox")));
    assert!(!state.is_execution_backed(&ActionId::new("email.send")));
    assert!(!state.is_execution_backed(&ActionId::new("unknown.future_action")));
}

#[tokio::test]
async fn fired_token_no_executor_cancels_reservation_and_rearms_once() {
    let state = test_state();
    let store = state.store.clone();
    let now = Timestamp::now();
    // #128: `email.create_draft` is scope-bound, so an unbounded rule for it
    // is now refused at activation. This test is about the fired-token
    // reservation lifecycle, not about that action, so it uses an action with
    // no delegation descriptor that still reaches `NoExecutor` — the boundary
    // under test is unchanged.
    let rule = manifest(
        "rule-fired-no-executor",
        "coolify.deploy",
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
            max_pending_exceptions: 1,
        }),
    );
    crate::store::standing_rules_tests::activate_or_install_legacy_allow(&store, &rule, now);
    let active_rule = store
        .active_standing_rule_for_action(&ActionId::new("coolify.deploy"), now)
        .unwrap()
        .unwrap();
    let (mut grant, _) = mint_grant_with_selection_token(
        &state,
        &["coolify.deploy"],
        now + Duration::from_secs(120),
    );
    grant.allowed_actions.clear();
    grant.approval_required_actions = vec![ActionId::new("coolify.deploy")];
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    let payload = json!({"subject": "draft", "body": "body"});
    let payload_ref = Some(
        state
            .artifacts
            .put(canonical_json(&payload).as_bytes())
            .unwrap(),
    );
    let fingerprint = crate::store::standing_rules::standing_rule_fingerprint(
        &active_rule.action_id,
        grant.id,
        OWNER_CHAT_ID,
        &payload_ref,
    );
    let timer_id = store
        .schedule_standing_rule_dark_window(
            &active_rule,
            grant.id,
            OWNER_CHAT_ID,
            payload_ref,
            &fingerprint,
            None,
            None,
            now + Duration::from_secs(60),
            now,
        )
        .unwrap()
        .timer_id()
        .expect("timer scheduled")
        .to_string();
    let pending = store
        .claim_standing_rule_dark_window(&timer_id, now + Duration::from_secs(60))
        .unwrap()
        .expect("timer fired as Allow default");

    let result = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("coolify.deploy"),
        OWNER_CHAT_ID,
        Some(&payload),
        FailureSurface::Detached,
        Some(&pending.pending_id),
    )
    .await;
    assert!(matches!(
        result,
        Err(DispatchError::NoExecutor(id)) if id == ActionId::new("coolify.deploy")
    ));
    assert_eq!(reserved_usage_count(&store, &active_rule.rule_id), 0);
    assert_eq!(committed_usage_count(&store, &active_rule.rule_id), 0);
    assert_eq!(
        store
            .standing_rule_remaining(&active_rule.rule_id, now)
            .unwrap(),
        (5, 5),
        "NoExecutor cancellation restores the full quota and rate budgets"
    );
    assert!(
        pending_token_consumed_at(&store, &pending.pending_id).is_none(),
        "the fired one-use token is re-armed after the reservation is cancelled"
    );
}

/// A failed reservation cancel must NOT re-arm the fired one-use token.
/// `cleanup_pre_effect_reservations` re-arms only inside the `Ok(())` arm of
/// `cancel_standing_rule_reservation`; when the cancel fails the pending row
/// stays `claimed` so recovery surfaces it fail-closed and the token can
/// never be spent twice, and the reserved budget stays reserved rather than
/// being silently released.
#[tokio::test]
async fn fired_token_cancel_failure_does_not_rearm_the_token() {
    let state = test_state();
    let store = state.store.clone();
    let now = Timestamp::now();
    // See the note above: an unbounded rule may no longer be activated for a
    // scope-bound action, and the fired-token lifecycle under test is
    // action-agnostic.
    let rule = manifest(
        "rule-fired-cancel-failure",
        "coolify.deploy",
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
            max_pending_exceptions: 1,
        }),
    );
    crate::store::standing_rules_tests::activate_or_install_legacy_allow(&store, &rule, now);
    let active_rule = store
        .active_standing_rule_for_action(&ActionId::new("coolify.deploy"), now)
        .unwrap()
        .unwrap();
    let (mut grant, _) = mint_grant_with_selection_token(
        &state,
        &["coolify.deploy"],
        now + Duration::from_secs(120),
    );
    grant.allowed_actions.clear();
    grant.approval_required_actions = vec![ActionId::new("coolify.deploy")];
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    let payload = json!({"subject": "draft", "body": "body"});
    let payload_ref = Some(
        state
            .artifacts
            .put(canonical_json(&payload).as_bytes())
            .unwrap(),
    );
    let fingerprint = crate::store::standing_rules::standing_rule_fingerprint(
        &active_rule.action_id,
        grant.id,
        OWNER_CHAT_ID,
        &payload_ref,
    );
    let timer_id = store
        .schedule_standing_rule_dark_window(
            &active_rule,
            grant.id,
            OWNER_CHAT_ID,
            payload_ref,
            &fingerprint,
            None,
            None,
            now + Duration::from_secs(60),
            now,
        )
        .unwrap()
        .timer_id()
        .expect("timer scheduled")
        .to_string();
    let pending = store
        .claim_standing_rule_dark_window(&timer_id, now + Duration::from_secs(60))
        .unwrap()
        .expect("timer fired as Allow default");

    // The fired path is mutually exclusive with the consult path
    // (`actions.rs` takes `if let Some(token) = fired_pending { … } else if
    // … { consult }`), so cleanup makes exactly one cancel call and the
    // one-shot flag lands on the fired reservation.
    store.fail_next_reservation_cancel_for_test();
    let result = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("coolify.deploy"),
        OWNER_CHAT_ID,
        Some(&payload),
        FailureSurface::Detached,
        Some(&pending.pending_id),
    )
    .await;
    assert!(matches!(
        result,
        Err(DispatchError::NoExecutor(id)) if id == ActionId::new("coolify.deploy")
    ));
    assert!(
        pending_token_consumed_at(&store, &pending.pending_id).is_some(),
        "a failed reservation cancel must leave the fired token claimed, never re-armed"
    );
    assert_eq!(
        reserved_usage_count(&store, &active_rule.rule_id),
        1,
        "the cancel failed, so the reserved budget row survives"
    );
    assert_eq!(committed_usage_count(&store, &active_rule.rule_id), 0);
    assert_eq!(
        store.count_audit_events_of_kind("draft.created").unwrap(),
        0,
        "no effect ran, so no draft evidence exists"
    );
}

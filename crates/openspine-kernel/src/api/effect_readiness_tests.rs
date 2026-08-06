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
    let rule = manifest(
        "rule-fired-no-executor",
        "email.create_draft",
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
    store.activate_standing_rule(&rule, None, now).unwrap();
    let active_rule = store
        .active_standing_rule_for_action(&ActionId::new("email.create_draft"), now)
        .unwrap()
        .unwrap();
    let (mut grant, _) = mint_grant_with_selection_token(
        &state,
        &["email.create_draft"],
        now + Duration::from_secs(120),
    );
    grant.allowed_actions.clear();
    grant.approval_required_actions = vec![ActionId::new("email.create_draft")];
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
            now + Duration::from_secs(60),
            now,
        )
        .unwrap()
        .expect("timer scheduled");
    let pending = store
        .claim_standing_rule_dark_window(&timer_id, now + Duration::from_secs(60))
        .unwrap()
        .expect("timer fired as Allow default");

    let result = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("email.create_draft"),
        OWNER_CHAT_ID,
        Some(&payload),
        FailureSurface::Detached,
        Some(&pending.pending_id),
    )
    .await;
    assert!(matches!(
        result,
        Err(DispatchError::NoExecutor(id)) if id == ActionId::new("email.create_draft")
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

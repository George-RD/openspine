//! Effect Truth regressions for scope-matched standing-rule settlement.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use serde_json::json;

use super::scoped_admission_support::*;
use crate::api::actions::{mediate_and_dispatch_action, FailureSurface};

/// #173 on the delegated path: a provider 5xx is delivery-unknown, so the
/// scoped reservation is RETAINED (not cancelled) and the fence stays open.
/// A 5xx was previously collapsed to a confirmed failure, cancelling the
/// reservation and releasing reviewed budget for a write that may have landed.
#[tokio::test]
async fn scoped_provider_5xx_retains_reservation_and_leaves_fence_open() {
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(&env.api_server, 502, json!({"error": {"code": 502}})).await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &grant).await;
    env.state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-5xx", &context),
            None,
            Timestamp::now(),
        )
        .unwrap();

    let result = mediate_and_dispatch_action(
        &env.state,
        &grant,
        ActionId::new("email.create_draft"),
        &crate::test_support::telegram_surface(CHAT_ID),
        Some(&draft_payload()),
        FailureSurface::DirectResponse,
        None,
    )
    .await;

    assert!(
        result.is_err(),
        "an unconfirmed 5xx write is never reported as a success"
    );
    assert_eq!(usage_count(&env.state, "rule-5xx", "reserved"), 1);
    assert_eq!(usage_count(&env.state, "rule-5xx", "committed"), 0);
    assert_eq!(
        env.state
            .store
            .standing_rule_remaining("rule-5xx", Timestamp::now())
            .unwrap(),
        (4, 2),
        "a retained reservation keeps consuming quota and rate"
    );
    assert_eq!(
        env.state.store.count_pending_draft_writes().unwrap(),
        1,
        "the reconciliation fence stays open on a 5xx"
    );
    assert!(audit_count(&env.state, "draft.delivery_unknown") >= 1);
}

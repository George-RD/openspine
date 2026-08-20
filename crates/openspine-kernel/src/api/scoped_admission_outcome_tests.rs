//! `EffectOutcome` acceptance tests for scope-matched standing-rule admission
//! (#128). Every test drives a real `email.create_draft` request through the
//! production mediation path and asserts the durable reservation, fence, and
//! audit consequences of each outcome.
use jiff::Timestamp;
use openspine_schemas::action::{ActionId, GateDecision};
use openspine_schemas::digest::canonical_json;
use rusqlite::params;
use serde_json::json;
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::scoped_admission_support::*;
use crate::api::actions::{mediate_and_dispatch_action, FailureSurface};
use crate::gmail::GmailConnector;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::test_state_with_gmail_and_telegram;
#[tokio::test]
async fn delivery_unknown_retains_reservation_and_leaves_fence_open() {
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(&env.api_server, 200, json!({"no_id": true})).await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &grant).await;
    env.state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-unknown", &context),
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
        "an unconfirmed write is never reported as a success"
    );
    // Retained, not finalized: the rows stay `reserved`. Releasing budget for
    // a write that may have landed would under-count real effects, and
    // finalizing to `committed` would foreclose the recovery the fence exists
    // to enable, because a cancel can only release a row that is still
    // `reserved`.
    assert_eq!(usage_count(&env.state, "rule-unknown", "reserved"), 1);
    assert_eq!(usage_count(&env.state, "rule-unknown", "committed"), 0);
    // A retained reservation still counts against both windows.
    assert_eq!(
        env.state
            .store
            .standing_rule_remaining("rule-unknown", Timestamp::now())
            .unwrap(),
        (4, 2),
        "a retained reservation keeps consuming quota and rate"
    );
    assert_eq!(
        env.state.store.count_pending_draft_writes().unwrap(),
        1,
        "the reconciliation fence stays open"
    );
    assert!(audit_count(&env.state, "draft.delivery_unknown") >= 1);
    // The retained rule is still a *used* rule: its lapse clock is refreshed
    // and the AD-010 drift trigger is re-evaluated, so a responsibility that
    // keeps saturating through ambiguous outcomes is still surfaced for owner
    // re-review even though nothing was ever finalized.
    let last_used: Option<i64> = env
        .state
        .store
        .conn
        .lock()
        .query_row(
            "SELECT last_used_at FROM standing_rules WHERE rule_id = 'rule-unknown'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        last_used.is_some(),
        "a retained reservation still records the rule as used"
    );
}
/// Acceptance: an existing delivery-unknown row is an identity-specific
/// retry fence. The second request returns to owner approval before scoped
/// consultation, so it neither reserves budget nor calls Gmail.
#[tokio::test]
async fn pending_delivery_unknown_fences_scoped_retry_before_reservation() {
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(&env.api_server, 200, json!({"id": "must-not-write"})).await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &grant).await;
    env.state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-fenced-retry", &context),
            None,
            Timestamp::now(),
        )
        .unwrap();

    let payload_ref = env
        .state
        .artifacts
        .put(canonical_json(&draft_payload()).as_bytes())
        .unwrap();
    let target_ref = context
        .target_refs()
        .first()
        .and_then(|target| target.id.clone())
        .expect("resolved target carries the thread id");
    let target_digest = context.target_digest().expect("resolved target digest");
    let fingerprint = crate::store::draft_request_fingerprint(
        "email.create_draft",
        &target_ref,
        target_digest,
        &payload_ref.digest,
    );
    env.state
        .store
        .insert_pending_draft_write(
            Ulid::new(),
            grant.id,
            Ulid::new(),
            &target_ref,
            &fingerprint,
        )
        .unwrap();

    let (decision, budget) = dispatch(&env.state, &grant).await;

    assert!(
        matches!(decision, GateDecision::ApprovalRequired { .. }),
        "a pending write must return to owner approval"
    );
    assert!(budget.is_none(), "fenced retries expose no scoped headroom");
    assert_eq!(drafts_written(&env.api_server).await, 0);
    assert_eq!(usage_count(&env.state, "rule-fenced-retry", "reserved"), 0);
    assert_eq!(env.state.store.count_pending_draft_writes().unwrap(), 1);
    assert!(audit_count(&env.state, "draft.pending_reconciliation_required") >= 1);
}

/// Acceptance: a confirmed post-attempt failure cancels the reservation and
/// resolves the fence — #127 semantics, unchanged.
#[tokio::test]
async fn failure_after_attempt_cancels_reservation() {
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(
        &env.api_server,
        400,
        json!({"error": {"code": 400, "message": "invalid draft"}}),
    )
    .await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &grant).await;
    env.state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-failed", &context),
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

    assert!(result.is_err());
    assert_eq!(usage_count(&env.state, "rule-failed", "committed"), 0);
    assert_eq!(usage_count(&env.state, "rule-failed", "reserved"), 0);
    assert_eq!(
        env.state.store.count_pending_draft_writes().unwrap(),
        0,
        "a confirmed failure resolves the fence"
    );
}

/// Acceptance: a pre-effect refusal inside the executor cancels the
/// reservation and performs no provider write. Here the thread changes
/// between the kernel's context resolution and the executor's own
/// re-derivation, so the digest-bound target no longer matches.
#[tokio::test]
async fn pre_effect_refusal_cancels_reservation_without_writing() {
    let token_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "test-access-token",
            "expires_in": 3600,
        })))
        .mount(&token_server)
        .await;
    let api_server = MockServer::start().await;
    // The first two fetches (rule minting + admission-time resolution) see
    // alice; the executor's re-derivation then sees a new participant.
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(thread_body("alice@example.com")))
        .up_to_n_times(2)
        .mount(&api_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(thread_body("bob@example.com")))
        .mount(&api_server)
        .await;
    mount_drafts(&api_server, 200, json!({"id": "draft-1"})).await;
    let telegram_server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": CHAT_ID, "type": "private"},
                "text": "ok"
            }
        })))
        .mount(&telegram_server)
        .await;
    let gmail = GmailConnector::new(
        "client-id".to_string(),
        "client-secret".to_string(),
        "refresh-token".to_string(),
        OWNER_MAILBOX.to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), api_server.uri());
    let state = test_state_with_gmail_and_telegram(
        gmail,
        TelegramConnector::with_api_url(
            "test-token".into(),
            telegram_server.uri().parse().unwrap(),
        ),
    );
    let grant = mint_draft_grant(&state, "thread-1");
    let context = resolved_context(&state, &grant).await;
    state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-refused", &context),
            None,
            Timestamp::now(),
        )
        .unwrap();

    let result = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("email.create_draft"),
        &crate::test_support::owner_surface_for(&state, CHAT_ID),
        Some(&draft_payload()),
        FailureSurface::DirectResponse,
        None,
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        drafts_written(&api_server).await,
        0,
        "a pre-effect refusal performs no provider write"
    );
    assert_eq!(usage_count(&state, "rule-refused", "committed"), 0);
    assert_eq!(usage_count(&state, "rule-refused", "reserved"), 0);
    assert_eq!(
        state.store.count_pending_draft_writes().unwrap(),
        0,
        "a refusal records no pending-write fence"
    );
}

/// Task 3 boundary: a construction failure leaves the decision at
/// ApprovalRequired, dispatches nothing, consults no rule at all, and names
/// the failure in a durable audit event. Here the grant has no briefcase, so
/// the counterparty is unbound.
#[tokio::test]
async fn unresolvable_context_fails_closed_and_consults_no_rule() {
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(&env.api_server, 200, json!({"id": "draft-1"})).await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &grant).await;
    env.state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-live", &context),
            None,
            Timestamp::now(),
        )
        .unwrap();
    // Remove the briefcase: the bound counterparty is now unresolvable.
    env.state
        .store
        .conn
        .lock()
        .execute(
            "DELETE FROM briefcases WHERE task_grant_id = ?1",
            params![grant.id.to_string()],
        )
        .unwrap();

    let (decision, _) = dispatch(&env.state, &grant).await;

    assert!(matches!(decision, GateDecision::ApprovalRequired { .. }));
    assert_eq!(drafts_written(&env.api_server).await, 0);
    assert_eq!(usage_count(&env.state, "rule-live", "reserved"), 0);
    assert_eq!(usage_count(&env.state, "rule-live", "committed"), 0);
    assert!(audit_count(&env.state, "action.scope_context_unresolved") >= 1);
}

/// The `CounterpartyRef::Unresolved` guard (`scoped_admission.rs:208-218`) with
/// an actually-`Unresolved` briefcase, entered through the kernel action API.
/// `unresolvable_context_fails_closed_and_consults_no_rule` deletes the
/// briefcase and therefore stops one arm earlier, at "grant has no briefcase",
/// so it is not evidence for this guard.
#[tokio::test]
async fn unresolved_counterparty_falls_back_via_action_api() {
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(&env.api_server, 200, json!({"id": "draft-1"})).await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &grant).await;
    env.state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-live", &context),
            None,
            Timestamp::now(),
        )
        .unwrap();
    // Keep the briefcase, but unbind its counterparty: the identity binding is
    // gone while every other resolved dimension still matches the live rule.
    {
        let conn = env.state.store.conn.lock();
        let stored: String = conn
            .query_row(
                "SELECT briefcase_json FROM briefcases WHERE task_grant_id = ?1",
                params![grant.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let mut briefcase: serde_json::Value = serde_json::from_str(&stored).unwrap();
        briefcase["task_shape"]["counterparty"] = json!({
            "kind": "unresolved",
            "channel": "email",
            "identifier": "stranger@example.com",
        });
        conn.execute(
            "UPDATE briefcases SET briefcase_json = ?1 WHERE task_grant_id = ?2",
            params![briefcase.to_string(), grant.id.to_string()],
        )
        .unwrap();
    }

    let (decision, _) = dispatch(&env.state, &grant).await;

    assert!(matches!(decision, GateDecision::ApprovalRequired { .. }));
    assert_eq!(drafts_written(&env.api_server).await, 0);
    assert_eq!(usage_count(&env.state, "rule-live", "reserved"), 0);
    assert_eq!(usage_count(&env.state, "rule-live", "committed"), 0);
    // Bind on this guard's own reason, not merely on "something fell back":
    // deleting the guard still yields ApprovalRequired (the scope no longer
    // matches), so a reason-free assertion would not kill it.
    let reasons: Vec<String> = {
        let conn = env.state.store.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT event_json FROM audit_log
                  WHERE kind = 'action.scope_context_unresolved'",
            )
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert!(
        reasons.iter().any(|event| event.contains(
            "briefcase counterparty is unresolved; reusable delegation requires an identity-bound counterparty"
        )),
        "the Unresolved guard must be the arm that refuses: {reasons:?}"
    );
}

/// MAJOR: the scoped lane's `NoExecutor` arm. A rule matched and reserved, then
/// the catalogued `executor_id` resolved to nothing. That is a proven
/// pre-effect failure — no write future was polled — so the reservation is
/// cancelled and the full budget is restored. This is the one arm of the
/// five-way `EffectOutcome`/dispatch matrix that no other test reaches: the
/// fired-token lane covers a different admission source, and the readiness
/// tests exercise messaging without a reservation lifecycle.
#[tokio::test]
async fn scoped_no_executor_cancels_reservation_and_restores_budget() {
    let mut env = draft_env(&["thread-1"]).await;
    mount_drafts(&env.api_server, 200, json!({"id": "draft-1"})).await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &grant).await;
    env.state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-no-executor", &context),
            None,
            Timestamp::now(),
        )
        .unwrap();
    let before = env
        .state
        .store
        .standing_rule_remaining("rule-no-executor", Timestamp::now())
        .unwrap();
    // The descriptor still names `gmail.create_draft`; the registry no longer
    // resolves it. Admission succeeds, dispatch fails closed.
    env.state.effect_executors = crate::api::effect_executors::EffectExecutorRegistry::empty();

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
        matches!(result, Err(crate::api::actions::DispatchError::NoExecutor(ref id))
            if id == &ActionId::new("email.create_draft")),
        "an unresolvable executor is a typed fail-closed NoExecutor: {result:?}"
    );
    assert_eq!(drafts_written(&env.api_server).await, 0);
    assert_eq!(usage_count(&env.state, "rule-no-executor", "reserved"), 0);
    assert_eq!(usage_count(&env.state, "rule-no-executor", "committed"), 0);
    assert_eq!(
        env.state
            .store
            .standing_rule_remaining("rule-no-executor", Timestamp::now())
            .unwrap(),
        before,
        "a pre-effect NoExecutor failure restores the full quota and rate budgets"
    );
    assert_eq!(
        env.state.store.count_pending_draft_writes().unwrap(),
        0,
        "no executor ran, so no pending-write fence exists"
    );
}

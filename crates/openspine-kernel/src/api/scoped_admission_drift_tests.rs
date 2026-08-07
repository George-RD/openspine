//! Drift and corrupt-binding acceptance tests for scope-matched standing-rule
//! admission (#128): a changed epoch, a changed bound dimension, a changed
//! thread participant set, and a persisted binding whose values disagree with
//! its digest all restore ordinary owner approval before any effect runs.

use jiff::Timestamp;
use openspine_schemas::action::{ActionId, GateDecision};
use rusqlite::params;
use serde_json::{json, Value};
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::scoped_admission_support::*;
use crate::api::actions::{mediate_and_dispatch_action, FailureSurface};
use crate::gmail::GmailConnector;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::test_state_with_gmail_and_telegram;
/// Acceptance: mutating the bound compatibility epoch restores ordinary
/// approval before the effect runs.
#[tokio::test]
async fn mutated_compatibility_epoch_restores_ordinary_approval_before_effect() {
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(&env.api_server, 200, json!({"id": "draft-1"})).await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &grant).await;
    env.state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-epoch", &context),
            None,
            Timestamp::now(),
        )
        .unwrap();
    // A drifted declaration axis: the persisted epoch no longer equals the
    // freshly resolved context's.
    let stale = format!("sha256:{}", "9".repeat(64));
    env.state
        .store
        .conn
        .lock()
        .execute(
            "UPDATE standing_rules SET compatibility_digest = ?1 WHERE rule_id = 'rule-epoch'",
            params![stale],
        )
        .unwrap();

    let (decision, _) = dispatch(&env.state, &grant).await;

    assert!(matches!(decision, GateDecision::ApprovalRequired { .. }));
    assert_eq!(
        drafts_written(&env.api_server).await,
        0,
        "the effect never runs under a drifted rule"
    );
    assert_eq!(usage_count(&env.state, "rule-epoch", "reserved"), 0);
}

/// Acceptance: mutating a bound scope dimension restores ordinary approval.
/// The persisted binding still parses, but its counterparty no longer equals
/// the kernel-resolved one.
#[tokio::test]
async fn mutated_scope_dimension_restores_ordinary_approval_before_effect() {
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(&env.api_server, 200, json!({"id": "draft-1"})).await;
    let reviewed_grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &reviewed_grant).await;
    env.state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-dimension", &context),
            None,
            Timestamp::now(),
        )
        .unwrap();
    // A second grant on the same thread whose briefcase binds a *different*
    // counterparty identity: one bound scope dimension changed.
    let other_grant =
        mint_draft_grant_with_counterparty(&env.state, "thread-1", Ulid::from(22_u128));

    let (decision, _) = dispatch(&env.state, &other_grant).await;

    assert!(matches!(decision, GateDecision::ApprovalRequired { .. }));
    assert_eq!(drafts_written(&env.api_server).await, 0);
    assert_eq!(usage_count(&env.state, "rule-dimension", "reserved"), 0);
}

/// Acceptance: a persisted binding whose stored values disagree with its
/// stored digest fails closed as an invalid scope rather than matching on
/// either half. The digest columns still match the resolved context, so only
/// the canonical `ReviewedActionScope::compare` can reject it.
#[tokio::test]
async fn corrupt_persisted_binding_fails_closed_as_invalid_scope() {
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(&env.api_server, 200, json!({"id": "draft-1"})).await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &grant).await;
    let rule = scoped_manifest("rule-corrupt", &context);
    env.state
        .store
        .activate_standing_rule(&rule, None, Timestamp::now())
        .unwrap();
    // Corrupt the persisted scope so its sealed context-class digest no
    // longer agrees with its stored dimension values. Both fast-path digest
    // columns are left intact.
    let mut corrupt: Value = serde_json::to_value(&rule).unwrap();
    corrupt["reviewed_scope"]["scope"]["context_class_digest"] =
        json!(format!("sha256:{}", "d".repeat(64)));
    env.state
        .store
        .conn
        .lock()
        .execute(
            "UPDATE standing_rules SET rule_json = ?1 WHERE rule_id = 'rule-corrupt'",
            params![serde_json::to_string(&corrupt).unwrap()],
        )
        .unwrap();

    let (decision, _) = dispatch(&env.state, &grant).await;

    assert!(
        matches!(decision, GateDecision::ApprovalRequired { .. }),
        "a corrupt binding is an invalid scope, never a match on either half"
    );
    assert_eq!(drafts_written(&env.api_server).await, 0);
    assert_eq!(usage_count(&env.state, "rule-corrupt", "reserved"), 0);
}

/// The participant set is a bound dimension. A rule reviewed while the thread
/// had one participant stops matching once a new participant posts into that
/// same thread — the thread id is unchanged and the compatibility epoch is
/// unchanged by design, so without the participant binding neither digest
/// could see this drift and the kernel would draft into a conversation the
/// owner never reviewed.
#[tokio::test]
async fn a_new_thread_participant_restores_ordinary_approval_before_effect() {
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
    // The rule is reviewed against a thread whose only participant is alice.
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(thread_body("alice@example.com")))
        .up_to_n_times(1)
        .mount(&api_server)
        .await;
    // Every later fetch sees carol in the thread as well.
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(two_party_thread_body(
                "alice@example.com",
                "carol@example.com",
            )),
        )
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
        "owner@example.com".to_string(),
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
    let reviewed = resolved_context(&state, &grant).await;
    assert_eq!(
        reviewed
            .bound_parameters()
            .get("thread_participants")
            .map(String::as_str),
        Some("alice@example.com"),
        "the rule is reviewed against the single-participant thread"
    );
    state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-participants", &reviewed),
            None,
            Timestamp::now(),
        )
        .unwrap();

    let result = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("email.create_draft"),
        CHAT_ID,
        Some(&draft_payload()),
        FailureSurface::DirectResponse,
        None,
    )
    .await;
    let (decision, _, dispatched, budget) = result.expect("a scope mismatch is not an error");

    assert!(
        matches!(decision, GateDecision::ApprovalRequired { .. }),
        "a changed participant set returns the action to ordinary owner approval"
    );
    assert!(dispatched.is_none());
    assert!(budget.is_none());
    assert_eq!(
        drafts_written(&api_server).await,
        0,
        "the effect does not run under the stale reviewed scope"
    );
    assert_eq!(usage_count(&state, "rule-participants", "reserved"), 0);
    assert_eq!(usage_count(&state, "rule-participants", "committed"), 0);
}

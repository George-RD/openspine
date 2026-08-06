//! Wave-4 evidence for the shared effect-executor boundary.
//!
//! These tests exercise both admission sources that can currently hold a
//! digest-bound `ActionRequest`: standing-rule mediation (which must fail
//! closed before a future scope-matched executor caller exists) and the owner
//! approval path (which must converge on the kernel-owned Gmail executor).

use crate::api::actions::{mediate_and_dispatch_action, DispatchError, FailureSurface};
use crate::api::dispatch_tests::{mint_grant_with_selection_token, OWNER_CHAT_ID};
use crate::gmail::GmailConnector;
use crate::pipeline::handle_owner_update;
use crate::store::standing_rules_tests::manifest;
use crate::telegram::{CallbackQueryUpdate, TelegramConnector};
use crate::test_support::fixtures::{owner_update, test_state, test_state_with_gmail_and_telegram};
use jiff::Timestamp;
use openspine_schemas::action::{ActionId, ActionRequest};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::{digest_of, Digest};
use openspine_schemas::event::{TargetRef, TargetRefKind};
use openspine_schemas::standing_rule::BudgetWindow;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

fn batched_failure_details(state: &crate::pipeline::AppState) -> Vec<String> {
    state
        .store
        .owner_digest_items()
        .unwrap()
        .into_iter()
        .map(|item| {
            let digest = Digest::parse(
                item.text_ref
                    .as_deref()
                    .expect("batched failure detail ref"),
            )
            .unwrap();
            let artifact_ref = ArtifactRef {
                digest,
                schema_version: 1,
            };
            String::from_utf8(state.artifacts.get(&artifact_ref).unwrap()).unwrap()
        })
        .collect()
}

fn draft_created_event(state: &crate::pipeline::AppState) -> Value {
    state
        .store
        .all_audit_event_jsons()
        .unwrap()
        .into_iter()
        .filter_map(|raw| serde_json::from_str::<Value>(&raw).ok())
        .find(|event| event["kind"] == "draft.created")
        .expect("draft.created audit row")
}

#[tokio::test]
async fn delegated_email_draft_fails_closed_and_cancels_reservation() {
    let state = test_state();
    let store = state.store.clone();
    let now = Timestamp::now();
    let rule = manifest(
        "rule-email-draft-no-executor",
        "email.create_draft",
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
    store.activate_standing_rule(&rule, None, now).unwrap();

    let (mut grant, _) = mint_grant_with_selection_token(
        &state,
        &["email.create_draft"],
        now + Duration::from_secs(120),
    );
    grant.allowed_actions.clear();
    grant.approval_required_actions = vec![ActionId::new("email.create_draft")];
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");

    let result = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("email.create_draft"),
        OWNER_CHAT_ID,
        None,
        FailureSurface::DirectResponse,
        None,
    )
    .await;

    assert!(matches!(
        result,
        Err(DispatchError::NoExecutor(id)) if id == ActionId::new("email.create_draft")
    ));
    assert_eq!(
        reserved_usage_count(&store, "rule-email-draft-no-executor"),
        0
    );
    assert_eq!(
        committed_usage_count(&store, "rule-email-draft-no-executor"),
        0
    );
    assert_eq!(
        store
            .standing_rule_remaining("rule-email-draft-no-executor", now)
            .unwrap(),
        (3, 3),
        "a pre-effect NoExecutor failure cancels the standing-rule reservation"
    );
    assert_eq!(
        store.count_audit_events_of_kind("draft.created").unwrap(),
        0,
        "the old successful stub cannot create draft evidence"
    );
    assert!(
        store
            .all_audit_event_jsons()
            .unwrap()
            .iter()
            .all(|event| !event.contains("\"stub\"")),
        "a failed dispatch must not persist a stub result"
    );
}

#[tokio::test]
async fn execution_backed_readiness_and_no_executor_summaries_are_distinct() {
    let backed = test_state();
    assert!(backed.is_execution_backed(&ActionId::new("email.create_draft")));
    assert!(!backed.is_execution_backed(&ActionId::new("email.send")));
    assert!(!backed.is_execution_backed(&ActionId::new("unknown.future_action")));

    let (backed_grant, _) = mint_grant_with_selection_token(
        &backed,
        &["email.create_draft"],
        Timestamp::now() + Duration::from_secs(120),
    );
    let backed_result = mediate_and_dispatch_action(
        &backed,
        &backed_grant,
        ActionId::new("email.create_draft"),
        OWNER_CHAT_ID,
        None,
        FailureSurface::DirectResponse,
        None,
    )
    .await;
    assert!(matches!(backed_result, Err(DispatchError::NoExecutor(_))));
    let backed_details = batched_failure_details(&backed);
    assert!(backed_details.iter().any(|detail| {
        detail.contains("email.create_draft: executor registered but not reachable on this path")
    }));

    let unbacked = test_state();
    let (unbacked_grant, _) = mint_grant_with_selection_token(
        &unbacked,
        &["email.send"],
        Timestamp::now() + Duration::from_secs(120),
    );
    let unbacked_result = mediate_and_dispatch_action(
        &unbacked,
        &unbacked_grant,
        ActionId::new("email.send"),
        OWNER_CHAT_ID,
        None,
        FailureSurface::DirectResponse,
        None,
    )
    .await;
    assert!(matches!(unbacked_result, Err(DispatchError::NoExecutor(_))));
    let unbacked_details = batched_failure_details(&unbacked);
    assert!(unbacked_details
        .iter()
        .any(|detail| detail.contains("email.send: no registered executor")));
}

async fn run_approved_draft_case(
    headless: bool,
    target_digest_override: Option<Digest>,
) -> (Option<Value>, crate::pipeline::AppState) {
    let token_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "test-access-token",
            "expires_in": 3600,
        })))
        .mount(&token_server)
        .await;

    let gmail_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messages": [{
                "payload": {
                    "mimeType": "text/plain",
                    "headers": [{"name": "From", "value": "alice@example.com"}],
                    "body": {"data": "aGk"}
                }
            }]
        })))
        .mount(&gmail_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "draft-1"})))
        .mount(&gmail_server)
        .await;

    let telegram_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/AnswerCallbackQuery"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(&telegram_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": 42, "type": "private"},
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
    .with_urls(format!("{}/token", token_server.uri()), gmail_server.uri());
    let state = test_state_with_gmail_and_telegram(
        gmail,
        TelegramConnector::with_api_url(
            "test-token".to_string(),
            telegram_server.uri().parse().unwrap(),
        ),
    );

    let grant = crate::pipeline::approval_fixture_grant();
    let pending_ref = state.artifacts.put(b"pending approval").unwrap();
    state
        .store
        .insert_task_grant(&grant, &pending_ref, state.owner_user_id)
        .unwrap();
    let payload_ref = state
        .artifacts
        .put(
            serde_json::to_vec(&json!({
                "subject": "Re: invoice",
                "body": "sounds good"
            }))
            .unwrap()
            .as_slice(),
        )
        .unwrap();
    let target_digest = target_digest_override.unwrap_or_else(|| {
        digest_of(&json!({
            "thread_id": "thread-1",
            "connector": "gmail_primary",
            "account_role": "owner_mailbox",
            "recipients": ["alice@example.com"],
        }))
    });
    let mut params = BTreeMap::new();
    if headless {
        params.insert("headless".to_string(), "true".to_string());
    }
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: ActionId::new("email.create_draft"),
        target_ref: Some(TargetRef {
            kind: TargetRefKind::EmailThread,
            id: Some("thread-1".to_string()),
        }),
        payload_ref: Some(payload_ref),
        target_digest: Some(target_digest),
        selection_token_id: None,
        params,
        skill_attribution: None,
        requested_at: Timestamp::now(),
        schema_version: 1,
    };
    state.store.insert_action_request(&request).unwrap();

    let mut update = owner_update("");
    update.chat_id = state.owner_user_id;
    update.sender_user_id = Some(state.owner_user_id);
    update.text = None;
    update.callback_query = Some(CallbackQueryUpdate {
        id: "draft-approval-callback".to_string(),
        data: Some(format!("approve_draft:{}", request.id)),
    });
    handle_owner_update(&state, &update)
        .await
        .expect("owner approval callback must complete");

    let draft = match state
        .store
        .count_audit_events_of_kind("draft.created")
        .unwrap()
    {
        0 => None,
        1 => Some(draft_created_event(&state)),
        count => panic!("unexpected draft.created audit count: {count}"),
    };
    assert_eq!(state.store.count_pending_draft_writes().unwrap(), 0);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("action.dispatch_failed")
            .unwrap(),
        0
    );
    let events = state.store.all_audit_event_jsons().unwrap();
    assert!(
        events.iter().all(|event| !event.contains("stub")),
        "approval flow must not persist the former stub result"
    );
    (draft, state)
}

#[tokio::test]
async fn headless_and_non_headless_approval_converge_on_gmail_executor() {
    let (headless_draft, headless_state) = run_approved_draft_case(true, None).await;
    let headless_draft = headless_draft.expect("headless approval must create a draft");
    assert_eq!(
        headless_state
            .store
            .count_audit_events_of_kind("headless.approved_dispatched")
            .unwrap(),
        1
    );

    let (ordinary_draft, ordinary_state) = run_approved_draft_case(false, None).await;
    let ordinary_draft = ordinary_draft.expect("ordinary approval must create a draft");
    assert_eq!(
        ordinary_state
            .store
            .count_audit_events_of_kind("headless.approved_dispatched")
            .unwrap(),
        0
    );
    assert_eq!(headless_draft["action"], ordinary_draft["action"]);
    assert_eq!(headless_draft["target_refs"], ordinary_draft["target_refs"]);
    assert_eq!(
        headless_draft["payload_refs"],
        ordinary_draft["payload_refs"]
    );
    assert_eq!(headless_draft["action"], "email.create_draft");
}

#[tokio::test]
async fn headless_refusal_appends_no_dispatched_audit() {
    let mismatched_target = digest_of(&json!({
        "thread_id": "thread-1",
        "connector": "gmail_primary",
        "account_role": "owner_mailbox",
        "recipients": ["mallory@example.com"],
    }));
    let (draft, state) = run_approved_draft_case(true, Some(mismatched_target)).await;

    assert!(draft.is_none(), "a target mutation must not create a draft");
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.target_mutated_since_approval")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("headless.approved_dispatched")
            .unwrap(),
        0,
        "only an Executed outcome may produce headless success evidence"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.created")
            .unwrap(),
        0
    );
}

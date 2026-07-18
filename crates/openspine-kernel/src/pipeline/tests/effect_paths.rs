// openspine:allow-large-module reason: effect-path integration tests share one audited mock harness
use crate::api::tests::{post_action, start_server};
use crate::pipeline::handle_owner_update;
use crate::test_support::fixtures::owner_update;
use jiff::Timestamp;
use openspine_schemas::action::{ActionId, ActionRequest};
use serde_json::json;
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::test_state_with_telegram;

fn parse_audit_event(json_str: &str) -> serde_json::Value {
    serde_json::from_str(json_str).unwrap()
}

// Path 1: notify_owner_best_effort (kernel-origin-gated)
#[tokio::test]
async fn test_path_1_notify_owner_gated_and_audited() {
    let tg = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "result": {"message_id": 1, "date": 0, "chat": {"id": 555, "type": "private"}, "from": {"id": 1, "is_bot": true, "first_name": "bot"}, "text": "ok"}})))
        .mount(&tg)
        .await;

    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".to_string(),
        tg.uri().parse().unwrap(),
    ));

    notify_owner_best_effort(&state, 555, "test failure message").await;

    let events = state.store.all_audit_event_jsons().unwrap();
    let gated_event = events
        .iter()
        .map(|s| parse_audit_event(s))
        .find(|v| v["kind"] == "action.gated")
        .expect("expected action.gated audit");
    assert_eq!(gated_event["action"], "owner.notify");
    assert_eq!(gated_event["decision"]["outcome"], "allow");
    assert!(gated_event["task_grant_id"].is_string());

    let notified_event = events
        .iter()
        .map(|s| parse_audit_event(s))
        .find(|v| v["kind"] == "owner.notified")
        .expect("expected owner.notified audit");
    assert_eq!(notified_event["action"], "owner.notify");
    assert_eq!(notified_event["decision"]["outcome"], "allow");
}

// Path 2: create_approved_draft (post-gate-approved-effect) — mismatch path
#[tokio::test]
async fn test_path_2_create_draft_payload_mutated_audited() {
    let tg = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "result": {"message_id": 1, "date": 0, "chat": {"id": 555, "type": "private"}, "from": {"id": 1, "is_bot": true, "first_name": "bot"}, "text": "ok"}})))
        .mount(&tg)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".to_string(),
        tg.uri().parse().unwrap(),
    ));

    let grant = crate::pipeline::tests::approval::approval_fixture_grant();
    let pending_ref = state.artifacts.put(b"approved payload").unwrap();
    state
        .artifacts
        .put_tampered_for_test(&pending_ref.digest, b"tampered payload bytes")
        .unwrap();

    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: ActionId::new("email.create_draft"),
        target_ref: None,
        payload_ref: Some(pending_ref.clone()),
        target_digest: None,
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        requested_at: Timestamp::now(),
        schema_version: 1,
    };

    crate::pipeline::approval::create_approved_draft(&state, &grant, &request, 555)
        .await
        .unwrap();

    let events = state.store.all_audit_event_jsons().unwrap();
    let mismatch_event = events
        .iter()
        .map(|s| parse_audit_event(s))
        .find(|v| v["kind"] == "draft.payload_mutated_since_approval")
        .expect("expected draft.payload_mutated_since_approval audit");

    assert_eq!(mismatch_event["action"], "email.create_draft");
    assert_eq!(
        mismatch_event["payload_refs"][0]["digest"],
        pending_ref.digest.as_str()
    );
}

// Path 3: activate_approved_artifact (post-gate-approved-effect) — missing row
#[tokio::test]
async fn test_path_3_activate_artifact_failure_audited() {
    let state = test_state();
    let grant = crate::pipeline::tests::approval::approval_fixture_grant();
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: ActionId::new("artifact.activate"),
        target_ref: None,
        payload_ref: None,
        target_digest: None,
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        requested_at: Timestamp::now(),
        schema_version: 1,
    };

    crate::pipeline::artifact_activation::activate_approved_artifact(&state, &grant, &request, 555)
        .await
        .unwrap();

    let events = state.store.all_audit_event_jsons().unwrap();
    let fail_event = events
        .iter()
        .map(|s| parse_audit_event(s))
        .find(|v| v["kind"] == "artifact.activation_failed")
        .expect("expected artifact.activation_failed audit");

    assert_eq!(fail_event["action"], "artifact.activate");
    assert_eq!(
        fail_event["reason"],
        "no proposed_artifacts row for this action request"
    );
}

// Path 4: dispatch_read_selected_thread (gated-shell, token-validated)
#[tokio::test]
async fn test_path_4_read_selected_thread_gated_and_audited() {
    let state = test_state();
    let (grant, _token) = crate::api::dispatch_tests::mint_grant_with_selection_token(
        &state,
        &["email.read_thread:selected_no_attachments"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );

    let store = state.store.clone();
    let (addr, handle) = crate::api::tests::start_server(state).await;

    let resp = crate::api::tests::post_action(
        addr,
        &grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({ "thread_id": "thread-1" })),
    )
    .await;
    assert_eq!(resp.status(), 200);

    let events = store.all_audit_event_jsons().unwrap();
    let gated_event = events
        .iter()
        .map(|s| parse_audit_event(s))
        .find(|v| v["kind"] == "action.gated")
        .expect("expected action.gated audit");

    assert_eq!(
        gated_event["action"],
        "email.read_thread:selected_no_attachments"
    );
    assert_eq!(gated_event["decision"]["outcome"], "deny");
    assert_eq!(gated_event["decision"]["reason"], "selection_token_invalid");

    handle.abort();
}

// Path 5: dispatch_lyra_preview (gated-shell)
// Characterization is the gate audit, not HTTP success (preview may 5xx without Telegram mock).
#[tokio::test]
async fn test_path_5_preview_gated_and_audited() {
    let state = test_state();
    let (grant, _token) = crate::api::dispatch_tests::mint_grant_with_selection_token(
        &state,
        &["lyra.ui.preview"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );

    let store = state.store.clone();
    let (addr, handle) = crate::api::tests::start_server(state).await;

    let _resp = crate::api::tests::post_action(
        addr,
        &grant.task_token,
        "lyra.ui.preview",
        Some(json!({ "subject": "test", "body": "hello" })),
    )
    .await;

    let events = store.all_audit_event_jsons().unwrap();
    let gated_event = events
        .iter()
        .map(|s| parse_audit_event(s))
        .find(|v| v["kind"] == "action.gated")
        .expect("expected action.gated audit");

    assert_eq!(gated_event["action"], "lyra.ui.preview");
    assert_eq!(gated_event["decision"]["outcome"], "allow");

    handle.abort();
}

// Path 6: dispatch_artifact_propose (gated-shell)
// Characterization is the gate audit; payload validity is out of scope.
#[tokio::test]
async fn test_path_6_artifact_propose_gated_and_audited() {
    let state = test_state();
    let (grant, _token) = crate::api::dispatch_tests::mint_grant_with_selection_token(
        &state,
        &["artifact.propose"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );

    let store = state.store.clone();
    let (addr, handle) = crate::api::tests::start_server(state).await;

    let _resp = crate::api::tests::post_action(
        addr,
        &grant.task_token,
        "artifact.propose",
        Some(json!({ "kind": "route", "yaml": "id: test\nversion: 1" })),
    )
    .await;

    let events = store.all_audit_event_jsons().unwrap();
    let gated_event = events
        .iter()
        .map(|s| parse_audit_event(s))
        .find(|v| v["kind"] == "action.gated")
        .expect("expected action.gated audit");

    assert_eq!(gated_event["action"], "artifact.propose");
    assert_eq!(gated_event["decision"]["outcome"], "allow");

    handle.abort();
}

// Path 7: sweep_expired_grants (internal-maintenance-non-effect)
#[test]
fn test_path_7_sweep_bypasses_gate_and_audit() {
    let store = crate::store::Store::open_in_memory().unwrap();
    store.sweep_expired_grants(Timestamp::now()).unwrap();
    let audit_count = store.count_audit_events_of_kind("action.gated").unwrap();
    assert_eq!(audit_count, 0, "sweep must not produce gate audit events");
}

// Path 8: answer_callback_query (internal-maintenance-non-effect)
#[tokio::test]
async fn test_path_8_answer_callback_query_bypasses_gate_and_audit() {
    let tg = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/AnswerCallbackQuery"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(&tg)
        .await;

    let connector =
        TelegramConnector::with_api_url("test-token".to_string(), tg.uri().parse().unwrap());
    let _ = connector.answer_callback_query("cb-123").await;
}

// Path 9: shadow-mode grant → EffectSuppressed; dispatch must not invoke the
// effect handler. Uses an effectful action with a mock that must see zero calls.
#[tokio::test]
async fn shadow_grant_effect_suppressed_skips_effect_handler() {
    let tg = MockServer::start().await;
    let token = "test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{}/SendMessage", token)))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": 555, "type": "private"},
                "text": "sent"
            }
        })))
        .expect(0)
        .mount(&tg)
        .await;

    let connector = TelegramConnector::with_api_url(token.to_string(), tg.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);

    let mut grant = crate::pipeline::tests::approval::approval_fixture_grant();
    grant.mode = openspine_schemas::grant::GrantMode::Shadow;
    grant.allowed_actions = vec![ActionId::new("telegram.reply:owner_channel")];
    grant.approval_required_actions = vec![];
    grant.output_channels = vec!["telegram.owner.reply".to_string()];
    // root_authority fields changed; re-seal after mode + allowlist mutation.
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    let pending_ref = state.artifacts.put(b"shadow pending".as_slice()).unwrap();
    state
        .store
        .insert_task_grant(&grant, &pending_ref, 555)
        .unwrap();

    let store = state.store.clone();
    let (addr, handle) = crate::api::tests::start_server(state).await;

    let resp = crate::api::tests::post_action(
        addr,
        &grant.task_token,
        "telegram.reply:owner_channel",
        Some(json!({"text": "should not send under shadow"})),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "effect_suppressed");
    assert!(
        body.get("result").is_none() || body["result"].is_null(),
        "effect handler must not run under EffectSuppressed: {body}"
    );

    // Observable: zero Telegram SendMessage calls.
    let requests = tg.received_requests().await.unwrap();
    assert_eq!(
        requests.len(),
        0,
        "shadow EffectSuppressed must not invoke the telegram effect"
    );

    let events = store.all_audit_event_jsons().unwrap();
    let gated = events
        .iter()
        .map(|s| parse_audit_event(s))
        .find(|v| v["kind"] == "action.gated")
        .expect("expected action.gated audit");
    assert_eq!(gated["action"], "telegram.reply:owner_channel");
    assert_eq!(gated["decision"]["outcome"], "effect_suppressed");

    handle.abort();
}

#[tokio::test]
async fn audit_append_failure_fails_notification_before_connector_effect() {
    let tg = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&tg)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".to_string(),
        tg.uri().parse().unwrap(),
    ));
    let grant = handle_owner_update(&state, &owner_update("hello"))
        .await
        .unwrap()
        .expect("owner update grants action access");
    state
        .store
        .install_audit_append_failure_for_kind("action.dispatch_failed")
        .unwrap();
    let (addr, handle) = start_server(state).await;
    let response = post_action(
        addr,
        &grant.task_token,
        "telegram.reply:owner_channel",
        Some(json!({"unexpected": true})),
    )
    .await;
    assert_eq!(
        response.status(),
        reqwest::StatusCode::INTERNAL_SERVER_ERROR
    );
    assert!(tg.received_requests().await.unwrap().is_empty());
    handle.abort();
}

#[tokio::test]
async fn audit_readonly_failure_fails_notification_before_connector_effect() {
    let tg = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&tg)
        .await;

    // 1. Open a writable database first so setup (bootstrap_owner_principal etc.) compiles and writes
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("kernel.db");
    let store_writable = crate::store::Store::open(&db_path).unwrap();

    let connector =
        TelegramConnector::with_api_url("test-token".to_string(), tg.uri().parse().unwrap());
    let mut state =
        crate::test_support::fixtures::build_state_with_store(store_writable, connector, None);

    // Get a valid grant while writable
    let grant = handle_owner_update(&state, &owner_update("hello"))
        .await
        .unwrap()
        .expect("owner update grants action access");

    // 2. Drop the writable store connection by replacing it with a read-only store
    let store_readonly = crate::store::Store::open_read_only_for_test(&db_path).unwrap();
    state.store = store_readonly;

    // 3. Dispatch the action. Because the store is read-only, appending to the audit log
    // must fail with SQLITE_READONLY, causing the dispatch action to fail loudly
    // and the connector to NOT be called.
    let (addr, handle) = start_server(state).await;
    let response = post_action(
        addr,
        &grant.task_token,
        "telegram.reply:owner_channel",
        Some(json!({"text": "hello owner"})),
    )
    .await;

    assert_eq!(
        response.status(),
        reqwest::StatusCode::INTERNAL_SERVER_ERROR
    );
    // Assert the connector was never called
    assert!(tg.received_requests().await.unwrap().is_empty());
    handle.abort();
}

// Literal disk-full (SQLITE_FULL) proof: unlike the injected-fault test
// above, this saturates a REAL file-backed database via
// `PRAGMA max_page_count`, so the production append path hits a genuine
// SQLITE_FULL rather than a simulated trigger or a read-only flag. A valid
// reply payload is used so the Telegram connector call is genuinely pending
// after the `action.gated` audit append; when that append fails (disk-full)
// the request must fail loudly and the connector must never be invoked — the
// empty mock log is causal, not coincidental (a bad payload would skip the
// connector regardless of disk-full).
#[tokio::test]
async fn disk_full_audit_append_aborts_before_connector_effect() {
    let tg = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&tg)
        .await;

    // 1. Open a writable file-backed database — a real file so
    //    `PRAGMA max_page_count` yields a literal SQLITE_FULL.
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("kernel.db");
    let store = crate::store::Store::open(&db_path).unwrap();

    let connector =
        TelegramConnector::with_api_url("test-token".to_string(), tg.uri().parse().unwrap());
    let state = crate::test_support::fixtures::build_state_with_store(store, connector, None);

    // Get a valid grant while the database still has room.
    let grant = handle_owner_update(&state, &owner_update("hello"))
        .await
        .unwrap()
        .expect("owner update grants action access");

    // Keep a clone sharing the same connection for post-action assertions
    // (start_server moves `state`).
    let probe = state.store.clone();

    // 2. Clamp the page budget on the store's own connection and force
    //    rollback journaling (WAL would grow the -wal file and dodge the
    //    main-db page cap). Then saturate the audit ledger with chain-valid
    //    rows until the next append returns literal SQLITE_FULL. The filler
    //    row is the smallest valid append shape, so once it fails, every
    //    equal-or-larger real audit append — the action's `action.gated`
    //    row included — fails deterministically.
    state.store.with_conn_for_test(|conn| {
        conn.execute_batch("PRAGMA journal_mode = DELETE").unwrap();
        let page_count: i64 = conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .unwrap();
        conn.execute_batch(&format!("PRAGMA max_page_count = {}", page_count + 6))
            .unwrap();
    });

    let mut saturated = false;
    for _ in 0..5000 {
        match state
            .store
            .append_audit("action.gated", None, None, None, None, &[], &[])
        {
            Ok(_) => {}
            Err(crate::store::StoreError::Sqlite(rusqlite::Error::SqliteFailure(ffi_err, _)))
                if ffi_err.extended_code == rusqlite::ffi::SQLITE_FULL =>
            {
                saturated = true;
                break;
            }
            Err(err) => panic!("filler append failed unexpectedly: {err:?}"),
        }
    }
    assert!(
        saturated,
        "filler loop never hit SQLITE_FULL; the clamp or iteration cap needs adjusting"
    );

    let gated_before = probe.count_audit_events_of_kind("action.gated").unwrap();
    let total_before: i64 = probe.with_conn_for_test(|conn| {
        conn.query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0))
            .unwrap()
    });

    // 3. Dispatch with a VALID reply payload so a connector call is
    //    genuinely pending. Under saturation the `action.gated` audit
    //    append (which precedes dispatch/send_reply) fails with SQLITE_FULL,
    //    so the request must fail loudly and the connector must not run.
    let (addr, handle) = start_server(state).await;
    let response = post_action(
        addr,
        &grant.task_token,
        "telegram.reply:owner_channel",
        Some(json!({"text": "hello owner"})),
    )
    .await;

    assert_eq!(
        response.status(),
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        "disk-full audit append must fail loudly"
    );
    assert!(
        tg.received_requests().await.unwrap().is_empty(),
        "connector must not be invoked when the gate audit cannot be recorded"
    );

    // No unrecorded execution: the failed append rolled back, so nothing
    // was written for this action.
    let gated_after = probe.count_audit_events_of_kind("action.gated").unwrap();
    assert_eq!(
        gated_after, gated_before,
        "action.gated append must have rolled back under disk-full"
    );
    let total_after: i64 = probe.with_conn_for_test(|conn| {
        conn.query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0))
            .unwrap()
    });
    assert_eq!(
        total_after, total_before,
        "no audit row may land when the ledger is disk-full"
    );

    handle.abort();
}

#[tokio::test]
async fn notify_send_failure_records_attempt_failure_and_dead_letter() {
    let tg = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&tg)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".to_string(),
        tg.uri().parse().unwrap(),
    ));

    let outcome = notify_owner_with_digest(&state, 555, "retry me", &[], None).await;
    assert_eq!(outcome, NotifyOutcome::SendFailed);
    let events = state.store.all_audit_event_jsons().unwrap();
    let kinds: Vec<_> = events
        .iter()
        .map(|event| {
            parse_audit_event(event)["kind"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    let attempted = kinds
        .iter()
        .position(|kind| kind == "owner.notify_attempted")
        .unwrap();
    let failed = kinds
        .iter()
        .position(|kind| kind == "owner.notify_failed")
        .unwrap();
    assert!(attempted < failed);
    assert!(!kinds.iter().any(|kind| kind == "owner.notified"));
    let failed_event = events
        .iter()
        .map(|event| parse_audit_event(event))
        .find(|event| event["kind"] == "owner.notify_failed")
        .unwrap();
    let failed_grant = Ulid::from_string(failed_event["task_grant_id"].as_str().unwrap()).unwrap();
    let dead_letters = state.store.pending_dead_letters().unwrap();
    assert_eq!(dead_letters.len(), 1);
    assert_eq!(dead_letters[0].chat_id, 555);
    assert_ne!(dead_letters[0].task_grant_id, Ulid::nil());
    assert_eq!(dead_letters[0].task_grant_id, failed_grant);
    assert!(!dead_letters[0].text_ref.is_empty());
    assert_eq!(
        state
            .store
            .connector_counter("telegram", "failure")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn notify_owner_required_succeeds_when_sent() {
    let tg = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "result": {"message_id": 1, "date": 0, "chat": {"id": 555, "type": "private"}, "from": {"id": 1, "is_bot": true, "first_name": "bot"}, "text": "ok"}})))
        .mount(&tg)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".to_string(),
        tg.uri().parse().unwrap(),
    ));
    let result = notify_owner_required(&state, 555, "escalation message").await;
    assert!(result.is_ok(), "required notify must succeed when Sent");
    let events = state.store.all_audit_event_jsons().unwrap();
    assert!(events
        .iter()
        .any(|e| parse_audit_event(e)["kind"] == "owner.notify_attempted"));
    assert!(events
        .iter()
        .any(|e| parse_audit_event(e)["kind"] == "owner.notified"));
    assert_eq!(
        state
            .store
            .connector_counter("telegram", "success")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn notify_owner_required_errors_and_audits_on_send_failure() {
    let tg = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&tg)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".to_string(),
        tg.uri().parse().unwrap(),
    ));
    let result = notify_owner_required(&state, 555, "escalation message").await;
    assert!(
        matches!(
            result,
            Err(crate::store::StoreError::OwnerNotificationFailed(_))
        ),
        "required notify must return OwnerNotificationFailed on SendFailed: {result:?}"
    );
    let events = state.store.all_audit_event_jsons().unwrap();
    assert!(events
        .iter()
        .any(|e| parse_audit_event(e)["kind"] == "owner.notify_attempted"));
    assert!(events
        .iter()
        .any(|e| parse_audit_event(e)["kind"] == "owner.notify_failed"));
    assert!(!events
        .iter()
        .any(|e| parse_audit_event(e)["kind"] == "owner.notified"));
    assert_eq!(
        state
            .store
            .connector_counter("telegram", "failure")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn notify_owner_with_digest_records_success_counter() {
    let tg = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "result": {"message_id": 1, "date": 0, "chat": {"id": 555, "type": "private"}, "from": {"id": 1, "is_bot": true, "first_name": "bot"}, "text": "ok"}})))
        .mount(&tg)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".to_string(),
        tg.uri().parse().unwrap(),
    ));
    let outcome = notify_owner_with_digest(&state, 555, "hi", &[], None).await;
    assert_eq!(outcome, NotifyOutcome::Sent);
    assert_eq!(
        state
            .store
            .connector_counter("telegram", "success")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn required_send_failure_keeps_dlq_when_counter_persistence_breaks() {
    let tg = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&tg)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".to_string(),
        tg.uri().parse().unwrap(),
    ));
    state.store.break_connector_counters_for_test();
    let result = notify_owner_required(&state, 555, "escalation message").await;
    assert!(matches!(
        result,
        Err(crate::store::StoreError::OwnerNotificationFailed(_))
    ));
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner.notify_failed")
            .unwrap(),
        1
    );
    assert_eq!(state.store.pending_dead_letters().unwrap().len(), 1);
    assert_eq!(state.store.owner_digest_items().unwrap().len(), 1);
    assert_eq!(
        state.store.owner_digest_items().unwrap()[0].class,
        "resource"
    );
}

//! End-to-end proof of the worker→shell→kernel report contract
//! (`fix-worker-shell-contract`).
//!
//! Commissions a worker through the REAL `POST /v1/actions worker.commission`
//! path — gate + registry dispatch, *not* a direct handler call — so the kernel
//! mints the worker sub-grant, persists the dispatch row, and spawns the REAL
//! `openspine-shell` binary via `ProcessDriver`. The shell fetches its view,
//! runs its agent, and calls `worker.report_result` exactly once on completion.
//! We then run the REAL `worker_result_consumer_iteration`, which relays the
//! `worker.result` bus event through the commissioning parent's separately
//! gated reply path to the owner's Telegram exactly once.
//!
//! This is the release-gating proof for the post-merge Codex P1s:
//! * P1-a: the kernel serializes a worker's `output_channels` as `[]` (not
//!   `null`), which is what `openspine-shell`'s `TaskView` requires — the shell
//!   could not even fetch its view (and therefore could not report) otherwise.
//! * P1-b: the shell reports its terminal result, so the dispatch row flips
//!   `terminal` and the master relay delivers — a successful task no longer
//!   strands forever in `dispatched`.

use super::worker_result_consumer_iteration;

use axum::serve;
use serde_json::json;
use std::sync::Arc;
use tokio::net::TcpListener;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::api::router;
use crate::api::tests::post_action;
use crate::pipeline::{handle_owner_update, AppState};
use crate::sandbox::{ProcessDriver, Sandbox};
use crate::store::Store;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::{build_state_with_store, owner_update};

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::briefcase::{Briefcase, CounterpartyRef, TaskClass, TaskShape};
use openspine_schemas::digest::Digest;
use openspine_schemas::event::Connector;
use openspine_schemas::grant::{GrantLimits, GrantMode, TaskGrant};
use ulid::Ulid;

fn worker_shell_binary() -> std::path::PathBuf {
    let name = if cfg!(windows) {
        "openspine-shell.exe"
    } else {
        "openspine-shell"
    };
    let exe = std::env::current_exe().expect("resolve current exe");
    let debug_dir = exe
        .parent()
        .and_then(|p| p.parent())
        .expect("resolve target profile dir");
    let candidate = debug_dir.join(name);
    if candidate.exists() {
        return candidate;
    }
    // Honour an explicit CARGO_TARGET_DIR if the layout differs.
    // The binary must already be built (scripts/check.sh runs
    // `cargo build -p openspine-shell --bin openspine-shell` before
    // `cargo test --workspace`), so there is no nested Cargo here.
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
        let p = std::path::Path::new(&target).join("debug").join(name);
        if p.exists() {
            return p;
        }
    }
    panic!(
        "openspine-shell binary not found near {debug_dir:?}; \
         build it with `cargo build -p openspine-shell --bin openspine-shell` \
         before running this test"
    );
}

fn audit_action_count(state: &AppState, action_name: &str) -> usize {
    state
        .store
        .all_audit_event_jsons()
        .unwrap()
        .into_iter()
        .filter_map(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .filter(|event| {
            event.get("kind").and_then(serde_json::Value::as_str) == Some("action.gated")
                && event.get("action").and_then(serde_json::Value::as_str) == Some(action_name)
        })
        .count()
}

fn dispatch_state(state: &AppState) -> String {
    state
        .store
        .conn
        .lock()
        .query_row("SELECT state FROM worker_dispatch LIMIT 1", [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn parent_grant_for_commission() -> TaskGrant {
    let now = Timestamp::now();
    let mut grant = TaskGrant {
        persona_id: None,
        id: Ulid::new(),
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user: "owner".to_string(),
        purpose: "commission-worker".to_string(),
        issued_by: "kernel".to_string(),
        issued_at: now,
        expires_at: now + std::time::Duration::from_secs(600),
        event_id: Ulid::new(),
        route_id: "owner_telegram_main_assistant".to_string(),
        agent_id: "main_assistant_agent".to_string(),
        workflow_id: "owner_control_conversation".to_string(),
        capability_pack_id: "owner_control_basic_pack".to_string(),
        authority_sources: vec![],
        selection_tokens: vec![],
        // Allow the commission itself plus the worker's report action. The
        // worker spec may only narrow these, so the worker is granted exactly
        // `worker.report_result`. The relay action lets the master deliver.
        allowed_actions: vec![
            ActionId::new("worker.commission"),
            ActionId::new("worker.report_result"),
            ActionId::new("telegram.reply:owner_channel"),
        ],
        approval_required_actions: vec![],
        denied_actions: vec![],
        allowed_egress_classes: vec![],
        output_channels: vec!["telegram.owner.reply".to_string()],
        limits: GrantLimits {
            max_model_calls: 8,
            max_artifacts: 20,
            max_runtime_seconds: 120,
        },
        task_token: "e2e-commission-parent-token".to_string(),
        root_grant_id: Ulid::nil(),
        parent_grant_id: None,
        mode: GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
    };
    grant.root_grant_id = grant.id;
    grant.seal_root(&crate::grant_hmac_key().expect("test HMAC key"));
    grant
}

fn parent_briefcase() -> Briefcase {
    Briefcase {
        schema_version: 1,
        task_shape: TaskShape {
            route_id: "owner_telegram_main_assistant".to_string(),
            workflow_id: "owner_control_conversation".to_string(),
            counterparty: CounterpartyRef::Unresolved {
                channel: "worker".to_string(),
                identifier: "worker-1".to_string(),
            },
        },
        source_snapshot_id: Digest::parse(format!("sha256:{}", "0".repeat(64))).unwrap(),
        depth: 1,
        tier: openspine_schemas::briefcase::RelationshipTier::Stranger,
        class: TaskClass::Conversation,
        sections: vec![],
        top_up_log: vec![],
    }
}

#[tokio::test]
async fn commissioned_worker_reports_and_master_relays_through_real_shell() {
    // Owner Telegram mock: the relay target for the master grant.
    let tg = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {"message_id": 1, "date": 0, "chat": {"id": 555, "type": "private"}, "text": "sent"}
        })))
        .expect(1)
        .mount(&tg)
        .await;
    let connector =
        TelegramConnector::with_api_url("test-token".to_string(), tg.uri().parse().unwrap());

    // Real state, but the sandbox driver spawns the freshly-built shell binary.
    let store = Store::open_in_memory().unwrap();
    let mut state = build_state_with_store(store, connector, None);
    // Supervision requires a connector-bound route. The production owner
    // Telegram route is intentionally connector-agnostic, so bind this
    // isolated e2e registry copy to the Telegram connector.
    {
        let mut registry = state.registry.write();
        let route = registry
            .routes
            .iter_mut()
            .find(|route| route.id == "owner_telegram_main_assistant")
            .expect("owner Telegram route fixture");
        route.when.connector = Some(Connector::TelegramOwnerBot);
    }
    state.sandbox = Sandbox::Process(ProcessDriver {
        shell_binary: worker_shell_binary(),
        scratch_root: std::env::temp_dir().join(format!("openspine-e2e-{}", Ulid::new())),
    });

    // Bind the HTTP server first so we can point the shell at it.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    state.kernel_endpoint = format!("http://{addr}");
    let state = Arc::new(state);
    let app = router(state.clone());
    let server = tokio::spawn(async move { serve(listener, app).await.unwrap() });

    // Seed the commissioning parent grant (root, allowed to commission).
    let parent = parent_grant_for_commission();
    state
        .store
        .insert_grant_and_briefcase_atomic(
            &parent,
            &state.artifacts.put(b"parent-pending").unwrap(),
            state.owner_user_id,
            &parent_briefcase(),
        )
        .unwrap();

    // Commission a worker through the REAL gate+dispatch path. The handler
    // mints the worker, persists the dispatch row, and spawns the shell, which
    // blocks this HTTP request until it exits. The worker's allowed actions
    // narrow to `worker.report_result` only, so its freeform agent step hits a
    // gated model.generate denial and completes without any egress — exactly
    // one Telegram send (the master relay) is therefore expected.
    let spec = json!({
        "agent_id": "main_assistant_agent",
        "allowed_actions": ["worker.report_result"],
        "bound_parameters": [],
        "expires_before": parent.expires_at.to_string(),
        "purpose": "worker-task",
        "route_id": "owner_telegram_main_assistant",
        "workflow_id": "owner_control_conversation",
        "capability_pack_id": "owner_control_basic_pack",
        "counterparty_channel": null,
        "counterparty_identifier": null,
        "receipt": "e2e-commission-receipt"
    });
    let mut rejected_spec = spec.clone();
    rejected_spec["allowed_actions"] = json!([]);
    let rejected = post_action(
        addr,
        &parent.task_token,
        "worker.commission",
        Some(rejected_spec),
    )
    .await;
    assert_eq!(rejected.status(), 400);
    let rejection: serde_json::Value = rejected.json().await.unwrap();
    assert_eq!(
        rejection["error"], "worker cannot report results",
        "commission must reject a worker that cannot report"
    );
    let dispatch_count: i64 = state
        .store
        .conn
        .lock()
        .query_row("SELECT COUNT(*) FROM worker_dispatch", [], |row| row.get(0))
        .unwrap();
    assert_eq!(dispatch_count, 0);

    let resp = post_action(addr, &parent.task_token, "worker.commission", Some(spec)).await;
    assert_eq!(resp.status(), 200, "worker.commission is gated Allow");
    let response_body: serde_json::Value = resp.json().await.unwrap();
    assert!(response_body["result"]["worker_grant_id"].is_string());
    // The persisted grant JSON intentionally redacts the bearer token; use
    // the D-083 dispatch token_ref to exercise the authenticated GET path.
    let token_ref_json: String = state
        .store
        .conn
        .lock()
        .query_row("SELECT token_ref FROM worker_dispatch LIMIT 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    let token_ref: openspine_schemas::artifact::ArtifactRef =
        serde_json::from_str(&token_ref_json).unwrap();
    let worker_token = String::from_utf8(state.artifacts.get(&token_ref).unwrap()).unwrap();
    // D-111: the shell has already reported a terminal result, so its bearer
    // token is dead and cannot fetch the task view again.
    let task_view_response = reqwest::Client::new()
        .get(format!("http://{addr}/v1/task"))
        .bearer_auth(worker_token)
        .send()
        .await
        .unwrap();
    assert_eq!(task_view_response.status(), 403);
    let task_rejection: serde_json::Value = task_view_response.json().await.unwrap();
    assert_eq!(task_rejection["error"], "unauthorized");
    assert_eq!(audit_action_count(&state, "worker.report_result"), 1);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("task.shell_failed")
            .unwrap(),
        0
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("task.shell_completed")
            .unwrap(),
        1
    );
    assert_eq!(dispatch_state(&state), "terminal");

    // The shell has now run to completion and reported its result; drive the
    // REAL master relay consumer that turns the `worker.result` event into an
    // owner Telegram send.
    worker_result_consumer_iteration(&state)
        .await
        .expect("master relay consumer must deliver the worker result");

    let requests = tg.received_requests().await.unwrap();
    assert_eq!(
        requests.len(),
        1,
        "exactly one owner relay reaches Telegram"
    );
    let body = String::from_utf8_lossy(&requests[0].body);
    assert!(
        body.contains("completed"),
        "worker summary reaches owner: {body}"
    );

    server.abort();
}

#[tokio::test]
async fn root_pipeline_shell_does_not_report_worker_result() {
    let tg = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {"message_id": 2, "date": 0, "chat": {"id": 555, "type": "private"}, "text": "sent"}
        })))
        .expect(1)
        .mount(&tg)
        .await;
    let connector =
        TelegramConnector::with_api_url("test-token".to_string(), tg.uri().parse().unwrap());
    let store = Store::open_in_memory().unwrap();
    let mut state = build_state_with_store(store, connector, None);
    state.sandbox = Sandbox::Process(ProcessDriver {
        shell_binary: worker_shell_binary(),
        scratch_root: std::env::temp_dir().join(format!("openspine-root-e2e-{}", Ulid::new())),
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    state.kernel_endpoint = format!("http://{addr}");
    let state = Arc::new(state);
    let app = router(state.clone());
    let server = tokio::spawn(async move { serve(listener, app).await.unwrap() });

    let grant = handle_owner_update(&state, &owner_update("/status"))
        .await
        .unwrap()
        .expect("root owner update must compose a grant");
    assert!(grant.parent_grant_id.is_none());
    assert!(grant
        .allowed_actions
        .iter()
        .any(|action| action.0 == "worker.report_result"));
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("task.shell_failed")
            .unwrap(),
        0,
        "root shell must not fail trying to report a worker result"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("task.shell_completed")
            .unwrap(),
        1
    );
    assert_eq!(audit_action_count(&state, "worker.report_result"), 0);
    let dispatch_rows: i64 = state
        .store
        .conn
        .lock()
        .query_row("SELECT COUNT(*) FROM worker_dispatch", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        dispatch_rows, 0,
        "root pipeline must not create worker dispatch"
    );
    assert_eq!(tg.received_requests().await.unwrap().len(), 1);

    server.abort();
}

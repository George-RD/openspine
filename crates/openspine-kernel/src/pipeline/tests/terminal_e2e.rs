use axum::serve;
use std::sync::Arc;
use tokio::net::TcpListener;
use ulid::Ulid;

use crate::api::router;
use crate::pipeline::{handle_terminal_message, AppState};
use crate::sandbox::{ProcessDriver, Sandbox};
use crate::store::Store;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::build_state_with_store;

fn shell_binary() -> std::path::PathBuf {
    let name = if cfg!(windows) {
        "openspine-shell.exe"
    } else {
        "openspine-shell"
    };
    let exe = std::env::current_exe().expect("resolve current test executable");
    let debug_dir = exe
        .parent()
        .and_then(|path| path.parent())
        .expect("resolve target profile directory");
    let candidate = debug_dir.join(name);
    if candidate.exists() {
        return candidate;
    }
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
        let candidate = std::path::Path::new(&target).join("debug").join(name);
        if candidate.exists() {
            return candidate;
        }
    }
    panic!(
        "openspine-shell binary not found near {debug_dir:?}; build it before running this test"
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

#[tokio::test]
async fn terminal_owner_status_reaches_device_through_real_shell() {
    let store = Store::open_in_memory().unwrap();
    let mut state = build_state_with_store(
        store,
        TelegramConnector::new("unused-terminal-e2e-token".to_string()),
        None,
    );
    let (reply_tx, mut reply_rx) = tokio::sync::mpsc::unbounded_channel();
    state.terminal_reply_tx = Some(reply_tx);
    state.sandbox = Sandbox::Process(ProcessDriver {
        shell_binary: shell_binary(),
        scratch_root: std::env::temp_dir().join(format!("openspine-terminal-e2e-{}", Ulid::new())),
    });

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    state.kernel_endpoint = format!("http://{addr}");
    let state = Arc::new(state);
    let app = router(state.clone());
    let server = tokio::spawn(async move { serve(listener, app).await.unwrap() });

    let grant = handle_terminal_message(&state, "/status".to_string())
        .await
        .unwrap()
        .expect("terminal owner request must compose and run a grant");

    assert_eq!(grant.agent_id, "main_terminal_assistant_agent");
    assert_eq!(grant.workflow_id, "owner_terminal_conversation");
    assert_eq!(grant.route_id, "owner_cli_main_assistant");
    assert_eq!(grant.capability_pack_id, "owner_terminal_basic_pack");

    let reply = tokio::time::timeout(std::time::Duration::from_secs(2), reply_rx.recv())
        .await
        .expect("terminal reply must arrive promptly")
        .expect("terminal reply channel must stay open");
    assert!(
        reply.contains("\"status\": \"ok\""),
        "status result must reach the owner device: {reply}"
    );
    assert_eq!(audit_action_count(&state, "openspine.status.read"), 1);
    assert_eq!(audit_action_count(&state, "terminal.reply:owner_device"), 1);
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
    assert!(state.store.verify_audit_chain().unwrap());

    server.abort();
}

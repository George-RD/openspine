use axum::serve;
use std::sync::Arc;
use tokio::net::TcpListener;
use ulid::Ulid;

use crate::api::router;
use crate::pipeline::{handle_terminal_message, run_terminal_turn, AppState};
use crate::sandbox::{ProcessDriver, Sandbox};
use crate::store::Store;
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
        crate::test_support::fixtures::offline_telegram(),
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

/// A worker that logs its progress, dies, and never replies — the shape of the
/// Docker glibc failure the readiness audit hit.
#[cfg(unix)]
fn failing_worker(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let script = dir.join("dying-shell.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\necho '[openspine-shell] INFO: starting' >&2\n\
         echo 'worker image: GLIBC_2.39 not found' >&2\nexit 3\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

/// A worker that dies must end the turn with its cause instead of leaving the
/// chat waiting for a reply that cannot arrive, and the next turn must work.
#[cfg(unix)]
#[tokio::test]
async fn terminal_turn_reports_worker_failure_and_chat_stays_usable() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let mut state = build_state_with_store(
        store,
        crate::test_support::fixtures::offline_telegram(),
        None,
    );
    let (reply_tx, mut reply_rx) = tokio::sync::mpsc::unbounded_channel();
    state.terminal_reply_tx = Some(reply_tx);
    state.sandbox = Sandbox::Process(ProcessDriver {
        shell_binary: failing_worker(tmp.path()),
        scratch_root: tmp.path().join("scratch"),
    });

    // Bounded far below the old 150s wait: failure must surface immediately.
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(4),
        run_terminal_turn(&state, &mut reply_rx, "hello".to_string()),
    )
    .await
    .expect("a dead worker must not leave the turn waiting")
    .expect_err("a dead worker must fail the turn");
    let message = format!("{error:#}");
    assert!(message.contains("worker failed"), "{message}");
    assert!(message.contains("exit status: 3"), "{message}");
    assert!(message.contains("GLIBC_2.39 not found"), "{message}");
    assert!(
        !message.contains("[openspine-shell]"),
        "progress lines must not reach the conversation: {message}"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("task.shell_failed")
            .unwrap(),
        1
    );

    // The same chat then serves a healthy turn through the real shell.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    state.kernel_endpoint = format!("http://{}", listener.local_addr().unwrap());
    state.sandbox = Sandbox::Process(ProcessDriver {
        shell_binary: shell_binary(),
        scratch_root: tmp.path().join("scratch"),
    });
    let state = Arc::new(state);
    let server = tokio::spawn({
        let app = router(state.clone());
        async move { serve(listener, app).await.unwrap() }
    });
    let reply = run_terminal_turn(&state, &mut reply_rx, "/status".to_string())
        .await
        .expect("chat must stay usable after a worker failure");
    assert!(reply.contains("\"status\": \"ok\""), "{reply}");
    server.abort();
}

/// A worker that exits cleanly without replying has nothing left to send; the
/// turn must say so rather than wait out a model-latency budget.
#[cfg(unix)]
#[tokio::test]
async fn terminal_turn_reports_worker_that_exits_without_replying() {
    use std::os::unix::fs::PermissionsExt as _;
    let tmp = tempfile::tempdir().unwrap();
    let script = tmp.path().join("silent-shell.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut state = build_state_with_store(
        Store::open_in_memory().unwrap(),
        crate::test_support::fixtures::offline_telegram(),
        None,
    );
    let (reply_tx, mut reply_rx) = tokio::sync::mpsc::unbounded_channel();
    state.terminal_reply_tx = Some(reply_tx);
    state.sandbox = Sandbox::Process(ProcessDriver {
        shell_binary: script,
        scratch_root: tmp.path().join("scratch"),
    });

    let error = run_terminal_turn(&state, &mut reply_rx, "hello".to_string())
        .await
        .expect_err("a silent worker must fail the turn");
    assert!(
        format!("{error:#}").contains("finished without sending a reply"),
        "{error:#}"
    );
}

/// A worker that already delivered its reply and then died still answered the
/// owner; the reply must not be discarded in favour of the failure.
#[cfg(unix)]
#[tokio::test]
async fn terminal_turn_delivers_a_reply_queued_before_the_worker_died() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = build_state_with_store(
        Store::open_in_memory().unwrap(),
        crate::test_support::fixtures::offline_telegram(),
        None,
    );
    let (reply_tx, mut reply_rx) = tokio::sync::mpsc::unbounded_channel();
    reply_tx.send("already answered".to_string()).unwrap();
    state.terminal_reply_tx = Some(reply_tx);
    state.sandbox = Sandbox::Process(ProcessDriver {
        shell_binary: failing_worker(tmp.path()),
        scratch_root: tmp.path().join("scratch"),
    });

    let reply = run_terminal_turn(&state, &mut reply_rx, "hello".to_string())
        .await
        .expect("a queued reply must be delivered");
    assert_eq!(reply, "already answered");
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("task.shell_failed")
            .unwrap(),
        1
    );
}

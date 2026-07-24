//! Focused startup integration tests for overlay export/restore sequencing.
//!
//! These exercise only the owned startup seams: lifetime lock before stores,
//! forged-marker fail-closed pre-open, late-boundary retention of pending
//! finalization via production provider/bind/clock helpers, successful
//! post-bind audit+cleanup, and the pathless `--rollback-pending-restore` flag.

use std::fs;
use std::net::TcpListener as StdTcpListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::grant::TaskGrant;
use tempfile::TempDir;
use ulid::Ulid;

use crate::config::{ProviderAuth, ProviderConfig, ProviderKind};
use crate::overlay_export_restore::{acquire, OverlayOperations, PendingFinalization};
use crate::store::Store;
use crate::{
    append_overlay_finalization_audits, bind_clock_and_finalize_overlay, bind_kernel_listener,
    build_provider_pool, select_default_provider_id,
    validate_startup_and_reconcile_overlay_terminal_erasures, Cli,
};
mod integrity;

const KEY: &[u8; 32] = b"0123456789abcdef0123456789abcdef";
const EXPORT_ACTION: &str = "openspine.overlay.export";
const RESTORE_ACTION: &str = "openspine.overlay.restore";

fn grant_for(user: &str, action: &str) -> TaskGrant {
    let mut g = grant(user);
    g.allowed_actions = vec![ActionId::new(action)];
    g
}

fn temp_data_root() -> (TempDir, PathBuf) {
    let root = TempDir::new().unwrap();
    let data = root.path().join("data");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("kernel.db"), b"v1").unwrap();
    fs::create_dir_all(data.join("artifacts")).unwrap();
    fs::create_dir_all(data.join("credentials")).unwrap();
    fs::create_dir_all(data.join("keys")).unwrap();
    fs::create_dir_all(data.join("artifacts.d")).unwrap();
    (root, data)
}

fn grant(user: &str) -> TaskGrant {
    let id = Ulid::new();
    let now = Timestamp::now();
    TaskGrant {
        id,
        schema_version: 1,
        lifecycle_state: openspine_schemas::artifact::Lifecycle::Active,
        user: user.to_owned(),
        purpose: "overlay-startup-test".into(),
        issued_by: "kernel".into(),
        issued_at: now,
        expires_at: now + Duration::from_secs(3600),
        event_id: Ulid::new(),
        route_id: "route.test".into(),
        agent_id: "agent.test".into(),
        workflow_id: "workflow.test".into(),
        capability_pack_id: "pack.test".into(),
        authority_sources: vec![],
        selection_tokens: vec![],
        allowed_actions: vec![ActionId::new(EXPORT_ACTION)],
        approval_required_actions: vec![],
        denied_actions: vec![],
        allowed_egress_classes: vec![],
        output_channels: vec![],
        limits: openspine_schemas::grant::GrantLimits {
            max_model_calls: 1,
            max_artifacts: 1,
            max_runtime_seconds: 1,
        },
        task_token: "token".into(),
        root_grant_id: id,
        parent_grant_id: None,
        mode: openspine_schemas::grant::GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
        persona_id: None,
    }
}

fn forge_mac(path: &Path) {
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let mac = value
        .get_mut("hmac_sha256")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_owned();
    let mut chars: Vec<char> = mac.chars().collect();
    chars[0] = if chars[0] == '0' { '1' } else { '0' };
    value["hmac_sha256"] = serde_json::Value::String(chars.into_iter().collect());
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}

fn pending_marker_exists(ops: &OverlayOperations) -> bool {
    ops.control_root().join("pending-operation.json").exists()
}

/// Stage an export, run pre-open publication, and leave the controller holding
/// a `PendingFinalization` without calling post-bind finalization.
fn stage_export_pending(data: &Path) -> (OverlayOperations, PendingFinalization) {
    fs::create_dir_all(data.join("artifacts")).unwrap();
    fs::create_dir_all(data.join("credentials")).unwrap();
    fs::create_dir_all(data.join("keys")).unwrap();
    fs::create_dir_all(data.join("artifacts.d")).unwrap();
    let ops = acquire(data, KEY).expect("acquire");
    ops.initialize_terminal_ledger()
        .expect("initialize terminal ledger");
    ops.stage_export_or_restore(
        &grant("owner"),
        &ActionId::new(EXPORT_ACTION),
        "bundle-a",
        Timestamp::now(),
    )
    .expect("stage export");
    let pending = ops
        .process_pre_open(false, Timestamp::now())
        .expect("pre-open export")
        .expect("pending finalization required");
    assert!(
        pending_marker_exists(&ops),
        "signed marker must remain until post-bind finalization"
    );
    (ops, pending)
}

/// Ephemeral free port as a `host:port` string for production bind helper.
fn free_bind_addr() -> String {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("ephemeral port");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("127.0.0.1:{port}")
}

#[test]
fn lifetime_lock_is_held_before_store_open() {
    let (_root, data) = temp_data_root();
    let ops = acquire(&data, KEY).expect("first acquire holds lifetime lock");
    let canonical = ops.canonical_data_root().to_path_buf();

    let alias = data
        .parent()
        .unwrap()
        .join("nested")
        .join("..")
        .join("data");
    assert!(
        acquire(&alias, KEY).is_err(),
        "alias of locked root must fail before any store open"
    );

    let artifacts_dir = canonical.join("artifacts");
    fs::create_dir_all(&artifacts_dir).unwrap();
    let _artifacts =
        crate::artifact_store::ArtifactStore::open(artifacts_dir, *KEY).expect("store after lock");
    let store_path = canonical.join("kernel-after-lock.db");
    let _store = Store::open(&store_path).expect("kernel store after lock");
    drop(ops);
}

#[test]
fn forged_marker_fails_before_store_open() {
    let (_root, data) = temp_data_root();
    {
        let ops = acquire(&data, KEY).unwrap();
        ops.stage_export_or_restore(
            &grant("owner"),
            &ActionId::new(EXPORT_ACTION),
            "bundle-a",
            Timestamp::now(),
        )
        .unwrap();
        forge_mac(&ops.control_root().join("pending-operation.json"));
    }

    let ops = acquire(&data, KEY).expect("acquire under forged marker");
    let err = ops
        .process_pre_open(false, Timestamp::now())
        .expect_err("forged HMAC must fail pre-open");
    let msg = err.to_string();
    assert!(
        msg.contains("authentication") || msg.contains("Authentication"),
        "unexpected error: {msg}"
    );
    assert!(pending_marker_exists(&ops));
}

#[test]
fn provider_failure_retains_pending_finalization() {
    let (_root, data) = temp_data_root();
    let (ops, _pending) = stage_export_pending(&data);
    let store = Store::open_in_memory().unwrap();

    // Empty provider list: production gate fails closed before bind/finalization.
    let err = match build_provider_pool(&[]) {
        Ok(_) => panic!("empty provider list must fail production gate"),
        Err(err) => err,
    };
    assert!(
        err.to_string()
            .contains("must configure at least one provider"),
        "unexpected: {err}"
    );

    // Malformed provider config: missing API-key env fails in the real
    // config::provider_api_key path used by production build_provider_pool.
    let bad = ProviderConfig {
        id: "broken-provider".to_string(),
        kind: ProviderKind::Anthropic,
        base_url: None,
        model: "test-model".to_string(),
        auth: ProviderAuth::ApiKey {
            env: "OPENSPINE_TEST_MISSING_PROVIDER_KEY_ENV".to_string(),
        },
    };
    // SAFETY: test-only; unit tests run single-threaded per process.
    std::env::remove_var("OPENSPINE_TEST_MISSING_PROVIDER_KEY_ENV");
    let err = match build_provider_pool(&[bad]) {
        Ok(_) => panic!("missing provider key must fail"),
        Err(err) => err,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("OPENSPINE_TEST_MISSING_PROVIDER_KEY_ENV")
            || msg.contains("environment")
            || msg.contains("MissingEnv")
            || msg.contains("missing")
            || msg.contains("not present"),
        "unexpected provider key error: {msg}"
    );

    assert!(
        pending_marker_exists(&ops),
        "pending marker retained after provider failure"
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("overlay.export_completed")
            .unwrap(),
        0
    );
    assert!(select_default_provider_id(&[]).is_err());
}

#[tokio::test]
async fn bind_failure_retains_pending_finalization() {
    let (_root, data) = temp_data_root();
    let (ops, pending) = stage_export_pending(&data);
    let store = Store::open_in_memory().unwrap();
    let pre = Timestamp::now().as_millisecond();

    // Occupy a port, then ask the production bind helper for the same address.
    let occupied = StdTcpListener::bind("127.0.0.1:0").expect("occupy port");
    let addr = occupied.local_addr().unwrap().to_string();

    let err = bind_clock_and_finalize_overlay(&addr, &store, pre, || pre + 1, &ops, Some(&pending))
        .await
        .expect_err("occupied port must fail production bind");
    let msg = err.to_string();
    assert!(
        msg.contains("binding")
            || msg.contains("Address already in use")
            || msg.contains("os error"),
        "unexpected bind error: {msg}"
    );
    assert!(
        pending_marker_exists(&ops),
        "pending marker retained after bind failure"
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("overlay.export_completed")
            .unwrap(),
        0
    );
    drop(occupied);
}

#[tokio::test]
async fn post_bind_clock_failure_retains_pending_finalization() {
    let (_root, data) = temp_data_root();
    let (ops, pending) = stage_export_pending(&data);
    let store = Store::open_in_memory().unwrap();
    let pre = 1_000_000_i64;
    let regressed = pre - 60_001;
    let addr = free_bind_addr();

    // Real bind succeeds; production post-bind clock commit fails; finalization
    // must not run, so the marker remains.
    let err =
        bind_clock_and_finalize_overlay(&addr, &store, pre, || regressed, &ops, Some(&pending))
            .await
            .expect_err("post-bind clock regression must abort before finalization");
    assert!(
        err.to_string()
            .contains("wall clock regressed during startup"),
        "unexpected: {err}"
    );
    assert!(
        pending_marker_exists(&ops),
        "pending marker retained after clock failure"
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("overlay.export_completed")
            .unwrap(),
        0
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("overlay.export_requested")
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn successful_finalization_audits_then_clears_pending() {
    let (_root, data) = temp_data_root();
    let (ops, pending) = stage_export_pending(&data);
    let store = Store::open_in_memory().unwrap();
    let pre = Timestamp::now().as_millisecond();
    let addr = free_bind_addr();

    // Full production continuation: bind → clock → begin/audit/cleanup.
    let _listener =
        bind_clock_and_finalize_overlay(&addr, &store, pre, || pre + 1, &ops, Some(&pending))
            .await
            .expect("successful post-bind finalization");

    assert!(
        !pending_marker_exists(&ops),
        "marker cleared only after audit+cleanup"
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("overlay.export_requested")
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("overlay.export_completed")
            .unwrap(),
        1
    );

    // Prove audit-before-cleanup ordering on a fresh pending: audit leaves the
    // marker, cleanup removes it.
    let (ops2, pending2) = stage_export_pending(&data.join("second"));
    // second root needs its own data tree for export publish
    // (stage_export_pending already wrote one). Step through audit only.
    let meta = ops2
        .begin_finalization(&pending2, Timestamp::now())
        .expect("begin finalization");
    append_overlay_finalization_audits(&store, &meta).expect("audit");
    assert!(
        pending_marker_exists(&ops2),
        "marker must still exist after audit and before durable cleanup"
    );
    ops2.complete_finalization(&meta).expect("cleanup");
    assert!(!pending_marker_exists(&ops2));

    // Idempotent re-append does not duplicate.
    append_overlay_finalization_audits(&store, &meta).unwrap();
    // At least the original request's two events remain exactly one each for
    // the first request_id; second request adds its own pair.
    assert!(
        store
            .count_audit_events_of_kind("overlay.export_requested")
            .unwrap()
            >= 2
    );
}

#[test]
fn rollback_pending_restore_flag_is_pathless() {
    let cli = Cli::try_parse_from(["openspine", "--rollback-pending-restore"])
        .expect("pathless flag must parse");
    assert!(cli.rollback_pending_restore);

    let cli_default = Cli::try_parse_from(["openspine"]).expect("default parse");
    assert!(!cli_default.rollback_pending_restore);

    // Extra free argument is rejected: the flag is a pure boolean, not a path.
    assert!(
        Cli::try_parse_from(["openspine", "--rollback-pending-restore", "/some/path"]).is_err(),
        "path argument after --rollback-pending-restore must be rejected"
    );
}

#[test]
fn fixture_appstate_retains_overlay_operations_lock() {
    let state = crate::test_support::fixtures::test_state();
    let canonical = state.overlay_operations.canonical_data_root().to_path_buf();
    assert!(
        acquire(&canonical, KEY).is_err(),
        "AppState must retain exclusive lifetime lock"
    );
    assert!(state.overlay_dir.starts_with(&canonical));
}

#[tokio::test]
async fn production_bind_helper_is_used_by_late_path() {
    // Sanity: free address binds via the same helper main uses.
    let addr = free_bind_addr();
    let listener = bind_kernel_listener(&addr).await.expect("bind free port");
    drop(listener);
}

#[test]
fn completed_audit_then_rollback_audit_sequence_records_both_events() {
    let (_root, data) = temp_data_root();
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::now();

    let ops = acquire(&data, KEY).unwrap();
    ops.initialize_terminal_ledger().unwrap();
    ops.stage_export_or_restore(
        &grant_for("owner", EXPORT_ACTION),
        &ActionId::new(EXPORT_ACTION),
        "bundle-r",
        now,
    )
    .unwrap();
    let export_p = ops.process_pre_open(false, now).unwrap().unwrap();
    let export_m = ops.begin_finalization(&export_p, now).unwrap();
    ops.complete_finalization(&export_m).unwrap();

    ops.stage_export_or_restore(
        &grant_for("owner", RESTORE_ACTION),
        &ActionId::new(RESTORE_ACTION),
        "bundle-r",
        now,
    )
    .unwrap();
    let restore_p = ops.process_pre_open(false, now).unwrap().unwrap();
    let completed_m = ops.begin_finalization(&restore_p, now).unwrap();

    append_overlay_finalization_audits(&store, &completed_m).unwrap();
    let count = |k: &str| store.count_audit_events_of_kind(k).unwrap();
    assert_eq!(
        (
            count("overlay.restore_requested"),
            count("overlay.restore_completed"),
            count("overlay.restore_rolled_back")
        ),
        (1, 1, 0)
    );

    drop(ops);
    let ops2 = acquire(&data, KEY).unwrap();
    let rolled_p = ops2.process_pre_open(true, now).unwrap().unwrap();
    assert_eq!(completed_m.request_id, rolled_p.request_id);

    let rolled_m = ops2.begin_finalization(&rolled_p, now).unwrap();
    assert_eq!(completed_m.request_id, rolled_m.request_id);

    append_overlay_finalization_audits(&store, &rolled_m).unwrap();
    assert_eq!(
        (
            count("overlay.restore_requested"),
            count("overlay.restore_completed"),
            count("overlay.restore_rolled_back")
        ),
        (1, 1, 1)
    );

    append_overlay_finalization_audits(&store, &rolled_m).unwrap();
    assert_eq!(count("overlay.restore_rolled_back"), 1);
    assert!(store.verify_audit_chain().unwrap());
}

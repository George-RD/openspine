//! Focused state-machine / failpoint tests for export/restore orchestration.

use super::control::{ControlError, OperationStage, EXPORT_ACTION, OPERATION_FILE, RESTORE_ACTION};
use super::install::{
    cleanup_install, inspect_install, install_or_recover, previous_path, rollback_or_recover,
    staged_path, InstallState,
};
use super::operation::{acquire, OverlayOperations};
use super::types::{
    FinalizationOutcome, OverlayOperationError, OverlayOperationKind, PendingFinalization,
};
use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::grant::TaskGrant;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;
use ulid::Ulid;

mod flows;
mod regressions;
mod rollback_regressions;

pub(super) const KEY: &[u8] = b"openspine-test-master-key-32bytes!";

pub(super) fn temp_data_root() -> (TempDir, PathBuf) {
    let root = TempDir::new().unwrap();
    let data = root.path().join("data");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("kernel.db"), b"v1").unwrap();
    fs::create_dir_all(data.join("keys")).unwrap();
    fs::write(data.join("keys").join("cp1"), b"wrapped").unwrap();
    (root, data)
}

pub(super) fn grant(user: &str, action: &str) -> TaskGrant {
    let id = Ulid::new();
    let now = Timestamp::now();
    TaskGrant {
        id,
        schema_version: 1,
        lifecycle_state: openspine_schemas::artifact::Lifecycle::Active,
        user: user.to_owned(),
        purpose: "overlay-operation-test".into(),
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
        allowed_actions: vec![ActionId::new(action)],
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

pub(super) fn seed_export(ops: &OverlayOperations, name: &str) -> String {
    ops.initialize_terminal_ledger().unwrap();
    let staged = ops
        .stage_export_or_restore(
            &grant("owner", EXPORT_ACTION),
            &ActionId::new(EXPORT_ACTION),
            name,
            Timestamp::now(),
        )
        .unwrap();
    staged.request_id().to_owned()
}

pub(super) fn finalize_ok(ops: &OverlayOperations, pending: PendingFinalization) {
    let meta = ops.begin_finalization(&pending, Timestamp::now()).unwrap();
    ops.complete_finalization(&meta).unwrap();
}

pub(super) fn marker(path: &Path) -> String {
    fs::read_to_string(path.join("kernel.db")).unwrap()
}

#[test]
fn export_retry_is_idempotent_across_restart() {
    let (_root, data) = temp_data_root();
    let request_id = {
        let ops = acquire(&data, KEY).unwrap();
        let id = seed_export(&ops, "export-a");
        let pending = ops
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .expect("export pending");
        assert_eq!(pending.kind, OverlayOperationKind::Export);
        assert_eq!(pending.outcome, FinalizationOutcome::Completed);
        assert_eq!(pending.request_id, id);
        assert!(ops
            .snapshots_root()
            .join("export-a")
            .join("manifest.json")
            .exists());
        id
    };
    let ops = acquire(&data, KEY).unwrap();
    let pending = ops
        .process_pre_open(false, Timestamp::now())
        .unwrap()
        .expect("export still pending");
    assert_eq!(pending.request_id, request_id);
    assert!(ops
        .snapshots_root()
        .join("export-a")
        .join("manifest.json")
        .exists());
    finalize_ok(&ops, pending);
    assert!(!ops.control_root().join(OPERATION_FILE).exists());
}

#[test]
fn restore_installs_and_finalizes_cleanly() {
    let (_root, data) = temp_data_root();
    {
        let ops = acquire(&data, KEY).unwrap();
        let _ = seed_export(&ops, "bundle-r");
        let pending = ops
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();
        finalize_ok(&ops, pending);
    }
    fs::write(data.join("kernel.db"), b"mutated").unwrap();
    let ops = acquire(&data, KEY).unwrap();
    ops.initialize_terminal_ledger().unwrap();
    let staged = ops
        .stage_export_or_restore(
            &grant("owner", RESTORE_ACTION),
            &ActionId::new(RESTORE_ACTION),
            "bundle-r",
            Timestamp::now(),
        )
        .unwrap();
    let request_id = staged.request_id().to_owned();
    let pending = ops
        .process_pre_open(false, Timestamp::now())
        .unwrap()
        .expect("restore pending");
    assert_eq!(pending.kind, OverlayOperationKind::Restore);
    assert_eq!(pending.outcome, FinalizationOutcome::Completed);
    assert_eq!(marker(&data), "v1");
    assert_eq!(
        inspect_install(&data, &request_id).unwrap(),
        InstallState::Swapped
    );
    finalize_ok(&ops, pending);
    assert_eq!(
        inspect_install(&data, &request_id).unwrap(),
        InstallState::Clean
    );
}

#[test]
fn restore_recover_from_staged_only_and_previous_only() {
    let root = TempDir::new().unwrap();
    let live = root.path().join("data");
    let request_id = Ulid::new().to_string();

    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("kernel.db"), b"live").unwrap();
    let staged = staged_path(&live, &request_id);
    fs::create_dir_all(&staged).unwrap();
    fs::write(staged.join("kernel.db"), b"new").unwrap();
    install_or_recover(&live, &request_id).unwrap();
    assert_eq!(marker(&live), "new");
    assert_eq!(
        inspect_install(&live, &request_id).unwrap(),
        InstallState::Swapped
    );

    fs::remove_dir_all(&live).unwrap();
    let previous = previous_path(&live, &request_id);
    fs::create_dir_all(&staged).unwrap();
    fs::write(staged.join("kernel.db"), b"new2").unwrap();
    fs::create_dir_all(&previous).unwrap();
    fs::write(previous.join("kernel.db"), b"old").unwrap();
    install_or_recover(&live, &request_id).unwrap();
    assert_eq!(marker(&live), "new2");
}

#[test]
fn pathless_rollback_restores_previous_tree() {
    let (_root, data) = temp_data_root();
    {
        let ops = acquire(&data, KEY).unwrap();
        let _ = seed_export(&ops, "bundle-rb");
        finalize_ok(
            &ops,
            ops.process_pre_open(false, Timestamp::now())
                .unwrap()
                .unwrap(),
        );
    }
    fs::write(data.join("kernel.db"), b"pre-restore").unwrap();
    let ops = acquire(&data, KEY).unwrap();
    ops.initialize_terminal_ledger().unwrap();
    let staged = ops
        .stage_export_or_restore(
            &grant("owner", RESTORE_ACTION),
            &ActionId::new(RESTORE_ACTION),
            "bundle-rb",
            Timestamp::now(),
        )
        .unwrap();
    let request_id = staged.request_id().to_owned();
    let _pending = ops
        .process_pre_open(false, Timestamp::now())
        .unwrap()
        .unwrap();
    assert_eq!(marker(&data), "v1");
    drop(ops);

    let ops = acquire(&data, KEY).unwrap();
    let rolled = ops
        .process_pre_open(true, Timestamp::now())
        .unwrap()
        .expect("rollback pending");
    assert_eq!(rolled.outcome, FinalizationOutcome::RolledBack);
    assert_eq!(marker(&data), "pre-restore");
    finalize_ok(&ops, rolled);
    assert_eq!(
        inspect_install(&data, &request_id).unwrap(),
        InstallState::Clean
    );
}

#[test]
fn rollback_recover_from_each_rename_state() {
    let root = TempDir::new().unwrap();
    let live = root.path().join("data");
    let request_id = Ulid::new().to_string();

    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("kernel.db"), b"live").unwrap();
    let staged = staged_path(&live, &request_id);
    fs::create_dir_all(&staged).unwrap();
    fs::write(staged.join("kernel.db"), b"staged").unwrap();
    rollback_or_recover(&live, &request_id).unwrap();
    assert_eq!(marker(&live), "live");
    assert!(staged.exists());
    cleanup_install(&live, &request_id).unwrap();
    assert!(!staged.exists());
    fs::write(live.join("kernel.db"), b"new").unwrap();
    let previous = previous_path(&live, &request_id);
    fs::create_dir_all(&previous).unwrap();
    fs::write(previous.join("kernel.db"), b"old").unwrap();
    rollback_or_recover(&live, &request_id).unwrap();
    assert_eq!(marker(&live), "old");
    assert!(staged.exists());
    cleanup_install(&live, &request_id).unwrap();
    assert!(!staged.exists());
    fs::remove_dir_all(&live).unwrap();
    fs::create_dir_all(&previous).unwrap();
    fs::write(previous.join("kernel.db"), b"old2").unwrap();
    fs::create_dir_all(&staged).unwrap();
    fs::write(staged.join("kernel.db"), b"junk").unwrap();
    rollback_or_recover(&live, &request_id).unwrap();
    assert_eq!(marker(&live), "old2");
    assert!(staged.exists());
    cleanup_install(&live, &request_id).unwrap();
    assert!(!staged.exists());
}

pub(super) fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), to).unwrap();
        }
    }
}
pub(super) fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    const D: &[u8; 16] = b"0123456789abcdef";
    for &b in bytes {
        out.push(D[(b >> 4) as usize] as char);
        out.push(D[(b & 15) as usize] as char);
    }
    out
}

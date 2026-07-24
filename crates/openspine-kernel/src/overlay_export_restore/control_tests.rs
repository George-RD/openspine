use std::collections::BTreeSet;

use super::*;
use std::fs;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;

const KEY: &[u8] = b"openspine-test-master-key-32bytes!";

fn temp_data_root() -> (TempDir, PathBuf) {
    let root = TempDir::new().unwrap();
    let data = root.path().join("data");
    fs::create_dir_all(&data).unwrap();
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
        purpose: "overlay-control-test".into(),
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

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
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

#[test]
fn rejects_alias_paths_and_symlinks() {
    let (root, data) = temp_data_root();
    let alias = root.path().join("nested").join("..").join("data");
    let _first = OverlayControl::acquire(&data, KEY).unwrap();
    assert!(matches!(
        OverlayControl::acquire(&alias, KEY),
        Err(ControlError::SymlinkDataRoot(_))
    ));

    // Parent path itself is a symlink — reject without following into real parent.
    let parent_target = root.path().join("real-parent");
    fs::create_dir_all(parent_target.join("data")).unwrap();
    let parent_link = root.path().join("sym-parent");
    symlink(&parent_target, &parent_link).unwrap();
    let data_under_link = parent_link.join("data");
    assert!(matches!(
        OverlayControl::acquire(&data_under_link, KEY),
        Err(ControlError::SymlinkDataRoot(_))
    ));
    // Nested ancestor symlink: root/link/sub/data where link is a symlink.
    let nested_target = root.path().join("nested-target");
    fs::create_dir_all(nested_target.join("sub").join("data")).unwrap();
    let nested_link = root.path().join("nested-link");
    symlink(&nested_target, &nested_link).unwrap();
    let nested_data = nested_link.join("sub").join("data");
    assert!(matches!(
        OverlayControl::acquire(&nested_data, KEY),
        Err(ControlError::SymlinkDataRoot(_))
    ));
    // Absolute same-basename user alias must still be rejected.
    let abs_target = root.path().join("abs-target");
    fs::create_dir_all(abs_target.join("sub").join("data")).unwrap();
    let abs_link = root.path().join("abs-target-link");
    // Same basename component renamed via absolute symlink.
    let attacker = root.path().join("attacker");
    fs::create_dir_all(attacker.join("abs-target-link").join("sub").join("data")).unwrap();
    symlink(attacker.join("abs-target-link"), &abs_link).unwrap();
    let abs_data = abs_link.join("sub").join("data");
    assert!(matches!(
        OverlayControl::acquire(&abs_data, KEY),
        Err(ControlError::SymlinkDataRoot(_))
    ));
}

#[test]
fn second_acquire_preserves_owner_temp_state() {
    let (_root, data) = temp_data_root();
    let first = OverlayControl::acquire(&data, KEY).unwrap();
    let temp = first.control_root().join(OPERATION_TEMP);
    let marker = b"owner-temp-marker-v1";
    fs::write(&temp, marker).unwrap();
    let before_meta = fs::metadata(&temp).unwrap();
    let before_ino = before_meta.ino();
    let before_mtime = (before_meta.mtime(), before_meta.mtime_nsec());
    assert!(matches!(
        OverlayControl::acquire(&data, KEY),
        Err(ControlError::AlreadyLocked(_))
    ));
    assert!(temp.exists(), "contender must not delete owner temp");
    assert_eq!(fs::read(&temp).unwrap(), marker);
    let after_meta = fs::metadata(&temp).unwrap();
    assert_eq!(after_meta.len(), before_meta.len());
    assert_eq!(
        after_meta.permissions().mode() & 0o777,
        before_meta.permissions().mode() & 0o777
    );
    assert_eq!(
        after_meta.ino(),
        before_ino,
        "contender must not replace temp"
    );
    assert_eq!(
        (after_meta.mtime(), after_meta.mtime_nsec()),
        before_mtime,
        "contender must not rewrite owner temp"
    );
    drop(first);
}

#[test]
fn rejects_symlink_and_non_directory_data_roots() {
    let root = TempDir::new().unwrap();
    let target = root.path().join("target");
    fs::create_dir_all(&target).unwrap();
    let link = root.path().join("linked-data");
    symlink(&target, &link).unwrap();
    assert!(matches!(
        OverlayControl::acquire(&link, KEY),
        Err(ControlError::SymlinkDataRoot(_))
    ));
    let file = root.path().join("file-root");
    fs::write(&file, b"nope").unwrap();
    assert!(matches!(
        OverlayControl::acquire(&file, KEY),
        Err(ControlError::NotDirectory(_))
    ));
}

#[test]
fn control_and_state_files_use_restrictive_modes() {
    let (_root, data) = temp_data_root();
    let control = OverlayControl::acquire(&data, KEY).unwrap();
    assert_eq!(mode(control.control_root()), 0o700);
    assert_eq!(mode(control.snapshots_root()), 0o700);
    assert_eq!(mode(&control.control_root().join("lifetime.lock")), 0o600);
    control.initialize_terminal_ledger().unwrap();
    assert_eq!(mode(&control.control_root().join(LEDGER_FILE)), 0o600);
    control
        .stage_export_or_restore(
            &grant("owner"),
            &ActionId::new(EXPORT_ACTION),
            "bundle-a",
            Timestamp::now(),
        )
        .unwrap();
    assert_eq!(mode(&control.control_root().join(OPERATION_FILE)), 0o600);
}

#[test]
fn bundle_name_validation_and_path_derivation() {
    assert!(BundleName::parse("").is_err());
    assert!(BundleName::parse(".hidden").is_err());
    assert!(BundleName::parse("bad/name").is_err());
    assert!(BundleName::parse(&"a".repeat(129)).is_err());
    assert!(BundleName::parse("good_Name-1").is_ok());
    let (_root, data) = temp_data_root();
    let control = OverlayControl::acquire(&data, KEY).unwrap();
    let name = BundleName::parse("good_Name-1").unwrap();
    assert_eq!(
        control.bundle_path(&name),
        control.snapshots_root().join("good_Name-1")
    );
}

#[test]
fn forged_marker_and_ledger_are_rejected() {
    let (_root, data) = temp_data_root();
    let control = OverlayControl::acquire(&data, KEY).unwrap();
    control
        .stage_export_or_restore(
            &grant("owner"),
            &ActionId::new(EXPORT_ACTION),
            "bundle-a",
            Timestamp::now(),
        )
        .unwrap();
    forge_mac(&control.control_root().join(OPERATION_FILE));
    assert!(matches!(
        control.load_operation(),
        Err(ControlError::AuthenticationFailed)
    ));
    let ledger = control.initialize_terminal_ledger().unwrap();
    let portable = control.encode_portable(ledger).unwrap();
    let mut forged: serde_json::Value = serde_json::from_slice(&portable).unwrap();
    let mac = forged["hmac_sha256"].as_str().unwrap().to_owned();
    let mut chars: Vec<char> = mac.chars().collect();
    chars[0] = if chars[0] == '0' { '1' } else { '0' };
    forged["hmac_sha256"] = serde_json::Value::String(chars.into_iter().collect());
    assert!(matches!(
        control.import_terminal_ledger(&serde_json::to_vec(&forged).unwrap()),
        Err(ControlError::AuthenticationFailed)
    ));
}

#[test]
fn enforces_one_pending_operation_and_transitions() {
    let (_root, data) = temp_data_root();
    let control = OverlayControl::acquire(&data, KEY).unwrap();
    let first = control
        .stage_export_or_restore(
            &grant("owner"),
            &ActionId::new(EXPORT_ACTION),
            "bundle-a",
            Timestamp::now(),
        )
        .unwrap();
    assert!(matches!(
        control.stage_export_or_restore(
            &grant("owner"),
            &ActionId::new(EXPORT_ACTION),
            "bundle-b",
            Timestamp::now(),
        ),
        Err(ControlError::OperationPending)
    ));
    assert_eq!(
        control
            .transition_operation(first.request_id(), OperationStage::Staged)
            .unwrap()
            .stage(),
        OperationStage::Staged
    );
    assert_eq!(
        control
            .transition_operation(first.request_id(), OperationStage::Finalizing)
            .unwrap()
            .stage(),
        OperationStage::Finalizing
    );
    control.clear_operation(first.request_id()).unwrap();
    assert!(control.load_operation().unwrap().is_none());
}

#[test]
fn interrupted_update_recovers_last_complete_state() {
    let (_root, data) = temp_data_root();
    let control = OverlayControl::acquire(&data, KEY).unwrap();
    let pending = control
        .stage_export_or_restore(
            &grant("owner"),
            &ActionId::new(EXPORT_ACTION),
            "bundle-a",
            Timestamp::now(),
        )
        .unwrap();
    let temp = control.control_root().join(OPERATION_TEMP);
    fs::write(&temp, b"partial").unwrap();
    let loaded = control.load_operation().unwrap().unwrap();
    assert_eq!(loaded.request_id(), pending.request_id());
    assert_eq!(loaded.stage(), OperationStage::Requested);
    drop(control);
    let control = OverlayControl::acquire(&data, KEY).unwrap();
    assert!(!temp.exists());
    assert_eq!(
        control.load_operation().unwrap().unwrap().request_id(),
        pending.request_id()
    );
}

#[test]
fn terminal_erasure_is_monotonic_and_idempotent() {
    let (_root, data) = temp_data_root();
    let control = OverlayControl::acquire(&data, KEY).unwrap();
    control.initialize_terminal_ledger().unwrap();
    assert_eq!(
        control.record_terminal_erasure("cp1").unwrap().sequence(),
        1
    );
    assert_eq!(
        control.record_terminal_erasure("cp1").unwrap().sequence(),
        1
    );
    let second = control.record_terminal_erasure("cp2").unwrap();
    assert_eq!(second.sequence(), 2);
    assert!(second.erased_counterparty_ids().contains("cp1"));
    assert!(second.erased_counterparty_ids().contains("cp2"));
}

#[test]
fn merges_bundle_baseline_and_rejects_missing_or_regressed_continuity() {
    let (_root, data) = temp_data_root();
    let control = OverlayControl::acquire(&data, KEY).unwrap();
    let init = control.initialize_terminal_ledger().unwrap();
    let continuity_id = init.continuity_id().to_owned();
    control.record_terminal_erasure("cp1").unwrap();
    control.record_terminal_erasure("cp2").unwrap();
    let portable = control.export_portable_continuity().unwrap();
    let baseline = control
        .sign_ledger(TerminalLedger::with_continuity_id(
            continuity_id.clone(),
            1,
            BTreeSet::from(["cp1".into()]),
        ))
        .unwrap();
    let merged = control.merge_bundle_baseline(&baseline, &portable).unwrap();
    assert_eq!(merged.sequence(), 2);
    assert_eq!(merged.continuity_id(), continuity_id);
    assert!(merged.erased_counterparty_ids().contains("cp2"));

    let fresh_root = TempDir::new().unwrap();
    let fresh_data = fresh_root.path().join("data");
    fs::create_dir_all(&fresh_data).unwrap();
    let fresh = OverlayControl::acquire(&fresh_data, KEY).unwrap();
    // No local continuity and empty portable.
    assert!(matches!(
        fresh.merge_bundle_baseline(&baseline, b""),
        Err(ControlError::MissingContinuity)
    ));
    // No local continuity yet: portable alone is insufficient for merge.
    assert!(matches!(
        fresh.merge_bundle_baseline(&baseline, &portable),
        Err(ControlError::MissingContinuity)
    ));
    // Foreign empty continuity must not accept source baseline.
    let foreign = fresh.initialize_terminal_ledger().unwrap();
    assert_ne!(foreign.continuity_id(), continuity_id);
    assert!(matches!(
        fresh.merge_bundle_baseline(&baseline, &portable),
        Err(ControlError::RegressedContinuity | ControlError::DivergedContinuity)
    ));
    // Clean host: import source continuity first, then merge succeeds.
    // Older local continuity alone cannot satisfy a higher baseline.
    let clean_root = TempDir::new().unwrap();
    let clean_data = clean_root.path().join("data");
    fs::create_dir_all(&clean_data).unwrap();
    let clean = OverlayControl::acquire(&clean_data, KEY).unwrap();
    let older = control
        .sign_ledger(TerminalLedger::with_continuity_id(
            continuity_id.clone(),
            0,
            BTreeSet::new(),
        ))
        .unwrap();
    let older_portable = control.encode_portable(older).unwrap();
    clean.import_terminal_ledger(&older_portable).unwrap();
    assert!(matches!(
        clean.merge_bundle_baseline(&baseline, &older_portable),
        Err(ControlError::RegressedContinuity)
    ));
    // Import full source continuity, then same-lineage merge is allowed.
    clean.import_terminal_ledger(&portable).unwrap();
    let good = clean.merge_bundle_baseline(&baseline, &portable).unwrap();
    assert_eq!(good.sequence(), 2);
    assert_eq!(good.continuity_id(), continuity_id);
}
#[path = "control_tests/hardening.rs"]
mod hardening;
#[path = "control_tests/portability.rs"]
mod portability;

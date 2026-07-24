//! Additional focused operation flow tests (split for file-size gate).

use super::*;
use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use std::fs;
use tempfile::TempDir;

#[test]
fn invalid_hmac_bundle_causes_no_mutation() {
    let (_root, data) = temp_data_root();
    {
        let ops = acquire(&data, KEY).unwrap();
        let _ = seed_export(&ops, "bundle-bad");
        finalize_ok(
            &ops,
            ops.process_pre_open(false, Timestamp::now())
                .unwrap()
                .unwrap(),
        );
    }
    {
        let ops = acquire(&data, KEY).unwrap();
        let manifest = ops
            .snapshots_root()
            .join("bundle-bad")
            .join("manifest.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        let mac = value["hmac_sha256"].as_str().unwrap().to_owned();
        let mut chars: Vec<char> = mac.chars().collect();
        chars[0] = if chars[0] == '0' { '1' } else { '0' };
        value["hmac_sha256"] = serde_json::Value::String(chars.into_iter().collect());
        fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    fs::write(data.join("kernel.db"), b"keep-me").unwrap();
    let before_entries: Vec<_> = fs::read_dir(&data)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    let ops = acquire(&data, KEY).unwrap();
    ops.initialize_terminal_ledger().unwrap();
    let ledger_path = ops.control_root().join("terminal-erasure-ledger.json");
    let ledger_before = fs::read(&ledger_path).unwrap();
    let err = ops
        .stage_export_or_restore(
            &grant("owner", RESTORE_ACTION),
            &ActionId::new(RESTORE_ACTION),
            "bundle-bad",
            Timestamp::now(),
        )
        .unwrap_err();
    assert!(
        matches!(err, ControlError::AuthenticationFailed),
        "request-time invalid HMAC must fail staging, got {err}"
    );
    assert_eq!(marker(&data), "keep-me");
    assert_eq!(fs::read(&ledger_path).unwrap(), ledger_before);
    let after_entries: Vec<_> = fs::read_dir(&data)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(before_entries, after_entries);
    assert!(
        !ops.control_root().join(OPERATION_FILE).exists(),
        "failed request-time validation must leave no pending marker"
    );
}

#[test]
fn invalid_bundle_causes_no_mutation() {
    let (_root, data) = temp_data_root();
    let ops = acquire(&data, KEY).unwrap();
    ops.initialize_terminal_ledger().unwrap();
    let ledger_path = ops.control_root().join("terminal-erasure-ledger.json");
    let ledger_before = fs::read(&ledger_path).unwrap();
    fs::write(data.join("kernel.db"), b"keep-me").unwrap();
    let err = ops
        .stage_export_or_restore(
            &grant("owner", RESTORE_ACTION),
            &ActionId::new(RESTORE_ACTION),
            "missing-bundle",
            Timestamp::now(),
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            ControlError::NotDirectory(_) | ControlError::AuthenticationFailed
        ),
        "request-time missing bundle must fail staging, got {err}"
    );
    assert_eq!(marker(&data), "keep-me");
    assert_eq!(fs::read(&ledger_path).unwrap(), ledger_before);
    assert!(
        !ops.control_root().join(OPERATION_FILE).exists(),
        "failed request-time validation must leave no pending marker"
    );
    // No staged install siblings created for the failed request.
    let sibling_names: Vec<_> = fs::read_dir(data.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| {
            name.contains(".openspine-restore-new") || name.contains(".openspine-restore-old")
        })
        .collect();
    assert!(
        sibling_names.is_empty(),
        "failed request-time validation must not create install siblings: {sibling_names:?}"
    );
}

#[test]
fn merged_ledger_hardening_applies_all_merged_ids() {
    let (_root, data) = temp_data_root();
    {
        let ops = acquire(&data, KEY).unwrap();
        fs::write(data.join("keys").join("cpabc123"), b"wrapped-live").unwrap();
        let _ = seed_export(&ops, "bundle-ledger");
        let pending = ops
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();
        finalize_ok(&ops, pending);
    }
    let ops = acquire(&data, KEY).unwrap();
    ops.record_terminal_erasure("cpabc123").unwrap();
    assert!(data.join("keys").join("cpabc123").exists());
    ops.stage_export_or_restore(
        &grant("owner", RESTORE_ACTION),
        &ActionId::new(RESTORE_ACTION),
        "bundle-ledger",
        Timestamp::now(),
    )
    .unwrap();
    let pending = ops
        .process_pre_open(false, Timestamp::now())
        .expect("restore pre-open should succeed after ledger harden")
        .expect("restore pending");
    assert!(
        !data.join("keys").join("cpabc123").exists(),
        "merged later erasure must be applied during restore staging"
    );
    assert!(
        data.join("keys").join("cpabc123.erased").exists(),
        "tombstone for merged later erasure must exist"
    );
    finalize_ok(&ops, pending);
    let ledger = ops.export_terminal_ledger().unwrap();
    assert!(ledger
        .erased_counterparty_ids()
        .iter()
        .any(|id| id == "cpabc123"));
}

#[test]
fn fresh_host_restore_rejects_without_continuity() {
    let root = TempDir::new().unwrap();
    // Host A: export bundle + portable continuity.
    let data_a = root.path().join("a");
    fs::create_dir_all(data_a.join("keys")).unwrap();
    fs::write(data_a.join("kernel.db"), b"v1").unwrap();
    fs::write(data_a.join("keys").join("cp1"), b"wrapped").unwrap();
    let (portable, bundle_src) = {
        let ops = acquire(&data_a, KEY).unwrap();
        let _ = seed_export(&ops, "bundle-fresh");
        let pending = ops
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();
        finalize_ok(&ops, pending);
        let portable = ops.export_portable_continuity().unwrap();
        let src = ops.snapshots_root().join("bundle-fresh");
        // Materialize bundle bytes while lock held, then drop.
        let staged = root.path().join("bundle-fresh-copy");
        copy_dir(&src, &staged);
        (portable, staged)
    };

    // Host B without continuity: must reject.
    let data_b = root.path().join("b");
    fs::create_dir_all(&data_b).unwrap();
    fs::write(data_b.join("kernel.db"), b"other").unwrap();
    {
        let ops = acquire(&data_b, KEY).unwrap();
        copy_dir(&bundle_src, &ops.snapshots_root().join("bundle-fresh"));
        ops.initialize_terminal_ledger().unwrap(); // different continuity_id
        ops.stage_export_or_restore(
            &grant("owner", RESTORE_ACTION),
            &ActionId::new(RESTORE_ACTION),
            "bundle-fresh",
            Timestamp::now(),
        )
        .unwrap();
        let err = ops.process_pre_open(false, Timestamp::now()).unwrap_err();
        assert!(
            matches!(
                err,
                OverlayOperationError::Control(
                    ControlError::MissingContinuity
                        | ControlError::RegressedContinuity
                        | ControlError::DivergedContinuity
                        | ControlError::AuthenticationFailed
                )
            ),
            "fresh host without matching continuity must reject, got {err}"
        );
        assert_eq!(marker(&data_b), "other");
    }

    // Host C: import matching continuity BEFORE staging restore.
    let data_c = root.path().join("c");
    fs::create_dir_all(&data_c).unwrap();
    fs::write(data_c.join("kernel.db"), b"other").unwrap();
    let ops = acquire(&data_c, KEY).unwrap();
    copy_dir(&bundle_src, &ops.snapshots_root().join("bundle-fresh"));
    ops.import_portable_continuity(&portable).unwrap();
    ops.stage_export_or_restore(
        &grant("owner", RESTORE_ACTION),
        &ActionId::new(RESTORE_ACTION),
        "bundle-fresh",
        Timestamp::now(),
    )
    .unwrap();
    let pending = ops
        .process_pre_open(false, Timestamp::now())
        .unwrap()
        .unwrap();
    assert_eq!(marker(&data_c), "v1");
    finalize_ok(&ops, pending);
}

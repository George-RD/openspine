//! Focused regressions for overlay export/restore edge cases and security invariants.

use super::super::install;
use super::*;
use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use std::fs;

#[test]
fn regression_stale_valid_bundle_metadata_mismatch_rejected() {
    let (_root, data) = temp_data_root();
    // Stage an export (no bundle yet), plant a same-name bundle with different
    // signed request metadata, then recover — process_pre_open must reject.
    let ops = acquire(&data, KEY).unwrap();
    ops.initialize_terminal_ledger().unwrap();
    let staged = ops
        .stage_export_or_restore(
            &grant("owner", EXPORT_ACTION),
            &ActionId::new(EXPORT_ACTION),
            "bundle-meta",
            Timestamp::now(),
        )
        .unwrap();
    let request_id = staged.request_id().to_owned();
    assert_ne!(request_id, "req_different");

    // Plant a same-name bundle whose signed request metadata does not match the pending op.
    let bundle_path = ops.snapshots_root().join("bundle-meta");
    fs::create_dir_all(bundle_path.join("data")).unwrap();
    let payload = b"v1";
    fs::write(bundle_path.join("data").join("kernel.db"), payload).unwrap();
    let digest = {
        use sha2::{Digest, Sha256};
        hex_encode(Sha256::digest(payload).as_slice())
    };
    let body = serde_json::json!({
        "version": 1,
        "bundle_name": "bundle-meta",
        "request": {
            "request_id": "req_different",
            "action_id": EXPORT_ACTION,
            "owner_principal_id": "owner",
            "grant_id": "grant-seed",
            "requested_at": "2020-01-01T00:00:00Z"
        },
        "terminal_ledger_baseline": {
            "continuity_id": "cont1",
            "sequence": 0,
            "erased_counterparty_ids": [],
            "ledger_hmac_sha256": "00".repeat(32)
        },
        "entries": [
            {"type": "directory", "path": "data"},
            {
                "type": "regular_file",
                "path": "data/kernel.db",
                "byte_length": payload.len() as u64,
                "sha256": digest
            }
        ]
    });
    let body_bytes = serde_json::to_vec(&body).unwrap();
    let mac = hmac_sha256::HMAC::mac(&body_bytes, KEY);
    let manifest = serde_json::json!({
        "body": body,
        "hmac_sha256": hex_encode(&mac)
    });
    fs::write(
        bundle_path.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let err = ops.process_pre_open(false, Timestamp::now()).unwrap_err();
    assert!(
        matches!(err, OverlayOperationError::Bundle(_)),
        "export recovery with mismatched request_id must be rejected, got {err}"
    );
    drop(ops);

    // 2. Restore authorization is distinct from the authenticated source request.
    // A different current principal must not be compared directly with export metadata.
    let (_root3, data3) = temp_data_root();
    {
        let ops3 = acquire(&data3, KEY).unwrap();
        let _id3 = seed_export(&ops3, "bundle-owner-test");
        let pending3 = ops3
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();
        finalize_ok(&ops3, pending3);
    }

    let ops4 = acquire(&data3, KEY).unwrap();
    let staged = ops4
        .stage_export_or_restore(
            &grant("other_user", RESTORE_ACTION),
            &ActionId::new(RESTORE_ACTION),
            "bundle-owner-test",
            Timestamp::now(),
        )
        .unwrap();
    assert_eq!(staged.owner_principal_id(), "other_user");
    let pending4 = ops4
        .process_pre_open(false, Timestamp::now())
        .expect("authenticated source request must verify")
        .expect("restore pending");
    assert_eq!(pending4.owner_principal_id, "other_user");
    finalize_ok(&ops4, pending4);
}

#[test]
fn regression_export_post_rename_fsync_retry() {
    let (_root, data) = temp_data_root();
    let id = {
        let ops = acquire(&data, KEY).unwrap();
        let id = seed_export(&ops, "export-retry");
        let pending = ops
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();
        assert_eq!(pending.request_id, id);
        id
    };

    // Simulate process restart before finalization: release the old controller,
    // then reacquire and call pre-open.
    let ops2 = acquire(&data, KEY).unwrap();
    let pending2 = ops2
        .process_pre_open(false, Timestamp::now())
        .unwrap()
        .expect("pending export on retry");
    assert_eq!(pending2.request_id, id);
    assert_eq!(pending2.outcome, FinalizationOutcome::Completed);
    finalize_ok(&ops2, pending2);
}

#[test]
fn regression_renamed_restore_bundle() {
    let (_root, data) = temp_data_root();
    {
        let ops = acquire(&data, KEY).unwrap();
        let _id = seed_export(&ops, "bundle-orig");
        let pending = ops
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();
        finalize_ok(&ops, pending);

        // Rename snapshot bundle-orig to bundle-renamed
        let orig = ops.snapshots_root().join("bundle-orig");
        let renamed = ops.snapshots_root().join("bundle-renamed");
        fs::create_dir_all(&renamed).unwrap();
        fs::copy(orig.join("manifest.json"), renamed.join("manifest.json")).unwrap();
        copy_dir(&orig.join("data"), &renamed.join("data"));
        drop(ops);
    }

    // Stage restore targeting "bundle-renamed"
    let ops2 = acquire(&data, KEY).unwrap();
    ops2.initialize_terminal_ledger().unwrap();
    let err = ops2
        .stage_export_or_restore(
            &grant("owner", RESTORE_ACTION),
            &ActionId::new(RESTORE_ACTION),
            "bundle-renamed",
            Timestamp::now(),
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            ControlError::AuthenticationFailed | ControlError::NotDirectory(_)
        ),
        "renamed bundle directory with mismatched manifest name must be rejected, got {err}"
    );
    assert!(
        !ops2.control_root().join(OPERATION_FILE).exists(),
        "failed request-time rename check must leave no pending marker"
    );
}

#[test]
fn regression_rollback_flag_at_export_requested_staged_clean() {
    let (_root, data) = temp_data_root();

    // Case 1: Rollback flag on pending Export -> rejected via process_pre_open
    let ops = acquire(&data, KEY).unwrap();
    let _id = seed_export(&ops, "export-rb");
    let err = ops.process_pre_open(true, Timestamp::now()).unwrap_err();
    assert!(matches!(
        err,
        OverlayOperationError::UnrecoverableStage { .. }
    ));

    // Case 2: Rollback flag on Restore in Requested stage -> rejected via process_pre_open
    let (_root2, data2) = temp_data_root();
    {
        let ops = acquire(&data2, KEY).unwrap();
        let _ = seed_export(&ops, "bundle-rb2");
        let p = ops
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();
        finalize_ok(&ops, p);
    }
    let ops2 = acquire(&data2, KEY).unwrap();
    ops2.initialize_terminal_ledger().unwrap();
    ops2.stage_export_or_restore(
        &grant("owner", RESTORE_ACTION),
        &ActionId::new(RESTORE_ACTION),
        "bundle-rb2",
        Timestamp::now(),
    )
    .unwrap();
    let err2 = ops2.process_pre_open(true, Timestamp::now()).unwrap_err();
    assert!(matches!(
        err2,
        OverlayOperationError::UnrecoverableStage { .. }
    ));

    // Case 3: Rollback flag on Restore in Staged stage -> rejected via process_pre_open
    let (_root3, data3) = temp_data_root();
    {
        let ops = acquire(&data3, KEY).unwrap();
        let _ = seed_export(&ops, "bundle-staged-rb");
        let p = ops
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();
        finalize_ok(&ops, p);
    }
    let ops3 = acquire(&data3, KEY).unwrap();
    ops3.initialize_terminal_ledger().unwrap();
    let staged_op = ops3
        .stage_export_or_restore(
            &grant("owner", RESTORE_ACTION),
            &ActionId::new(RESTORE_ACTION),
            "bundle-staged-rb",
            Timestamp::now(),
        )
        .unwrap();
    let staged_id = staged_op.request_id().to_owned();
    ops3.transition_operation(&staged_id, OperationStage::Staged)
        .unwrap();
    let err3 = ops3.process_pre_open(true, Timestamp::now()).unwrap_err();
    assert!(matches!(
        err3,
        OverlayOperationError::UnrecoverableStage { .. }
    ));

    // Case 4: Rollback flag on Restore when InstallState::Clean -> rejected via process_pre_open
    let (_root4, data4) = temp_data_root();
    {
        let ops = acquire(&data4, KEY).unwrap();
        let _ = seed_export(&ops, "bundle-clean-rb");
        let p = ops
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();
        finalize_ok(&ops, p);
    }
    let ops4 = acquire(&data4, KEY).unwrap();
    ops4.initialize_terminal_ledger().unwrap();
    let clean_op = ops4
        .stage_export_or_restore(
            &grant("owner", RESTORE_ACTION),
            &ActionId::new(RESTORE_ACTION),
            "bundle-clean-rb",
            Timestamp::now(),
        )
        .unwrap();
    let _clean_id = clean_op.request_id().to_owned();
    let p4 = ops4
        .process_pre_open(false, Timestamp::now())
        .unwrap()
        .unwrap();
    finalize_ok(&ops4, p4);

    // Install state is now Clean. Reach Installed only through legal transitions.
    // process_pre_open(true) must inspect install state (Clean) and return MissingInstallState.
    let re_staged = ops4
        .stage_export_or_restore(
            &grant("owner", RESTORE_ACTION),
            &ActionId::new(RESTORE_ACTION),
            "bundle-clean-rb",
            Timestamp::now(),
        )
        .unwrap();
    let re_clean_id = re_staged.request_id().to_owned();
    ops4.transition_operation(&re_clean_id, OperationStage::Staged)
        .unwrap();
    ops4.transition_operation(&re_clean_id, OperationStage::Installed)
        .unwrap();
    let err_clean = ops4.process_pre_open(true, Timestamp::now()).unwrap_err();
    assert!(matches!(
        err_clean,
        OverlayOperationError::MissingInstallState
    ));
}

#[test]
fn regression_previous_only_recovery_without_root_recreation() {
    let root = tempfile::TempDir::new().unwrap();
    let live = root.path().join("data");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("kernel.db"), b"pre-export").unwrap();
    fs::create_dir_all(live.join("keys")).unwrap();

    // Publish a restore source bundle, then stage a restore and advance only to Staged.
    let request_id = {
        let ops = acquire(&live, KEY).unwrap();
        let _ = seed_export(&ops, "bundle-prev-only");
        finalize_ok(
            &ops,
            ops.process_pre_open(false, Timestamp::now())
                .unwrap()
                .unwrap(),
        );
        ops.initialize_terminal_ledger().unwrap();
        let staged = ops
            .stage_export_or_restore(
                &grant("owner", RESTORE_ACTION),
                &ActionId::new(RESTORE_ACTION),
                "bundle-prev-only",
                Timestamp::now(),
            )
            .unwrap();
        let request_id = staged.request_id().to_owned();
        ops.transition_operation(&request_id, OperationStage::Staged)
            .unwrap();
        request_id
    };

    // Crash shape: live gone mid-install, staged + previous both present.
    // process_pre_open must recover via install_or_recover, not first-boot root creation.
    let staged = install::staged_path(&live, &request_id);
    let previous = install::previous_path(&live, &request_id);
    if live.exists() {
        fs::remove_dir_all(&live).unwrap();
    }
    fs::create_dir_all(&staged).unwrap();
    fs::write(staged.join("kernel.db"), b"new-state").unwrap();
    fs::create_dir_all(&previous).unwrap();
    fs::write(previous.join("kernel.db"), b"old-state").unwrap();
    assert!(!live.exists());
    assert_eq!(
        install::inspect_install(&live, &request_id).unwrap(),
        InstallState::PreviousOnly
    );

    let ops = acquire(&live, KEY).unwrap();
    assert!(!live.exists(), "acquire must not create missing live root");
    let pending = ops
        .process_pre_open(false, Timestamp::now())
        .expect("previous-only signed pending must recover")
        .expect("restore pending finalization");
    assert_eq!(pending.outcome, FinalizationOutcome::Completed);
    assert!(live.exists());
    assert_eq!(
        fs::read_to_string(live.join("kernel.db")).unwrap(),
        "new-state",
        "previous-only recovery must rename staged tree into live, not recreate an empty root"
    );
    assert_eq!(
        install::inspect_install(&live, &request_id).unwrap(),
        InstallState::Swapped
    );
    finalize_ok(&ops, pending);
}

#[test]
fn regression_finalization_cleanup_retry() {
    let (_root, data) = temp_data_root();
    {
        let ops = acquire(&data, KEY).unwrap();
        let _ = seed_export(&ops, "bundle-retry-fin");
        let p = ops
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();
        finalize_ok(&ops, p);
    }
    let ops = acquire(&data, KEY).unwrap();
    ops.initialize_terminal_ledger().unwrap();
    ops.stage_export_or_restore(
        &grant("owner", RESTORE_ACTION),
        &ActionId::new(RESTORE_ACTION),
        "bundle-retry-fin",
        Timestamp::now(),
    )
    .unwrap();
    let pending = ops
        .process_pre_open(false, Timestamp::now())
        .unwrap()
        .unwrap();

    let meta = ops.begin_finalization(&pending, Timestamp::now()).unwrap();
    ops.complete_finalization(&meta).unwrap();

    // Finalization is complete and install is clean.
    // Re-running cleanup_install on clean state succeeds cleanly without claiming rollback.
    install::cleanup_install(&data, &pending.request_id).unwrap();
    assert_eq!(
        inspect_install(&data, &pending.request_id).unwrap(),
        InstallState::Clean
    );
}

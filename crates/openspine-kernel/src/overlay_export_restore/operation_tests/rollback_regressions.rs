use super::super::control::{OperationStage, RESTORE_ACTION};
use super::super::install::{self, inspect_install, InstallState};
use super::super::operation::acquire;
use super::super::types::{FinalizationOutcome, OverlayOperationError};
use super::{finalize_ok, grant, marker, seed_export, temp_data_root, KEY};
use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use std::fs;
use ulid::Ulid;

#[test]
fn regression_rollback_crash_at_rename_states_and_persistence_boundary() {
    let (_root, data) = temp_data_root();
    {
        let ops = acquire(&data, KEY).unwrap();
        let _ = seed_export(&ops, "bundle-rb-crash");
        let p = ops
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();
        finalize_ok(&ops, p);
    }
    fs::write(data.join("kernel.db"), b"original-data").unwrap();

    // 1. Crash at Swapped with RollbackRequested.
    let request_id = {
        let ops = acquire(&data, KEY).unwrap();
        ops.initialize_terminal_ledger().unwrap();
        let staged = ops
            .stage_export_or_restore(
                &grant("owner", RESTORE_ACTION),
                &ActionId::new(RESTORE_ACTION),
                "bundle-rb-crash",
                Timestamp::now(),
            )
            .unwrap();
        let request_id = staged.request_id().to_owned();
        let _pending = ops
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();
        assert_eq!(
            inspect_install(&data, &request_id).unwrap(),
            InstallState::Swapped
        );
        ops.transition_operation(&request_id, OperationStage::RollbackRequested)
            .unwrap();
        request_id
    };

    // Simulate restart after crash in RollbackRequested while layout is Swapped.
    {
        let ops = acquire(&data, KEY).unwrap();
        let pending = ops
            .process_pre_open(true, Timestamp::now())
            .unwrap()
            .expect("rollback pending");
        assert_eq!(pending.outcome, FinalizationOutcome::RolledBack);
        assert_eq!(marker(&data), "original-data");
        // Staged must still exist before complete_finalization.
        let staged = install::staged_path(&data, &request_id);
        assert!(staged.exists());
        finalize_ok(&ops, pending);
        assert!(!staged.exists());
        assert_eq!(
            inspect_install(&data, &request_id).unwrap(),
            InstallState::Clean
        );
    }

    // 2. Crash at PreviousOnly with RollbackRequested.
    fs::write(data.join("kernel.db"), b"original-data-2").unwrap();
    let request_id_2 = {
        let ops = acquire(&data, KEY).unwrap();
        ops.initialize_terminal_ledger().unwrap();
        let staged = ops
            .stage_export_or_restore(
                &grant("owner", RESTORE_ACTION),
                &ActionId::new(RESTORE_ACTION),
                "bundle-rb-crash",
                Timestamp::now(),
            )
            .unwrap();
        let request_id = staged.request_id().to_owned();
        let _pending = ops
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();

        // Simulate crash mid-rollback: live moved to staged, live missing, previous present.
        let staged = install::staged_path(&data, &request_id);
        let _previous = install::previous_path(&data, &request_id);
        fs::rename(&data, &staged).unwrap();
        assert_eq!(
            inspect_install(&data, &request_id).unwrap(),
            InstallState::PreviousOnly
        );
        ops.transition_operation(&request_id, OperationStage::RollbackRequested)
            .unwrap();
        request_id
    };

    {
        let ops = acquire(&data, KEY).unwrap();
        let pending = ops
            .process_pre_open(true, Timestamp::now())
            .unwrap()
            .expect("rollback pending from PreviousOnly");
        assert_eq!(pending.outcome, FinalizationOutcome::RolledBack);
        assert_eq!(marker(&data), "original-data-2");
        let staged = install::staged_path(&data, &request_id_2);
        assert!(staged.exists());
        finalize_ok(&ops, pending);
        assert!(!staged.exists());
    }

    // 3. Crash after RolledBack is persisted.
    fs::write(data.join("kernel.db"), b"original-data-3").unwrap();
    let request_id_3 = {
        let ops = acquire(&data, KEY).unwrap();
        ops.initialize_terminal_ledger().unwrap();
        let staged = ops
            .stage_export_or_restore(
                &grant("owner", RESTORE_ACTION),
                &ActionId::new(RESTORE_ACTION),
                "bundle-rb-crash",
                Timestamp::now(),
            )
            .unwrap();
        let request_id = staged.request_id().to_owned();
        let _pending = ops
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();

        ops.transition_operation(&request_id, OperationStage::RollbackRequested)
            .unwrap();
        install::rollback_or_recover(&data, &request_id).unwrap();
        ops.transition_operation(&request_id, OperationStage::RolledBack)
            .unwrap();
        request_id
    };

    {
        let ops = acquire(&data, KEY).unwrap();
        let pending = ops
            .process_pre_open(true, Timestamp::now())
            .unwrap()
            .expect("already rolled back pending");
        assert_eq!(pending.outcome, FinalizationOutcome::RolledBack);
        assert_eq!(marker(&data), "original-data-3");
        let staged = install::staged_path(&data, &request_id_3);
        assert!(staged.exists());
        finalize_ok(&ops, pending);
        assert!(!staged.exists());
    }
}

#[test]
fn regression_cleanup_rename_durable_boundary_and_refusal() {
    let (_root, data) = temp_data_root();
    {
        let ops = acquire(&data, KEY).unwrap();
        let _ = seed_export(&ops, "bundle-cleanup-bound");
        let p = ops
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();
        finalize_ok(&ops, p);
    }
    fs::write(data.join("kernel.db"), b"pre-restore").unwrap();

    // 1. Pre-rename rollback request while `previous` exists MUST succeed.
    let request_id_1 = {
        let ops = acquire(&data, KEY).unwrap();
        ops.initialize_terminal_ledger().unwrap();
        let staged = ops
            .stage_export_or_restore(
                &grant("owner", RESTORE_ACTION),
                &ActionId::new(RESTORE_ACTION),
                "bundle-cleanup-bound",
                Timestamp::now(),
            )
            .unwrap();
        let request_id = staged.request_id().to_owned();
        let _pending = ops
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();

        assert_eq!(
            inspect_install(&data, &request_id).unwrap(),
            InstallState::Swapped
        );
        let previous = install::previous_path(&data, &request_id);
        assert!(previous.exists());

        // Rollback before cleanup rename must succeed.
        let rolled = ops
            .process_pre_open(true, Timestamp::now())
            .unwrap()
            .expect("rollback before cleanup rename must succeed");
        assert_eq!(rolled.outcome, FinalizationOutcome::RolledBack);
        assert_eq!(marker(&data), "pre-restore");

        let meta = ops.begin_finalization(&rolled, Timestamp::now()).unwrap();
        assert_eq!(meta.outcome, FinalizationOutcome::RolledBack);
        ops.complete_finalization(&meta).unwrap();
        assert_eq!(
            inspect_install(&data, &request_id).unwrap(),
            InstallState::Clean
        );
        request_id
    };
    let _ = request_id_1;

    // 2. Post-rename rollback request after `previous` renamed to `cleanup` MUST refuse.
    fs::write(data.join("kernel.db"), b"pre-restore-2").unwrap();
    let ops = acquire(&data, KEY).unwrap();
    ops.initialize_terminal_ledger().unwrap();
    let staged = ops
        .stage_export_or_restore(
            &grant("owner", RESTORE_ACTION),
            &ActionId::new(RESTORE_ACTION),
            "bundle-cleanup-bound",
            Timestamp::now(),
        )
        .unwrap();
    let request_id_2 = staged.request_id().to_owned();
    let pending = ops
        .process_pre_open(false, Timestamp::now())
        .unwrap()
        .unwrap();

    assert_eq!(
        inspect_install(&data, &request_id_2).unwrap(),
        InstallState::Swapped
    );

    // Simulate durable rename of previous -> cleanup sibling during finalization.
    let previous = install::previous_path(&data, &request_id_2);
    let cleanup = install::cleanup_path(&data, &request_id_2);
    fs::rename(&previous, &cleanup).unwrap();

    assert_eq!(
        inspect_install(&data, &request_id_2).unwrap(),
        InstallState::CleanupCommitted
    );

    // Rollback MUST be explicitly refused once CleanupCommitted is reached.
    let res = ops.process_pre_open(true, Timestamp::now());
    assert!(
        matches!(res, Err(OverlayOperationError::MissingInstallState)),
        "rollback after durable cleanup rename must be refused: {res:?}"
    );

    // Complete finalization cleanup idempotently from CleanupCommitted.
    install::cleanup_install(&data, &request_id_2).unwrap();
    assert_eq!(
        inspect_install(&data, &request_id_2).unwrap(),
        InstallState::Clean
    );
    assert!(!cleanup.exists());

    let meta = ops.begin_finalization(&pending, Timestamp::now()).unwrap();
    ops.complete_finalization(&meta).unwrap();
}

#[test]
fn regression_never_install_partially_deleted_prior_tree() {
    let (_root, data) = temp_data_root();
    let request_id = Ulid::new().to_string();

    // Partial/corrupt state: live is missing, staged is missing, previous is present.
    fs::remove_dir_all(&data).unwrap();
    let previous = install::previous_path(&data, &request_id);
    fs::create_dir_all(&previous).unwrap();

    // inspect_install must classify ambiguous state, not Clean or Swapped.
    let state = inspect_install(&data, &request_id).unwrap();
    assert_eq!(state, InstallState::Ambiguous);

    let res = install::install_or_recover(&data, &request_id);
    assert!(
        matches!(res, Err(OverlayOperationError::AmbiguousInstallState)),
        "install_or_recover must refuse partial/ambiguous prior tree: {res:?}"
    );
}

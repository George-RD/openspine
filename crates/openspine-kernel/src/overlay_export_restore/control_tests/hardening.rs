use super::*;

#[test]
fn missing_live_root_remains_absent_and_acquire_does_not_mutate() {
    let root = TempDir::new().unwrap();
    let missing_data = root.path().join("missing-data");
    let control = OverlayControl::acquire(&missing_data, KEY).unwrap();
    assert!(
        !missing_data.exists(),
        "acquire must not create missing live root"
    );
    assert!(matches!(
        control.initialize_terminal_ledger(),
        Err(ControlError::MissingContinuity)
    ));
    assert!(
        !missing_data.exists(),
        "failed initialization must leave root absent"
    );
    control.ensure_data_root_for_first_boot().unwrap();
    assert!(
        missing_data.exists(),
        "explicit first boot creates data root"
    );
    assert!(control
        .canonical_data_root()
        .join(generation::MARKER_FILE)
        .exists());
}

#[test]
fn lock_symlink_is_rejected() {
    let root = TempDir::new().unwrap();
    let data = root.path().join("data");
    fs::create_dir_all(&data).unwrap();
    let identity = acquire::resolve_root_identity(&data).unwrap();
    fs::create_dir_all(&identity.control_root).unwrap();
    let target = root.path().join("outside-lock");
    fs::write(&target, b"lock-target").unwrap();
    symlink(&target, identity.control_root.join("lifetime.lock")).unwrap();
    assert!(matches!(
        OverlayControl::acquire(&data, KEY),
        Err(ControlError::Io { .. } | ControlError::UnsafeControlPath(_))
    ));
}

#[test]
fn snapshot_staging_validates_absence_and_bundle_type_under_lock() {
    let (_root, data) = temp_data_root();
    let control = OverlayControl::acquire(&data, KEY).unwrap();
    let snaps = control.snapshots_root();
    let op_path = control.control_root().join(OPERATION_FILE);

    // Restore missing bundle rejected
    assert!(matches!(
        control.stage_export_or_restore(
            &grant("owner"),
            &ActionId::new(RESTORE_ACTION),
            "missing-bundle",
            Timestamp::now()
        ),
        Err(ControlError::NotDirectory(_))
    ));
    assert!(
        !op_path.exists(),
        "invalid restore staging must not persist pending operation"
    );

    // Restore symlink bundle rejected
    let target = TempDir::new().unwrap();
    let sym = snaps.join("sym-bundle");
    symlink(target.path(), &sym).unwrap();
    assert!(matches!(
        control.stage_export_or_restore(
            &grant("owner"),
            &ActionId::new(RESTORE_ACTION),
            "sym-bundle",
            Timestamp::now()
        ),
        Err(ControlError::NotDirectory(_))
    ));
    assert!(
        !op_path.exists(),
        "symlink restore staging must not persist pending operation"
    );

    // Export existing snapshot rejected
    let existing = snaps.join("existing-export");
    fs::create_dir_all(&existing).unwrap();
    assert!(matches!(
        control.stage_export_or_restore(
            &grant("owner"),
            &ActionId::new(EXPORT_ACTION),
            "existing-export",
            Timestamp::now()
        ),
        Err(ControlError::UnsafeControlPath(_))
    ));
    assert!(
        !op_path.exists(),
        "export over existing snapshot must not persist pending operation"
    );

    // Restore invalid/unauthenticated manifest rejected
    let corrupt = snaps.join("corrupt-bundle");
    fs::create_dir_all(&corrupt).unwrap();
    fs::write(corrupt.join("manifest.json"), b"invalid-manifest-data").unwrap();
    assert!(matches!(
        control.stage_export_or_restore(
            &grant("owner"),
            &ActionId::new(RESTORE_ACTION),
            "corrupt-bundle",
            Timestamp::now()
        ),
        Err(ControlError::AuthenticationFailed)
    ));
    assert!(
        !op_path.exists(),
        "unauthenticated restore manifest must not persist pending operation"
    );
}

#[test]
fn deleting_established_ledger_fails_with_missing_continuity() {
    let (_root, data) = temp_data_root();
    let control = OverlayControl::acquire(&data, KEY).unwrap();
    control.ensure_data_root_for_first_boot().unwrap();
    let ledger_file = control.control_root().join(LEDGER_FILE);
    fs::remove_file(&ledger_file).unwrap();
    assert!(matches!(
        control.export_terminal_ledger(),
        Err(ControlError::MissingContinuity)
    ));
    assert!(matches!(
        control.record_terminal_erasure("cp1"),
        Err(ControlError::MissingContinuity)
    ));
}

#[test]
fn portable_import_aligns_marker() {
    let (_root, data) = temp_data_root();
    let control = OverlayControl::acquire(&data, KEY).unwrap();
    control.ensure_data_root_for_first_boot().unwrap();
    let ledger = control.export_terminal_ledger().unwrap();
    let portable = control.export_portable_continuity().unwrap();

    let fresh_root = TempDir::new().unwrap();
    let fresh_data = fresh_root.path().join("fresh-data");
    let fresh_control = OverlayControl::acquire(&fresh_data, KEY).unwrap();

    let imported = fresh_control.import_terminal_ledger(&portable).unwrap();
    assert_eq!(imported.continuity_id(), ledger.continuity_id());
    assert!(fresh_data.join(generation::MARKER_FILE).exists());
    let marker_id = generation::read_generation_marker(&fresh_data, KEY)
        .unwrap()
        .unwrap();
    assert_eq!(marker_id, ledger.continuity_id());
}

#[test]
fn second_contender_during_previous_only_state_cannot_create_live_root() {
    let root = TempDir::new().unwrap();
    let missing_data = root.path().join("missing-data");
    let req_id = Ulid::new().to_string();
    // PreviousOnly = staged + previous present, live absent.
    let previous_dir =
        crate::overlay_export_restore::install::previous_path(&missing_data, &req_id);
    let staged_dir = crate::overlay_export_restore::install::staged_path(&missing_data, &req_id);
    fs::create_dir_all(&previous_dir).unwrap();
    fs::write(previous_dir.join("kernel.db"), b"old-state").unwrap();
    fs::create_dir_all(&staged_dir).unwrap();
    fs::write(staged_dir.join("kernel.db"), b"staged-state").unwrap();

    assert_eq!(
        crate::overlay_export_restore::install::inspect_install(&missing_data, &req_id).unwrap(),
        crate::overlay_export_restore::install::InstallState::PreviousOnly
    );

    let first = OverlayControl::acquire(&missing_data, KEY).unwrap();
    assert!(!missing_data.exists());

    // Contender acquire blocked by lock and does not recreate live root while previous-only exists
    assert!(matches!(
        OverlayControl::acquire(&missing_data, KEY),
        Err(ControlError::AlreadyLocked(_))
    ));
    assert!(
        !missing_data.exists(),
        "contender must not create missing live root during previous-only state"
    );

    drop(first);
}

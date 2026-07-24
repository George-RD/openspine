use super::*;

#[test]
fn relative_data_roots_normalize_and_preserve_stable_identity() {
    static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    struct DirGuard(PathBuf);
    impl Drop for DirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    let _g = CWD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let orig_cwd = std::env::current_dir().unwrap();
    let _guard = DirGuard(orig_cwd);

    let temp = TempDir::new().unwrap();
    std::env::set_current_dir(temp.path()).unwrap();
    fs::create_dir_all("data").unwrap();

    let control_rel = OverlayControl::acquire(Path::new("data"), KEY).unwrap();
    let root_rel = control_rel.canonical_data_root().to_path_buf();
    drop(control_rel);

    let control_dot = OverlayControl::acquire(Path::new("./data"), KEY).unwrap();
    let root_dot = control_dot.canonical_data_root().to_path_buf();
    drop(control_dot);

    let abs_path = temp.path().join("data");
    let control_abs = OverlayControl::acquire(&abs_path, KEY).unwrap();
    let root_abs = control_abs.canonical_data_root().to_path_buf();
    drop(control_abs);

    assert_eq!(root_rel, root_dot);
    assert_eq!(root_rel, root_abs);

    assert!(matches!(
        OverlayControl::acquire(Path::new("../data"), KEY),
        Err(ControlError::SymlinkDataRoot(_))
    ));
}

#[test]
fn portable_restore_accepts_fresh_destination_owner_id_and_preserves_request() {
    let source_root = TempDir::new().unwrap();
    let source_snaps = source_root.path().join("snaps");
    let source_data = source_root.path().join("data");
    fs::create_dir_all(&source_snaps).unwrap();
    fs::create_dir_all(&source_data).unwrap();
    fs::write(source_data.join("kernel.db"), b"alice-data").unwrap();

    let source_control = OverlayControl::acquire(&source_data, KEY).unwrap();
    let alice_grant = grant("alice_owner");
    let export_op = source_control
        .stage_export_or_restore(
            &alice_grant,
            &ActionId::new(EXPORT_ACTION),
            "bundle-alice",
            Timestamp::now(),
        )
        .unwrap();

    let ledger = source_control.initialize_terminal_ledger().unwrap();
    let source_req = crate::overlay_export_restore::bundle::BundleRequestMetadata {
        request_id: export_op.request_id().to_owned(),
        action_id: EXPORT_ACTION.into(),
        owner_principal_id: "alice_owner".into(),
        grant_id: alice_grant.id.to_string(),
        requested_at: export_op.requested_at().to_owned(),
    };
    let baseline = crate::overlay_export_restore::bundle::TerminalLedgerBaseline {
        continuity_id: ledger.continuity_id().to_owned(),
        sequence: ledger.sequence(),
        erased_counterparty_ids: ledger.erased_counterparty_ids().iter().cloned().collect(),
        ledger_hmac_sha256: ledger.hmac_sha256().to_owned(),
    };
    let _manifest = crate::overlay_export_restore::bundle::publish_bundle(
        source_control.snapshots_root(),
        "bundle-alice",
        source_control.canonical_data_root(),
        source_req.clone(),
        baseline,
        KEY,
    )
    .unwrap();

    let dest_root = TempDir::new().unwrap();
    let dest_data = dest_root.path().join("data");
    fs::create_dir_all(&dest_data).unwrap();

    let dest_control = OverlayControl::acquire(&dest_data, KEY).unwrap();
    fs::create_dir_all(dest_control.snapshots_root()).unwrap();
    let bundle_src = source_control.snapshots_root().join("bundle-alice");
    let bundle_dst = dest_control.snapshots_root().join("bundle-alice");
    fs::create_dir_all(&bundle_dst).unwrap();
    for entry in fs::read_dir(&bundle_src).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            let sub = bundle_dst.join(entry.file_name());
            fs::create_dir_all(&sub).unwrap();
            for file in fs::read_dir(entry.path()).unwrap() {
                let file = file.unwrap();
                fs::copy(file.path(), sub.join(file.file_name())).unwrap();
            }
        } else {
            fs::copy(entry.path(), bundle_dst.join(entry.file_name())).unwrap();
        }
    }

    let bob_grant = grant("bob_destination_owner");
    let restore_op = dest_control
        .stage_export_or_restore(
            &bob_grant,
            &ActionId::new(RESTORE_ACTION),
            "bundle-alice",
            Timestamp::now(),
        )
        .unwrap();

    assert_eq!(restore_op.owner_principal_id(), "bob_destination_owner");
    let saved_source_req = restore_op.source_bundle_request().unwrap();
    assert_eq!(
        saved_source_req,
        &OperationAuthorization {
            action_id: EXPORT_ACTION.into(),
            owner_principal_id: "alice_owner".into(),
            grant_id: alice_grant.id.to_string(),
            request_id: export_op.request_id().to_owned(),
            requested_at: export_op.requested_at().to_owned(),
        }
    );
}

#[test]
fn first_ledger_initialization_recovers_both_crash_states_and_enforces_ledger_first_write() {
    let root = TempDir::new().unwrap();
    let data = root.path().join("data");
    fs::create_dir_all(&data).unwrap();

    let control = OverlayControl::acquire(&data, KEY).unwrap();
    let marker_path = data.join(generation::MARKER_FILE);
    let ledger_path = control.control_root().join(LEDGER_FILE);

    assert!(!marker_path.exists());
    assert!(!ledger_path.exists());

    // Crash state (None, Some): ledger exists, marker absent.
    let init_ledger = control
        .sign_ledger(TerminalLedger::with_continuity_id(
            Ulid::new().to_string(),
            0,
            BTreeSet::new(),
        ))
        .unwrap();
    let canonical = serde_json::to_vec(&init_ledger).unwrap();
    fs::write(&ledger_path, &canonical).unwrap();
    assert!(!marker_path.exists());

    control.ensure_data_root_for_first_boot().unwrap();
    assert!(marker_path.exists());
    let marker_id = generation::read_generation_marker(&data, KEY)
        .unwrap()
        .unwrap();
    assert_eq!(marker_id, init_ledger.continuity_id());

    // Clean reset for failpoint order verification.
    fs::remove_file(&marker_path).unwrap();
    fs::remove_file(&ledger_path).unwrap();

    control
        .fail_before_init_ledger_marker
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(control.initialize_terminal_ledger().is_err());
    assert!(
        ledger_path.exists(),
        "durable ledger must exist after failpoint"
    );
    assert!(
        !marker_path.exists(),
        "marker must remain absent at failpoint"
    );

    // Retry completes generation marker matching durable ledger.
    let init = control.initialize_terminal_ledger().unwrap();
    assert!(marker_path.exists());
    assert_eq!(
        generation::read_generation_marker(&data, KEY)
            .unwrap()
            .unwrap(),
        init.continuity_id()
    );

    // Failpoint for first boot initialization path.
    fs::remove_file(&marker_path).unwrap();
    fs::remove_file(&ledger_path).unwrap();

    control
        .fail_before_first_boot_marker
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(control.ensure_data_root_for_first_boot().is_err());
    assert!(
        ledger_path.exists(),
        "durable ledger must exist at first boot failpoint"
    );
    assert!(
        !marker_path.exists(),
        "marker must remain absent at first boot failpoint"
    );

    control.ensure_data_root_for_first_boot().unwrap();
    assert!(marker_path.exists());

    // Explicit check that (Some, None) is rejected as missing continuity.
    fs::remove_file(&ledger_path).unwrap();
    assert!(matches!(
        control.export_terminal_ledger(),
        Err(ControlError::MissingContinuity)
    ));
}

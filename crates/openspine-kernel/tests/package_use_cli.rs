//! Owner-reviewed package selection through the real binary and a real PTY.
#![cfg(any(target_os = "linux", target_os = "macos"))]
// The shared review fixture carries helpers this file does not use.
#![allow(dead_code)]

use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

#[path = "package_use/helpers.rs"]
mod helpers;
#[path = "package_use/pty.rs"]
mod pty;
#[path = "package_review/support.rs"]
mod support;
use helpers::*;
use support::{assert_success, database_rows, files, Fixture};

#[test]
fn use_commits_after_typed_consent_and_a_fresh_process_sees_the_exact_selection() {
    let fixture = Fixture::new();
    let original = fixture.install();
    let variant = install_variant(&fixture, 3);
    let target = id(&variant);
    let key_files = |fixture: &Fixture| {
        let keys = fixture.root.path().join("data/keys");
        keys.is_dir().then(|| files(&keys))
    };
    let keys = key_files(&fixture);
    let env_before = fs::read(fixture.root.path().join("openspine.env")).unwrap();
    let protected = protected_rows(&fixture);
    let chain = audit_chain(&fixture);

    let finished = select(&fixture, &target);
    assert_eq!(finished.code, Some(0), "{}", finished.output);
    assert!(
        finished.output.contains("Selected installation"),
        "{}",
        finished.output
    );
    assert_eq!(selection(&fixture), Some((1, target.clone(), 1)));
    assert_eq!(
        count(&fixture, "SELECT COUNT(*) FROM package_selection_history"),
        1
    );

    // Owner binding, keys, package receipts and every earlier audit row are
    // exactly as they were; the receipts are appended after them.
    assert_eq!(protected_rows(&fixture), protected);
    assert_eq!(key_files(&fixture), keys);
    assert_eq!(
        fs::read(fixture.root.path().join("openspine.env")).unwrap(),
        env_before
    );
    assert_eq!(audit_chain(&fixture)[..chain.len()], chain[..]);

    // A restarted process resolves the managed selection, not `lyra_dir`, even
    // after the legacy tree disappears.
    fs::remove_dir_all(fixture.root.path().join("artifacts/lyra")).unwrap();
    let listing: Value =
        serde_json::from_slice(&fixture.run(&["package", "list", "--json"]).stdout).unwrap();
    for package in listing["packages"].as_array().unwrap() {
        assert_eq!(
            package["selected"],
            package["receipt"]["installation_id"] == target.as_str()
        );
    }
    let review = fixture.review(&original);
    assert_eq!(review["source"]["mode"], "managed");
    assert_eq!(review["source"]["installation_id"], target.as_str());
    assert_eq!(review["source"]["selection_generation"], 1);
    assert_eq!(
        review["source"]["identity"]["content_digest"],
        variant["receipt"]["content_digest"]
    );
    assert_eq!(review["blockers"], serde_json::json!([]));
}

#[test]
fn cancel_eof_a_wrong_phrase_noninteractive_and_yes_flags_select_nothing() {
    let fixture = Fixture::new();
    fixture.install();
    let target = id(&install_variant(&fixture, 3));

    let mut eof = consent_session(&fixture, &target);
    eof.wait_for(PROMPT);
    eof.send_eof();
    let eof = eof.finish();
    assert_ne!(eof.code, Some(0), "{}", eof.output);
    assert!(
        eof.output.contains("Consent was not given"),
        "{}",
        eof.output
    );

    for answer in [
        "yes",
        "y",
        "select",
        &format!("select  {target}"),
        &format!("SELECT {target}"),
    ] {
        let mut session = consent_session(&fixture, &target);
        session.wait_for(PROMPT);
        session.send_line(answer);
        let finished = session.finish();
        assert_ne!(finished.code, Some(0), "{answer}: {}", finished.output);
        assert!(
            finished.output.contains("Consent was not given"),
            "{}",
            finished.output
        );
    }

    // No terminal: refused before anything is opened.
    let files_before = files(fixture.root.path());
    let rows_before = database_rows(&fixture);
    let piped = fixture
        .command(&["use", &target])
        .stdin(std::process::Stdio::piped())
        .output()
        .unwrap();
    assert!(!piped.status.success());
    assert!(String::from_utf8_lossy(&piped.stderr).contains("interactive terminal"));
    // An unattended flag is not accepted, with or without a terminal.
    for args in [
        vec!["use", target.as_str(), "--yes"],
        vec!["use", "--yes"],
        vec!["use", "-y", target.as_str()],
    ] {
        assert!(!fixture.run(&args).status.success(), "{args:?}");
    }
    assert_eq!(files(fixture.root.path()), files_before);
    assert_eq!(database_rows(&fixture), rows_before);

    assert_eq!(selection(&fixture), None);
    assert_eq!(
        count(&fixture, "SELECT COUNT(*) FROM package_selection_history"),
        0
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT COUNT(*) FROM audit_log WHERE kind = 'package.selection_committed'"
        ),
        0
    );
}

#[test]
fn a_foreign_owner_or_unprotected_instance_is_refused_before_anything_changes() {
    let fixture = Fixture::new();
    fixture.install();
    let target = id(&install_variant(&fixture, 3));
    let rows = database_rows(&fixture);
    let files_before = files(fixture.root.path());
    let refused = |fixture: &Fixture, expected: &str| {
        let finished = pty::Session::spawn(fixture.command(&["use", &target])).finish();
        assert_ne!(finished.code, Some(0), "{}", finished.output);
        assert!(finished.output.contains(expected), "{}", finished.output);
        assert!(!finished.output.contains(PROMPT), "{}", finished.output);
    };

    // The configured owner is not the persisted owner.
    let config = fixture.root.path().join("openspine.yaml");
    let original = fs::read_to_string(&config).unwrap();
    fs::write(&config, original.replace("987654321", "111222333")).unwrap();
    refused(&fixture, "persisted owner could not be verified");
    fs::write(&config, &original).unwrap();

    use std::os::unix::fs::PermissionsExt as _;
    let mode = |path: &Path, bits: u32| {
        fs::set_permissions(path, fs::Permissions::from_mode(bits)).unwrap()
    };
    // Another account could write the protected state.
    let data = fixture.root.path().join("data");
    let data_mode = fs::metadata(&data).unwrap().permissions().mode() & 0o7777;
    mode(&data, 0o777);
    refused(&fixture, "owned and writable only by this account");
    mode(&data, data_mode);
    let config_mode = fs::metadata(&config).unwrap().permissions().mode() & 0o7777;
    mode(&config, 0o666);
    refused(&fixture, "owned and writable only by this account");
    mode(&config, config_mode);
    // The key file is readable by others.
    let env = fixture.root.path().join("openspine.env");
    mode(&env, 0o644);
    refused(&fixture, "owned and writable only by this account");
    mode(&env, 0o600);
    // A recorded erasure the signed ledger does not carry cannot be reconciled
    // by maintenance, so selection refuses rather than proceeding.
    db(&fixture)
        .execute(
            "INSERT INTO erased_counterparties (counterparty_id, erased_at) VALUES ('01ARZ3NDEKTSV4RRFFQ69G5FAV', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    refused(&fixture, "signed erasure ledger does not match");
    db(&fixture)
        .execute("DELETE FROM erased_counterparties", [])
        .unwrap();
    // A linked data directory is never followed.
    let link = fixture.root.path().join("data-link");
    std::os::unix::fs::symlink(&data, &link).unwrap();
    fs::write(
        &config,
        original.replace(
            &format!("data_dir: {}", data.display()),
            &format!("data_dir: {}", link.display()),
        ),
    )
    .unwrap();
    refused(&fixture, "a protected path is a link");
    fs::write(&config, &original).unwrap();
    fs::remove_file(&link).unwrap();

    assert_eq!(database_rows(&fixture), rows);
    let mut after = files(fixture.root.path());
    let mut before = files_before;
    after.retain(|path, _| path.file_name().is_none_or(|name| name != "data-link"));
    before.retain(|path, _| path.file_name().is_none_or(|name| name != "data-link"));
    assert_eq!(after, before);
    assert_eq!(selection(&fixture), None);
}

#[test]
fn a_state_change_after_consent_makes_the_review_stale() {
    let fixture = Fixture::new();
    fixture.install();
    let target = id(&install_variant(&fixture, 3));
    let barrier = tempfile::tempdir().unwrap();
    let mut command = fixture.command(&["use", &target]);
    command
        .env("OPENSPINE_TEST_PACKAGE_PAUSE", "use-after-consent")
        .env("OPENSPINE_TEST_PACKAGE_BARRIER", barrier.path());
    let mut session = pty::Session::spawn(command);
    session.wait_for(PROMPT);
    session.send_line(&format!("select {target}"));
    let ready = barrier.path().join("use-after-consent.ready");
    while !ready.exists() {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // The owner state moves after the plan was displayed and consented to.
    db(&fixture)
        .execute(
            "INSERT INTO kv_state (key, value) VALUES ('changed.after.consent', '1')",
            [],
        )
        .unwrap();
    fs::write(barrier.path().join("use-after-consent.release"), b"").unwrap();
    let finished = session.finish();
    if cfg!(debug_assertions) {
        assert_ne!(finished.code, Some(0), "{}", finished.output);
        assert!(
            finished.output.contains("changed after consent (controls)"),
            "{}",
            finished.output
        );
        assert_eq!(selection(&fixture), None);
        assert_eq!(
            count(
                &fixture,
                "SELECT COUNT(*) FROM audit_log WHERE kind = 'package.selection_committed'"
            ),
            0
        );
    }
}

#[test]
fn crashes_on_either_side_of_the_commit_leave_the_old_selection_never_a_mixed_base() {
    for point in ["selection-after-audit", "selection-before-commit"] {
        let fixture = Fixture::new();
        fixture.install();
        let target = id(&install_variant(&fixture, 3));
        let mut command = fixture.command(&["use", &target]);
        command.env("OPENSPINE_TEST_PACKAGE_CRASH", point);
        let mut session = pty::Session::spawn(command);
        session.wait_for(PROMPT);
        session.send_line(&format!("select {target}"));
        let crashed = session.finish();
        if cfg!(debug_assertions) {
            assert_eq!(crashed.code, Some(75), "{point}: {}", crashed.output);
            // Atomic: no selection, receipt, history or consumed decision.
            assert_eq!(selection(&fixture), None, "{point}");
            assert_eq!(
                count(&fixture, "SELECT COUNT(*) FROM package_selection_history"),
                0
            );
            assert_eq!(
                count(
                    &fixture,
                    "SELECT COUNT(*) FROM package_selection_decisions WHERE state = 'consumed'"
                ),
                0
            );
            assert_eq!(
                count(
                    &fixture,
                    "SELECT COUNT(*) FROM audit_log WHERE kind = 'package.selection_committed'"
                ),
                0
            );
            // The ledger still validates and a clean retry commits generation 1.
            assert_success(&fixture.run(&["package", "list", "--json"]));
        } else {
            // Release builds never honor the debug fault hooks.
            assert_eq!(crashed.code, Some(0), "{point}: {}", crashed.output);
        }
        let retried = select(&fixture, &target);
        if cfg!(debug_assertions) {
            assert_eq!(retried.code, Some(0), "{point}: {}", retried.output);
            assert_eq!(selection(&fixture).unwrap().0, 1);
        }
    }
}

#[test]
fn outstanding_work_blocks_selection_without_destroying_anything() {
    let fixture = Fixture::new();
    fixture.install();
    let target = id(&install_variant(&fixture, 3));
    db(&fixture)
        .execute(
            "INSERT INTO kv_state (key, value) VALUES ('secret.intake.pending', 'pending')",
            [],
        )
        .unwrap();
    let rows = database_rows(&fixture);
    let finished = pty::Session::spawn(fixture.command(&["use", &target])).finish();
    assert_ne!(finished.code, Some(0), "{}", finished.output);
    assert!(
        finished.output.contains("outstanding-work"),
        "{}",
        finished.output
    );
    assert!(!finished.output.contains(PROMPT));
    assert_eq!(selection(&fixture), None);
    assert_eq!(
        database_rows(&fixture),
        rows,
        "work census must be non-destructive"
    );
}

#[test]
fn selecting_forward_and_back_advances_generations_and_revives_no_authority() {
    let fixture = Fixture::new();
    let original = fixture.install();
    let variant = install_variant(&fixture, 3);
    // An owner-accepted learned route that is part of the effective overlay.
    let yaml = "id: accepted-route\nschema_version: 1\nversion: 1\nlifecycle_state: active\neffect: allow\n";
    let routes = fixture.root.path().join("data/artifacts.d/routes");
    fs::create_dir_all(&routes).unwrap();
    let path = routes.join("accepted-route.yaml");
    fs::write(&path, yaml).unwrap();
    let digest = {
        use sha2::{Digest as _, Sha256};
        let hash: String = Sha256::digest(yaml.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        format!("sha256:{hash}")
    };
    {
        let db = db(&fixture);
        db.execute(
            "INSERT INTO proposed_artifacts (id, kind, artifact_id, version, state, yaml_digest, task_grant_id, proposed_at)
             VALUES ('01ARZ3NDEKTSV4RRFFQ69G5FAV', 'route', 'accepted-route', 1, 'active', ?1, '01ARZ3NDEKTSV4RRFFQ69G5FAW', '2026-09-09T00:00:00Z')",
            [&digest],
        )
        .unwrap();
        db.execute(
            "INSERT INTO learned_artifacts (kind, artifact_id, version, namespace, provenance, learned_at, compatibility, nomination, pending_yaml_digest, source_path, accepted_base_epoch)
             VALUES ('route', 'accepted-route', 1, 'overlay', '{\"legacy_migration\":{\"discovered_at\":\"2026-09-09T00:00:00Z\"}}',
                     '2026-09-09T00:00:00Z', 'owner_accepted', 'none', ?1, ?2, 'epoch-the-owner-accepted')",
            [&digest, &path.to_string_lossy().to_string()],
        )
        .unwrap();
    }
    let status = |fixture: &Fixture| -> (String, Option<String>) {
        db(fixture)
            .query_row(
                "SELECT compatibility, accepted_base_epoch FROM learned_artifacts WHERE artifact_id = 'accepted-route'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    };
    let protected = protected_rows(&fixture);
    let chain = audit_chain(&fixture);

    let first = select(&fixture, &id(&variant));
    assert_eq!(first.code, Some(0), "{}", first.output);
    assert_eq!(selection(&fixture), Some((1, id(&variant), 1)));
    // The epoch changed: reusable authority returns to reconfirmation, with its
    // recorded acceptance epoch untouched.
    assert_eq!(
        status(&fixture),
        (
            "reconfirmation_required".into(),
            Some("epoch-the-owner-accepted".into())
        )
    );

    let back = select(&fixture, &id(&original));
    assert_eq!(back.code, Some(0), "{}", back.output);
    // A->B->A is generation 2 with a second invalidation; nothing was revived.
    assert_eq!(selection(&fixture), Some((2, id(&original), 2)));
    assert_eq!(status(&fixture).0, "reconfirmation_required");
    assert_eq!(
        count(&fixture, "SELECT COUNT(*) FROM package_selection_history"),
        2
    );
    assert_eq!(protected_rows(&fixture), protected);
    assert_eq!(audit_chain(&fixture)[..chain.len()], chain[..]);
    // Selecting what is already selected is refused as a no-op.
    let same = pty::Session::spawn(fixture.command(&["use", &id(&original)])).finish();
    assert_ne!(same.code, Some(0), "{}", same.output);
    assert_eq!(selection(&fixture).unwrap().0, 2);
}

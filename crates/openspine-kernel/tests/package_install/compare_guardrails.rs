//! Adversarial maintenance boundaries through the real comparison command.
use super::{assert_error, compare, id, Fixture};
use std::fs;
use std::os::fd::AsRawFd;
use std::path::PathBuf;

fn control_root(fixture: &Fixture) -> PathBuf {
    let roots: Vec<_> = fs::read_dir(fixture.root.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("lifetime.lock").is_file())
        .collect();
    assert_eq!(roots.len(), 1);
    roots.into_iter().next().unwrap()
}

#[test]
fn comparison_respects_the_existing_runtime_lifetime_lock() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(control_root(&fixture).join("lifetime.lock"))
        .unwrap();
    // The production controller uses flock on this same descriptor-backed file.
    // Dropping this owned File releases the lock even during assertion unwinding.
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    assert_error(
        &fixture.run(&[
            "package",
            "compare",
            id(&installed),
            id(&installed),
            "--json",
        ]),
        "data-directory-locked",
    );
    drop(lock);
    assert_eq!(compare(&fixture, &installed, &installed)["identical"], true);
}

#[test]
fn pending_authenticated_operation_is_not_executed_or_cleared_by_comparison() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    let environment = fs::read_to_string(fixture.root.path().join("openspine.env")).unwrap();
    let key_hex = environment
        .lines()
        .find_map(|line| line.strip_prefix("OPENSPINE_ARTIFACT_KEY="))
        .unwrap()
        .trim()
        .trim_matches('"')
        .trim_matches('\'');
    assert_eq!(key_hex.len(), 64);
    let key: Vec<u8> = key_hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    // Fixed wire fixture, in PendingOperation/OperationAuthorization field order.
    // Tests pending-state refusal, not permission to admit a new export request.
    let body = r#"{"version":1,"kind":"export","bundle_name":"comparison-fixture","authorization":{"action_id":"openspine.overlay.export","owner_principal_id":"fixture-owner","grant_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","request_id":"01ARZ3NDEKTSV4RRFFQ69G5FAW","requested_at":"2026-09-09T00:00:00Z"},"source_bundle_request":null,"stage":"requested"}"#;
    let mut message = b"openspine.overlay.operation.v1\0".to_vec();
    message.extend_from_slice(body.as_bytes());
    let mac: String = hmac_sha256::HMAC::mac(&message, &key)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let signed = serde_json::json!({
        "body": serde_json::from_str::<serde_json::Value>(body).unwrap(),
        "hmac_sha256": mac,
    });
    let pending = control_root(&fixture).join("pending-operation.json");
    let before = serde_json::to_vec(&signed).unwrap();
    fs::write(&pending, &before).unwrap();
    assert_error(
        &fixture.run(&[
            "package",
            "compare",
            id(&installed),
            id(&installed),
            "--json",
        ]),
        "pending-overlay-operation",
    );
    assert_eq!(fs::read(&pending).unwrap(), before);
    assert!(!control_root(&fixture)
        .join("snapshots/comparison-fixture")
        .exists());
    // An unauthenticated marker is an error too, never permission to continue.
    fs::write(&pending, b"{}").unwrap();
    assert_error(
        &fixture.run(&[
            "package",
            "compare",
            id(&installed),
            id(&installed),
            "--json",
        ]),
        "ledger-unavailable",
    );
    assert_eq!(fs::read(&pending).unwrap(), b"{}");
}

#[test]
fn noncanonical_and_hyphenated_selectors_are_bounded_content_free_errors() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    let oversized = "Z".repeat(8192);
    for invalid in [
        "01arz3ndektsv4rrffq69g5fav",
        "81ARZ3NDEKTSV4RRFFQ69G5FAV",
        " 01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "01ARZ3NDEKTSV4RRFFQ69G5FAV\n",
        "--FORGED-\u{1b}[31m",
        oversized.as_str(),
    ] {
        for (from, to) in [(invalid, id(&installed)), (id(&installed), invalid)] {
            let output = fixture.run(&["package", "compare", from, to, "--json"]);
            assert_error(&output, "installation-id-invalid");
            assert!(output.stdout.len() < 512);
            assert!(!output.stdout.contains(&0x1b));
            assert!(!String::from_utf8_lossy(&output.stdout).contains("FORGED"));
            assert!(output.stderr.is_empty());
        }
    }
}

#[test]
fn comparison_never_bootstraps_missing_empty_or_unrelated_ledgers() {
    for state in ["missing", "empty", "unrelated"] {
        let fixture = Fixture::new();
        let installed = fixture.install();
        let path = fixture.root.path().join("data/kernel.db");
        fs::remove_file(&path).unwrap();
        if state == "empty" {
            fs::write(&path, []).unwrap();
        } else if state == "unrelated" {
            let db = rusqlite::Connection::open(&path).unwrap();
            db.execute_batch(
                "CREATE TABLE unrelated (value TEXT); INSERT INTO unrelated VALUES ('keep');",
            )
            .unwrap();
        }
        let before = fs::read(&path).ok();
        assert_error(
            &fixture.run(&[
                "package",
                "compare",
                id(&installed),
                id(&installed),
                "--json",
            ]),
            "ledger-unavailable",
        );
        assert_eq!(fs::read(&path).ok(), before);
    }
}

fn database_rows(fixture: &Fixture) -> Vec<(String, Vec<String>)> {
    let db = rusqlite::Connection::open(fixture.root.path().join("data/kernel.db")).unwrap();
    let names = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    names
        .into_iter()
        .map(|name| {
            let mut statement = db
                .prepare(&format!("SELECT * FROM \"{}\"", name.replace('"', "\"\"")))
                .unwrap();
            let columns = statement.column_count();
            let mut rows = statement
                .query_map([], |row| {
                    (0..columns)
                        .map(|index| Ok(format!("{:?}", row.get_ref(index)?)))
                        .collect::<Result<Vec<_>, rusqlite::Error>>()
                        .map(|values| values.join("|"))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows.sort();
            (name, rows)
        })
        .collect()
}

#[test]
fn comparison_preserves_every_existing_database_row_including_authority_and_audit() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    let before = database_rows(&fixture);
    assert!(!before.is_empty());
    compare(&fixture, &installed, &installed);
    assert_eq!(database_rows(&fixture), before);
}

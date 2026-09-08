//! Operator authority, existing-ledger and conflict boundaries through the CLI.
use super::{assert_success, copy_tree, Fixture};
use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Stdio;

fn error_code(output: &std::process::Output) -> String {
    assert!(!output.status.success());
    serde_json::from_slice::<Value>(&output.stdout).unwrap()["error"]["code"].as_str().unwrap().to_owned()
}

#[test]
fn empty_and_unrelated_databases_are_not_bootstrapped_by_installation() {
    for unrelated in [false, true] {
        let fixture = Fixture::new();
        let database = fixture.root.path().join("data/kernel.db");
        fs::remove_file(&database).unwrap();
        if unrelated {
            let conn = Connection::open(&database).unwrap();
            conn.execute_batch("CREATE TABLE not_openspine (value TEXT); INSERT INTO not_openspine VALUES ('keep');").unwrap();
        } else { fs::write(&database, []).unwrap(); }
        let before = fs::read(&database).unwrap();
        let output = fixture.run(&["install", "--from", "candidate", "--json"]);
        assert_eq!(error_code(&output), "ledger-unavailable");
        assert_eq!(fs::read(&database).unwrap(), before);
        assert!(!fixture.root.path().join("data/packages").exists());
    }
}

#[test]
fn corrupt_audit_is_refused_without_another_installation_or_audit_append() {
    let fixture = Fixture::new();
    fixture.install();
    let conn = Connection::open(fixture.root.path().join("data/kernel.db")).unwrap();
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0)).unwrap();
    assert!(conn.execute("UPDATE audit_log SET event_json = '{}' WHERE seq = (SELECT MIN(seq) FROM audit_log)", []).unwrap() > 0);
    let output = fixture.run(&["install", "--from", "candidate", "--json"]);
    assert_eq!(error_code(&output), "ledger-unavailable");
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get::<_, i64>(0)).unwrap(), count);
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM installed_packages", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn package_namespace_cannot_overlap_configured_active_artifacts() {
    let fixture = Fixture::new();
    let config = fixture.root.path().join("openspine.yaml");
    let text = fs::read_to_string(&config).unwrap();
    assert_eq!(text.lines().filter(|line| line.starts_with("lyra_dir:")).count(), 1);
    let text = text.lines().map(|line| {
        if line.starts_with("lyra_dir:") { "lyra_dir: data/packages/objects" } else { line }
    }).collect::<Vec<_>>().join("\n") + "\n";
    fs::write(config, text).unwrap();
    let output = fixture.run(&["install", "--from", "candidate", "--json"]);
    assert_eq!(error_code(&output), "destination-invalid");
    assert!(!fixture.root.path().join("data/packages").exists());
}

#[test]
fn writable_shared_namespace_is_not_accepted_as_immutable_storage() {
    let fixture = Fixture::new();
    let namespace = fixture.root.path().join("data/packages");
    fs::create_dir(&namespace).unwrap();
    fs::set_permissions(&namespace, fs::Permissions::from_mode(0o777)).unwrap();
    assert_eq!(error_code(&fixture.run(&["install", "--from", "candidate", "--json"])), "destination-invalid");
    assert_eq!(fs::read_dir(namespace).unwrap().count(), 0);
}

#[test]
fn exact_retry_does_not_upgrade_original_provenance() {
    let fixture = Fixture::new();
    let first = fixture.install();
    let bundled_retry = fixture.json(&["install", "lyra", "--json"]);
    assert_eq!(first["receipt"], bundled_retry["receipt"]);
    assert_eq!(bundled_retry["receipt"]["provenance"], "local-unverified");
    assert_eq!(bundled_retry["idempotent_retry"], true);
    fs::remove_dir_all(fixture.root.path().join("candidate")).unwrap();
    assert_eq!(fixture.list()["packages"][0]["availability"], "available");
}

#[test]
fn different_revisions_coexist_without_selecting_either() {
    let fixture = Fixture::new();
    let first = fixture.install();
    let next = first["receipt"]["revision"].as_u64().unwrap() + 1;
    let declaration = fixture.root.path().join("candidate/package.yaml");
    let text = fs::read_to_string(&declaration).unwrap();
    assert_eq!(text.lines().filter(|line| line.starts_with("version:")).count(), 1);
    let text = text.lines().map(|line| {
        if line.starts_with("version:") { format!("version: {next}") } else { line.to_owned() }
    }).collect::<Vec<_>>().join("\n") + "\n";
    fs::write(declaration, text).unwrap();
    let second = fixture.install();
    assert_ne!(first["receipt"]["installation_id"], second["receipt"]["installation_id"]);
    assert_ne!(first["receipt"]["content_digest"], second["receipt"]["content_digest"]);
    let listed = fixture.list();
    let packages = listed["packages"].as_array().unwrap();
    assert_eq!(packages.len(), 2);
    for package in packages { assert_eq!(package["selected"], false); assert_eq!(package["active"], false); }
}

#[test]
fn concurrent_conflicting_installers_publish_only_one_revision_identity() {
    let fixture = Fixture::new();
    copy_tree(&fixture.root.path().join("candidate"), &fixture.root.path().join("different"));
    fs::write(fixture.root.path().join("different/README.md"), "Different bytes.\n").unwrap();
    let a = fixture.command(&["install", "--from", "candidate", "--json"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let b = fixture.command(&["install", "--from", "different", "--json"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let a = a.wait_with_output().unwrap(); let b = b.wait_with_output().unwrap();
    let (success, conflict) = if a.status.success() { (&a, &b) } else { (&b, &a) };
    assert_success(success);
    assert_eq!(error_code(conflict), "revision-conflict");
    let success: Value = serde_json::from_slice(&success.stdout).unwrap();
    let listed = fixture.list();
    assert_eq!(listed["packages"].as_array().unwrap().len(), 1);
    assert_eq!(listed["packages"][0]["receipt"], success["receipt"]);
}

fn authority_rows(fixture: &Fixture) -> Vec<(String, Vec<String>)> {
    let conn = Connection::open(fixture.root.path().join("data/kernel.db")).unwrap();
    let names = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND (
        name LIKE '%grant%' OR name LIKE 'standing_rule%' OR name LIKE '%credential%'
        OR name LIKE '%connector%' OR name LIKE '%principal%' OR name = 'learned_artifacts'
        OR name LIKE 'nerve_%') ORDER BY name").unwrap()
        .query_map([], |row| row.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    assert!(!names.is_empty());
    names.into_iter().map(|name| {
        let mut statement = conn.prepare(&format!("SELECT * FROM \"{}\"", name.replace('"', "\"\""))).unwrap();
        let columns = statement.column_count();
        let rows = statement.query_map([], |row| {
            (0..columns).map(|index| Ok(format!("{:?}", row.get_ref(index)?))).collect::<Result<Vec<_>, rusqlite::Error>>().map(|values| values.join("|"))
        }).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        (name, rows)
    }).collect()
}

#[test]
fn install_retry_list_and_conflict_preserve_all_existing_authority_rows() {
    let fixture = Fixture::new();
    let before = authority_rows(&fixture);
    fixture.install(); fixture.install(); fixture.list();
    fs::write(fixture.root.path().join("candidate/README.md"), "Different.\n").unwrap();
    assert_eq!(error_code(&fixture.run(&["install", "--from", "candidate", "--json"])), "revision-conflict");
    assert_eq!(authority_rows(&fixture), before);
}

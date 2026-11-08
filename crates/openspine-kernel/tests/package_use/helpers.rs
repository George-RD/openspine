//! Shared fixtures for the `openspine use` CLI contracts.
use super::*;

pub(super) const MARKER: &str = "You are Lyra (selected revision), a concise";

pub(super) fn id(installed: &Value) -> String {
    installed["receipt"]["installation_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// A second, modified revision of the base: its effective registry (and base
/// epoch) differs from the original.
pub(super) fn install_variant(fixture: &Fixture, revision: u32) -> Value {
    let source = fixture.root.path().join(format!("variant-{revision}"));
    copy(&fixture.root.path().join("candidate"), &source);
    let manifest = source.join("package.yaml");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replace("version: 2", &format!("version: {revision}")),
    )
    .unwrap();
    let template = source.join("templates/owner_terminal_template.yaml");
    let text = fs::read_to_string(&template).unwrap();
    fs::write(&template, text.replace("You are Lyra, a concise", MARKER)).unwrap();
    let output = fixture.run(&["install", "--from", source.to_str().unwrap(), "--json"]);
    assert_success(&output);
    serde_json::from_slice(&output.stdout).unwrap()
}

pub(super) fn copy(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

pub(super) fn db(fixture: &Fixture) -> rusqlite::Connection {
    rusqlite::Connection::open(fixture.root.path().join("data/kernel.db")).unwrap()
}

pub(super) fn selection(fixture: &Fixture) -> Option<(i64, String, i64)> {
    db(fixture)
        .query_row(
            "SELECT generation, installation_id, invalidation_generation FROM package_selection",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok()
}

pub(super) fn count(fixture: &Fixture, sql: &str) -> i64 {
    db(fixture).query_row(sql, [], |row| row.get(0)).unwrap()
}

pub(super) fn audit_chain(fixture: &Fixture) -> Vec<(i64, String)> {
    let db = db(fixture);
    let mut statement = db
        .prepare("SELECT seq, hash FROM audit_log ORDER BY seq")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Rows of the tables that must survive any selection untouched.
pub(super) fn protected_rows(fixture: &Fixture) -> Vec<(String, Vec<String>)> {
    const KEEP: [&str; 6] = [
        "principals",
        "identities",
        "identity_identifiers",
        "installed_packages",
        "erased_counterparties",
        "standing_rules",
    ];
    database_rows(fixture)
        .into_iter()
        .filter(|(name, _)| KEEP.contains(&name.as_str()))
        .collect()
}

pub(super) fn consent_session(fixture: &Fixture, installation: &str) -> pty::Session {
    pty::Session::spawn(fixture.command(&["use", installation]))
}

pub(super) const PROMPT: &str = "anything else cancels:";

pub(super) fn select(fixture: &Fixture, installation: &str) -> pty::Finished {
    let mut session = consent_session(fixture, installation);
    session.wait_for(PROMPT);
    session.send_line(&format!("select {installation}"));
    session.finish()
}

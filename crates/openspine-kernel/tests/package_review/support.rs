use super::*;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub(super) struct Fixture {
    pub root: tempfile::TempDir,
}

impl Fixture {
    pub fn new() -> Self {
        let fixture = Self {
            root: tempfile::tempdir().unwrap(),
        };
        assert_success(&fixture.run(&["init", "--owner", "987654321", "--name", "Tester"]));
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra"),
            &fixture.root.path().join("candidate"),
        );
        copy_tree(
            &fixture.root.path().join("candidate"),
            &fixture.root.path().join("artifacts/lyra"),
        );
        fixture
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_openspine"));
        command
            .current_dir(self.root.path())
            .arg("--config")
            .arg(self.root.path().join("openspine.yaml"))
            .args(args)
            .env("HOME", self.root.path())
            .env_remove("OPENSPINE_ARTIFACT_KEY")
            .env_remove("OPENSPINE_GRANT_HMAC_KEY")
            .env_remove("OPENSPINE_WEBHOOK_HMAC_KEY")
            .env_remove("OPENSPINE_LOCAL_API_KEY")
            .env_remove("OPENSPINE_TEST_PACKAGE_CRASH")
            .env_remove("OPENSPINE_TEST_PACKAGE_PAUSE")
            .env_remove("OPENSPINE_TEST_PACKAGE_BARRIER");
        command
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    pub fn install(&self) -> Value {
        let output = self.run(&["install", "--from", "candidate", "--json"]);
        assert_success(&output);
        serde_json::from_slice(&output.stdout).unwrap()
    }

    pub fn review_command(&self, installed: &Value) -> Command {
        self.command(&[
            "package",
            "review",
            installed["receipt"]["installation_id"].as_str().unwrap(),
            "--json",
        ])
    }

    pub fn review(&self, installed: &Value) -> Value {
        let output = self.review_command(installed).output().unwrap();
        assert_success(&output);
        serde_json::from_slice(&output.stdout).unwrap()
    }

    pub fn object(&self, installed: &Value) -> PathBuf {
        self.root.path().join("data/packages/objects").join(
            installed["receipt"]["content_digest"]
                .as_str()
                .unwrap()
                .strip_prefix("sha256:")
                .unwrap(),
        )
    }
}

pub(super) fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

pub(super) fn database_rows(fixture: &Fixture) -> Vec<(String, Vec<String>)> {
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

/// SQLite housekeeping, lock files and empty maintenance directories are not
/// logical changes. Compare every other existing payload byte recursively.
pub(super) fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, found: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, found);
            } else if path
                .file_name()
                .and_then(|x| x.to_str())
                .is_some_and(|name| !name.starts_with("kernel.db") && !name.ends_with(".lock"))
            {
                found.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut found = BTreeMap::new();
    visit(root, root, &mut found);
    found
}

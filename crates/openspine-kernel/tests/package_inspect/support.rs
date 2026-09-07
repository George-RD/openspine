//! Real CLI fixture and filesystem assertions.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

pub(super) struct Fixture {
    pub(super) root: tempfile::TempDir,
    pub(super) source: PathBuf,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("candidate");
        copy_tree(&bundled(), &source, false);
        Self { root, source }
    }

    pub(super) fn run(&self, json: bool) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_openspine"));
        command
            .env_clear()
            .env("HOME", self.root.path().join("home"))
            .current_dir(self.root.path())
            .arg("--config")
            .arg(self.root.path().join("application/openspine.yaml"))
            .args(["package", "inspect"])
            .arg(&self.source);
        if json {
            command.arg("--json");
        }
        command.output().unwrap()
    }

    pub(super) fn report(&self) -> Value {
        let output = self.run(true);
        assert!(
            output.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        serde_json::from_slice(&output.stdout).unwrap()
    }

    pub(super) fn rejected(&self, code: &str) {
        let output = self.run(true);
        assert!(!output.status.success(), "invalid candidate was accepted");
        let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "not a JSON inspection failure: {error}; stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["valid"], false);
        assert_eq!(report["provenance"], "local-unverified");
        assert_eq!(report["error"]["code"], code);
        assert!(!self.root.path().join("home").exists());
        assert!(!self.root.path().join("application").exists());
    }

    pub(super) fn declaration(&self, edit: impl FnOnce(&mut serde_yaml::Value)) {
        let path = self.source.join("package.yaml");
        let mut value = serde_yaml::from_slice(&fs::read(&path).unwrap()).unwrap();
        edit(&mut value);
        fs::write(path, serde_yaml::to_string(&value).unwrap()).unwrap();
    }

    pub(super) fn artifact(&self, family: &str, id: &str) -> PathBuf {
        fs::read_dir(self.source.join(family))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                let value: serde_yaml::Value =
                    serde_yaml::from_slice(&fs::read(path).unwrap()).unwrap();
                value["id"].as_str() == Some(id)
            })
            .expect("fixture artifact")
    }
}

pub(super) fn bundled() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra")
}

pub(super) fn copy_tree(source: &Path, destination: &Path, reverse: bool) {
    fs::create_dir_all(destination).unwrap();
    let mut paths: Vec<_> = fs::read_dir(source)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    if reverse {
        paths.reverse();
    }
    for path in paths {
        let output = destination.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_tree(&path, &output, reverse);
        } else {
            fs::copy(path, output).unwrap();
        }
    }
}

pub(super) fn file_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn collect(root: &Path, dir: &Path, result: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(root, &path, result);
            } else {
                result.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .replace('\\', "/"),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    collect(root, root, &mut result);
    result
}

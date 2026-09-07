//! Exercise package inspection through the real executable, without runtime setup.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use openspine_schemas::digest::{digest_of, digest_of_bytes};
use serde_json::Value;

struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("candidate");
        copy_tree(&bundled(), &source, false);
        Self { root, source }
    }

    fn run(&self, json: bool) -> Output {
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

    fn report(&self) -> Value {
        let output = self.run(true);
        assert!(output.status.success(), "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert!(output.stderr.is_empty());
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn rejected(&self, code: &str) {
        let output = self.run(true);
        assert!(!output.status.success(), "invalid candidate was accepted");
        let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!("not a JSON inspection failure: {error}; stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
        });
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["valid"], false);
        assert_eq!(report["provenance"], "local-unverified");
        assert_eq!(report["error"]["code"], code);
        assert!(!self.root.path().join("home").exists());
        assert!(!self.root.path().join("application").exists());
    }

    fn declaration(&self, edit: impl FnOnce(&mut serde_yaml::Value)) {
        let path = self.source.join("package.yaml");
        let mut value = serde_yaml::from_slice(&fs::read(&path).unwrap()).unwrap();
        edit(&mut value);
        fs::write(path, serde_yaml::to_string(&value).unwrap()).unwrap();
    }

    fn artifact(&self, family: &str, id: &str) -> PathBuf {
        fs::read_dir(self.source.join(family)).unwrap().map(|entry| entry.unwrap().path())
            .find(|path| {
                let value: serde_yaml::Value = serde_yaml::from_slice(&fs::read(path).unwrap()).unwrap();
                value["id"].as_str() == Some(id)
            }).expect("fixture artifact")
    }
}

fn bundled() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra")
}

fn copy_tree(source: &Path, destination: &Path, reverse: bool) {
    fs::create_dir_all(destination).unwrap();
    let mut paths: Vec<_> = fs::read_dir(source).unwrap().map(|entry| entry.unwrap().path()).collect();
    paths.sort();
    if reverse { paths.reverse(); }
    for path in paths {
        let output = destination.join(path.file_name().unwrap());
        if path.is_dir() { copy_tree(&path, &output, reverse); }
        else { fs::copy(path, output).unwrap(); }
    }
}

fn file_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn collect(root: &Path, dir: &Path, result: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() { collect(root, &path, result); }
            else {
                result.insert(path.strip_prefix(root).unwrap().to_str().unwrap().replace('\\', "/"), fs::read(path).unwrap());
            }
        }
    }
    let mut result = BTreeMap::new();
    collect(root, root, &mut result);
    result
}

#[test]
fn package_inspect_real_bundle_is_read_only_and_inventory_binds_every_file() {
    let fixture = Fixture::new();
    let before = file_bytes(&fixture.source);
    let report = fixture.report();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["inventory_format_version"], 1);
    assert_eq!(report["valid"], true);
    assert_eq!(report["package_id"], "lyra");
    assert_eq!(report["revision"], 1);
    assert_eq!(report["provenance"], "local-unverified");
    let inventory = report["inventory"].as_array().unwrap();
    assert_eq!(inventory.len(), before.len());
    for (entry, (path, bytes)) in inventory.iter().zip(before.iter()) {
        assert_eq!(entry["path"].as_str().unwrap(), path);
        assert_eq!(entry["bytes"].as_u64().unwrap(), bytes.len() as u64);
        assert_eq!(entry["digest"], digest_of_bytes(bytes).to_string());
    }
    assert_eq!(report["content_digest"], digest_of(&serde_json::json!({
        "inventory_format_version": 1, "files": inventory
    })).to_string());
    assert_eq!(before, file_bytes(&fixture.source));
    assert!(!fixture.root.path().join("home").exists());
    assert!(!fixture.root.path().join("application").exists());
}

#[test]
fn package_inspect_identity_ignores_source_location_and_enumeration_order() {
    let left = Fixture::new();
    let mut right = Fixture::new();
    right.source = right.root.path().join("different-location");
    copy_tree(&left.source, &right.source, true);
    assert_eq!(left.report(), right.report());
}

#[test]
fn package_inspect_documentation_bytes_and_paths_change_identity() {
    let fixture = Fixture::new();
    let original = fixture.report()["content_digest"].clone();
    fs::write(fixture.source.join("notes.md"), "Documentation, not authority.\n").unwrap();
    let added = fixture.report()["content_digest"].clone();
    assert_ne!(original, added);
    fs::rename(fixture.source.join("notes.md"), fixture.source.join("other.md")).unwrap();
    let renamed = fixture.report()["content_digest"].clone();
    assert_ne!(added, renamed);
    fs::write(fixture.source.join("other.md"), "Changed documentation.\n").unwrap();
    assert_ne!(renamed, fixture.report()["content_digest"]);
}

#[test]
fn package_inspect_missing_declaration_fails() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.source.join("package.yaml")).unwrap();
    fixture.rejected("declaration-missing");
}

#[test]
fn package_inspect_malformed_declaration_fails() {
    let fixture = Fixture::new();
    fs::write(fixture.source.join("package.yaml"), "invalid: [").unwrap();
    fixture.rejected("declaration-invalid");
}

#[test]
fn package_inspect_duplicate_declaration_keys_fail() {
    let fixture = Fixture::new();
    let path = fixture.source.join("package.yaml");
    let mut bytes = fs::read(path.clone()).unwrap();
    bytes.extend_from_slice(b"\nid: replacement\n");
    fs::write(path, bytes).unwrap();
    fixture.rejected("declaration-invalid");
}

#[test]
fn package_inspect_unsupported_schema_fails() {
    let fixture = Fixture::new();
    fixture.declaration(|value| value["schema_version"] = 2.into());
    fixture.rejected("declaration-invalid");
}

#[test]
fn package_inspect_unknown_family_fails() {
    let fixture = Fixture::new();
    fixture.declaration(|value| value["artifacts"]["hooks"] = serde_yaml::Value::Sequence(vec![]));
    fixture.rejected("declaration-invalid");
}

#[test]
fn package_inspect_duplicate_declared_ids_fail() {
    let fixture = Fixture::new();
    fixture.declaration(|value| {
        let agents = value["artifacts"]["agents"].as_sequence_mut().unwrap();
        agents.push(agents[0].clone());
    });
    fixture.rejected("declaration-invalid");
}

#[test]
fn package_inspect_missing_declared_artifact_fails() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.artifact("agents", "main_assistant_agent")).unwrap();
    fixture.rejected("inventory-mismatch");
}

#[test]
fn package_inspect_undeclared_loadable_artifact_fails() {
    let fixture = Fixture::new();
    fixture.declaration(|value| {
        value["artifacts"]["agents"].as_sequence_mut().unwrap()
            .retain(|id| id.as_str() != Some("main_assistant_agent"));
    });
    fixture.rejected("inventory-mismatch");
}

#[test]
fn package_inspect_unknown_entry_agent_fails() {
    let fixture = Fixture::new();
    fixture.declaration(|value| value["entry_agent"] = "not-an-agent".into());
    fixture.rejected("entry-agent-invalid");
}

#[test]
fn package_inspect_invalid_typed_artifact_fails() {
    let fixture = Fixture::new();
    fs::write(fixture.artifact("agents", "main_assistant_agent"), "id: main_assistant_agent\nunknown: field\n").unwrap();
    fixture.rejected("artifact-invalid");
}

#[test]
fn package_inspect_versioned_collision_fails() {
    let fixture = Fixture::new();
    fs::copy(fixture.artifact("agents", "main_assistant_agent"), fixture.source.join("agents/duplicate.yaml")).unwrap();
    fixture.rejected("artifact-collision");
}

#[test]
fn package_inspect_unversioned_golden_set_collision_fails() {
    let fixture = Fixture::new();
    fs::copy(fixture.artifact("golden_sets", "model_swap_default"), fixture.source.join("golden_sets/duplicate.yaml")).unwrap();
    fixture.rejected("artifact-collision");
}

#[test]
fn package_inspect_highest_entry_agent_version_must_be_active() {
    let fixture = Fixture::new();
    let path = fixture.artifact("agents", "main_assistant_agent");
    let mut agent: serde_yaml::Value = serde_yaml::from_slice(&fs::read(path).unwrap()).unwrap();
    agent["version"] = 100.into();
    let next = fixture.source.join("agents/newer.yaml");
    fs::write(&next, serde_yaml::to_string(&agent).unwrap()).unwrap();
    fixture.report();
    agent["lifecycle_state"] = "draft".into();
    fs::write(next, serde_yaml::to_string(&agent).unwrap()).unwrap();
    fixture.rejected("entry-agent-invalid");
}

#[test]
fn package_inspect_nested_artifact_directories_fail() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.source.join("agents/nested")).unwrap();
    fixture.rejected("payload-unsupported");
}

#[test]
fn package_inspect_base_personas_fail() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.source.join("personas")).unwrap();
    fs::write(fixture.source.join("personas/hidden.yaml"), "id: hidden\n").unwrap();
    fixture.rejected("payload-unsupported");
}

#[test]
fn package_inspect_executable_payload_fails() {
    let fixture = Fixture::new();
    fs::write(fixture.source.join("install.sh"), "#!/bin/sh\necho never-run\n").unwrap();
    fixture.rejected("payload-unsupported");
}

#[test]
fn package_inspect_file_size_limit_fails() {
    let fixture = Fixture::new();
    let file = fs::File::create(fixture.source.join("huge.md")).unwrap();
    file.set_len(8 * 1024 * 1024 + 1).unwrap();
    fixture.rejected("limit-exceeded");
}

#[test]
fn package_inspect_total_size_limit_fails() {
    let fixture = Fixture::new();
    let bytes = vec![b'x'; 8 * 1024 * 1024];
    for index in 0..9 {
        fs::write(fixture.source.join(format!("large-{index}.md")), &bytes).unwrap();
    }
    fixture.rejected("limit-exceeded");
}

#[test]
fn package_inspect_file_count_limit_fails() {
    let fixture = Fixture::new();
    for index in 0..4097 {
        fs::write(fixture.source.join(format!("note-{index}.md")), b"").unwrap();
    }
    fixture.rejected("limit-exceeded");
}

#[test]
fn package_inspect_case_ambiguous_paths_fail() {
    let fixture = Fixture::new();
    fs::write(fixture.source.join("note.md"), "one").unwrap();
    fs::write(fixture.source.join("NOTE.md"), "two").unwrap();
    fixture.rejected("path-invalid");
}

#[test]
fn package_inspect_untrusted_metadata_is_not_echoed_or_treated_as_verification() {
    let fixture = Fixture::new();
    fixture.declaration(|value| {
        value["display_name"] = "\u{1b}[2JPUBLISHER-VERIFIED\u{202e}".into();
        value["security_invariants"] = serde_yaml::Value::Sequence(vec!["PAYLOAD-MARKER".into()]);
    });
    for json in [false, true] {
        let output = fixture.run(json);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains("PUBLISHER-VERIFIED"));
        assert!(!text.contains("PAYLOAD-MARKER"));
        assert!(!text.contains('\u{1b}'));
        assert!(!text.contains('\u{202e}'));
        assert!(text.contains("local-unverified"));
        if !json { assert!(text.len() < 2048); }
    }
}

#[cfg(unix)]
#[test]
fn package_inspect_symlink_file_fails() {
    let fixture = Fixture::new();
    std::os::unix::fs::symlink("README.md", fixture.source.join("link.md")).unwrap();
    fixture.rejected("source-unavailable");
}

#[cfg(unix)]
#[test]
fn package_inspect_symlink_directory_fails() {
    let fixture = Fixture::new();
    let agents = fixture.source.join("agents");
    let outside = fixture.root.path().join("outside-agents");
    fs::rename(&agents, &outside).unwrap();
    std::os::unix::fs::symlink(outside, agents).unwrap();
    fixture.rejected("source-unavailable");
}

#[cfg(unix)]
#[test]
fn package_inspect_socket_fails_without_reading_or_blocking() {
    let fixture = Fixture::new();
    let _socket = std::os::unix::net::UnixListener::bind(fixture.source.join("socket.md")).unwrap();
    fixture.rejected("source-unavailable");
}

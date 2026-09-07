//! Exercise package inspection through the real executable, without runtime setup.
use std::fs;
use openspine_schemas::digest::{digest_of, digest_of_bytes};

#[path = "package_inspect/support.rs"]
mod support;
use support::*;

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
    agent["lifecycle_state"] = "retired".into();
    fs::write(next, serde_yaml::to_string(&agent).unwrap()).unwrap();
    fixture.rejected("entry-agent-invalid");
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

#[test]
fn package_inspect_does_not_load_existing_owner_configuration_or_environment() {
    let fixture = Fixture::new();
    let application = fixture.root.path().join("application");
    fs::create_dir(&application).unwrap();
    fs::write(application.join("openspine.yaml"), "not valid config\n").unwrap();
    fs::write(application.join("openspine.env"), "KEY-MATERIAL-MUST-NOT-BE-READ\n").unwrap();
    fs::write(application.join("kernel.db"), "not a database\n").unwrap();
    let before = file_bytes(&application);
    fixture.report();
    assert_eq!(before, file_bytes(&application));
    assert!(!fixture.root.path().join("home").exists());
}

#[test]
fn package_inspect_unversioned_golden_sets_reject_fabricated_versions() {
    let fixture = Fixture::new();
    let path = fixture.artifact("golden_sets", "model_swap_default");
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend_from_slice(b"\nversion: 1\n");
    fs::write(path, bytes).unwrap();
    fixture.rejected("artifact-invalid");
}

#[path = "package_inspect/filesystem.rs"]
mod filesystem;

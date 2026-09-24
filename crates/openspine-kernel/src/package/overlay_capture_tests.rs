use super::*;
use std::fs;
use std::os::unix::fs::symlink;

#[test]
fn overlay_review_refuses_a_linked_family_instead_of_assessing_empty_state() {
    let root = tempfile::tempdir().unwrap();
    let overlay = root.path().join("artifacts.d");
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir(&overlay).unwrap();
    fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, overlay.join("routes")).unwrap();
    assert!(capture(&overlay, &HashMap::new()).is_err());
}

#[test]
fn overlay_review_retains_every_standing_rule_and_model_swap_version() {
    let root = tempfile::tempdir().unwrap();
    for family in ["standing_rules", "model_swaps"] {
        fs::create_dir(root.path().join(family)).unwrap();
    }
    for (name, version) in [("a", 2), ("z", 1)] {
        fs::write(
            root.path().join(format!("standing_rules/{name}.yaml")),
            format!(
            "id: appointments\nschema_version: 1\nversion: {version}\nlifecycle_state: active\n\
             action_id: calendar.book_appointment\ndescription: Booking rule\n\
             quota: {{max: 5, window_secs: 604800}}\nrate: {{max: 1, window_secs: 3600}}\n\
             expires_after_secs: 7776000\n"
        ),
        )
        .unwrap();
        fs::write(
            root.path().join(format!("model_swaps/{name}.yaml")),
            format!(
                "id: base\nversion: {version}\nlifecycle_state: active\nrole: base\n\
             target_provider_id: chosen\ngolden_set_id: stable\n"
            ),
        )
        .unwrap();
    }
    let captured = capture(root.path(), &HashMap::new()).unwrap();
    assert_eq!(captured.registry.sources.len(), 4);
    assert_eq!(captured.registry.standing_rules["appointments"].version, 2);
    assert_eq!(captured.registry.model_swaps["base"].version, 2);
    for (kind, id) in [("standing_rule", "appointments"), ("model_swap", "base")] {
        for version in [1, 2] {
            assert!(captured
                .registry
                .sources
                .contains_key(&(kind.into(), id.into(), version)));
        }
    }
}

fn route_fixture() -> (tempfile::TempDir, std::path::PathBuf, Vec<u8>) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("routes")).unwrap();
    let path = root
        .path()
        .join("routes")
        .join(artifact_loader::overlay_filename("route", 1));
    let bytes = b"id: route\nschema_version: 1\nversion: 1\nlifecycle_state: active\neffect: allow\nwhen: {}\n".to_vec();
    fs::write(&path, &bytes).unwrap();
    (root, path, bytes)
}

#[test]
fn overlay_review_refuses_linked_files_and_oversized_payloads() {
    for kind in ["symlink", "hardlink", "oversized"] {
        let (root, file, _) = route_fixture();
        let external = tempfile::tempdir().unwrap();
        match kind {
            "symlink" => {
                let saved = external.path().join("saved.yaml");
                fs::rename(&file, &saved).unwrap();
                symlink(saved, &file).unwrap();
            }
            "hardlink" => fs::hard_link(&file, external.path().join("alias.yaml")).unwrap(),
            "oversized" => fs::OpenOptions::new()
                .write(true)
                .open(&file)
                .unwrap()
                .set_len(super::super::MAX_FILE_BYTES as u64 + 1)
                .unwrap(),
            _ => unreachable!(),
        }
        assert!(capture(root.path(), &HashMap::new()).is_err(), "{kind}");
    }
}

#[test]
fn overlay_review_owns_original_bytes_and_binds_ignored_personas() {
    let (root, path, bytes) = route_fixture();
    fs::create_dir(root.path().join("personas")).unwrap();
    let unadmitted = root.path().join("personas/unadmitted.yaml");
    fs::write(&unadmitted, b"not valid: [persona").unwrap();
    let first = capture(root.path(), &HashMap::new()).unwrap();
    let repeated = capture(root.path(), &HashMap::new()).unwrap();
    assert_eq!(
        first.source_inventory_digest,
        repeated.source_inventory_digest
    );
    assert_eq!(first.ignored_persona_files, ["personas/unadmitted.yaml"]);
    assert!(first.registry.personas.is_empty());
    fs::write(unadmitted, b"different unadmitted: [persona").unwrap();
    let changed = capture(root.path(), &HashMap::new()).unwrap();
    assert_ne!(
        first.source_inventory_digest,
        changed.source_inventory_digest
    );
    root.close().unwrap();
    let source = &first.registry.sources[&("route".into(), "route".into(), 1)];
    assert_eq!(source.path, path);
    assert_eq!(source.bytes, bytes);
}

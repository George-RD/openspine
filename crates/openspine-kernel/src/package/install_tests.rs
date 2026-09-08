//! Proves that the installer consumes the inspected object, not its source path.
use super::{inspect, install, install_types::PackageProvenance, object_store::PackageObjects};
use crate::store::Store;
use std::fs;
use std::path::Path;

fn copy(source: &Path, destination: &Path) {
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

#[test]
fn package_install_uses_retained_validated_bytes_after_source_disappears() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("candidate");
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra"),
        &source,
    );
    let snapshot = inspect(&source).unwrap();
    fs::remove_dir_all(&source).unwrap();
    let data = root.path().join("data");
    fs::create_dir(&data).unwrap();
    let store = Store::open(&data.join("kernel.db")).unwrap();
    let objects = PackageObjects::open(&data, &root.path().join("active")).unwrap();
    let result = install::install(
        &store,
        &objects,
        &snapshot,
        PackageProvenance::LocalUnverified,
    )
    .unwrap();
    assert_eq!(result.receipt.identity(), snapshot.identity());
    let object = data.join("packages/objects").join(
        result
            .receipt
            .content_digest
            .as_str()
            .strip_prefix("sha256:")
            .unwrap(),
    );
    for (path, bytes) in snapshot.files() {
        assert_eq!(fs::read(object.join(path)).unwrap(), bytes);
    }
    assert!(store.verify_audit_chain().unwrap());
}

#[test]
fn package_object_verification_binds_every_identity_field() {
    use super::install_types::InstallError;
    use openspine_schemas::digest::digest_of_bytes;

    let root = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra");
    let snapshot = inspect(&source).unwrap();
    let data = root.path().join("data");
    fs::create_dir(&data).unwrap();
    let objects = PackageObjects::open(&data, &root.path().join("active")).unwrap();
    objects.publish(ulid::Ulid::new(), &snapshot).unwrap();
    let identity = snapshot.identity();
    objects.verify(&identity).unwrap();
    for field in ["package", "revision", "format", "manifest", "content"] {
        let mut changed = identity.clone();
        match field {
            "package" => changed.package_id.push_str("-other"),
            "revision" => changed.revision += 1,
            "format" => changed.inventory_format_version += 1,
            "manifest" => changed.manifest_digest = digest_of_bytes(b"other manifest"),
            "content" => {
                changed.content_digest = digest_of_bytes(b"other inventory");
                // Keep a readable object at the wrong content address so this
                // case proves hashing, not merely missing-directory rejection.
                let directory = data.join("packages/objects");
                let original_digest = identity.content_digest.as_str();
                let changed_digest = changed.content_digest.as_str();
                copy(
                    &directory.join(original_digest.strip_prefix("sha256:").unwrap()),
                    &directory.join(changed_digest.strip_prefix("sha256:").unwrap()),
                );
            }
            _ => unreachable!(),
        }
        assert!(
            matches!(objects.verify(&changed), Err(InstallError::ObjectCorrupt)),
            "{field}"
        );
    }
    objects.verify(&identity).unwrap();
}

/// Build historical-byte fixtures with matching recorded digests, not a new
/// validated snapshot. This deliberately bypasses installation only in tests.
fn retained_manifest(
    root: &Path,
    snapshot: &super::PackageSnapshot,
    manifest: Vec<u8>,
) -> (
    PackageObjects,
    super::install_types::PackageIdentity,
    std::path::PathBuf,
) {
    let data = root.join("data");
    fs::create_dir(&data).unwrap();
    let objects = PackageObjects::open(&data, &root.join("active")).unwrap();
    let mut identity = snapshot.identity();
    identity.manifest_digest = openspine_schemas::digest::digest_of_bytes(&manifest);
    let mut files = snapshot
        .files()
        .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
        .collect::<std::collections::BTreeMap<_, _>>();
    files.insert("package.yaml".into(), manifest);
    identity.content_digest = super::inventory_of(&files).1;
    let object = data.join("packages/objects").join(
        identity
            .content_digest
            .as_str()
            .strip_prefix("sha256:")
            .unwrap(),
    );
    for (path, bytes) in files {
        let path = object.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    (objects, identity, object)
}

/// Availability checks recorded identity, not today's unrelated declaration
/// fields. The same bytes must still fail inspection as a new candidate.
#[test]
fn package_object_verification_ignores_current_declaration_compatibility() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra");
    let snapshot = inspect(&source).unwrap();
    let manifest = snapshot
        .files()
        .find(|(path, _)| *path == "package.yaml")
        .unwrap()
        .1;
    let original: serde_yaml::Value = serde_yaml::from_slice(manifest).unwrap();
    for (field, value) in [
        ("schema_version", serde_yaml::Value::from(999)),
        ("display_name", serde_yaml::Value::Sequence(Vec::new())),
        ("future_metadata", serde_yaml::Value::from(true)),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut changed = original.clone();
        changed
            .as_mapping_mut()
            .unwrap()
            .insert(serde_yaml::Value::from(field), value);
        let (objects, identity, object) = retained_manifest(
            root.path(),
            &snapshot,
            serde_yaml::to_string(&changed).unwrap().into_bytes(),
        );
        assert!(
            matches!(
                inspect(&object),
                Err(super::InspectionError::DeclarationInvalid)
            ),
            "new candidate with {field} must still be rejected"
        );
        objects
            .verify(&identity)
            .unwrap_or_else(|error| panic!("retained {field}: {error}"));
    }
}

/// Ignoring non-identity fields must not admit ambiguous, absent or coerced
/// identity fields, even when the complete recorded byte digests match.
#[test]
fn package_object_verification_rejects_malformed_retained_identity() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra");
    let snapshot = inspect(&source).unwrap();
    for (manifest, expected_id) in [
        ("id: lyra\nid: lyra\nversion: 2\n", "lyra"),
        ("id: lyra\nversion: 2\nversion: 2\n", "lyra"),
        ("version: 2\n", "lyra"),
        ("id: lyra\n", "lyra"),
        ("id: [lyra]\nversion: 2\n", "lyra"),
        ("id: lyra\nversion: 2.5\n", "lyra"),
        ("id: null\nversion: 2\n", "null"),
        ("id: true\nversion: 2\n", "true"),
        ("id: lyra\nversion: \"2\"\n", "lyra"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (objects, mut identity, _) =
            retained_manifest(root.path(), &snapshot, manifest.as_bytes().to_vec());
        identity.package_id = expected_id.to_owned();
        assert!(matches!(
            objects.verify(&identity),
            Err(super::install_types::InstallError::ObjectCorrupt)
        ));
    }
}

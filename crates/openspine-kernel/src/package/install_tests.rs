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
    let snapshot = inspect(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra"))
        .unwrap();
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
                copy(
                    &directory.join(identity.content_digest.as_str().strip_prefix("sha256:").unwrap()),
                    &directory.join(changed.content_digest.as_str().strip_prefix("sha256:").unwrap()),
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

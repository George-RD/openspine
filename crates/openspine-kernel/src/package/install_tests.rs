//! Proves that the installer consumes the inspected object, not its source path.
use std::fs;
use std::path::Path;
use super::{inspect, install, install_types::PackageProvenance, object_store::PackageObjects};
use crate::store::Store;

fn copy(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() { copy(&entry.path(), &target); }
        else { fs::copy(entry.path(), target).unwrap(); }
    }
}

#[test]
fn package_install_uses_retained_validated_bytes_after_source_disappears() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("candidate");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra"), &source);
    let snapshot = inspect(&source).unwrap();
    fs::remove_dir_all(&source).unwrap();
    let data = root.path().join("data");
    fs::create_dir(&data).unwrap();
    let store = Store::open(&data.join("kernel.db")).unwrap();
    let objects = PackageObjects::open(&data, &root.path().join("active")).unwrap();
    let result = install::install(&store, &objects, &snapshot, PackageProvenance::LocalUnverified).unwrap();
    assert_eq!(result.receipt.identity(), snapshot.identity());
    let object = data.join("packages/objects").join(result.receipt.content_digest.as_str().strip_prefix("sha256:").unwrap());
    for (path, bytes) in snapshot.files() { assert_eq!(fs::read(object.join(path)).unwrap(), bytes); }
    assert!(store.verify_audit_chain().unwrap());
}

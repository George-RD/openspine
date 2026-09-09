use super::*;
use std::fs;
use std::path::PathBuf;

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let source = entry.unwrap().path();
        let destination = destination.join(source.file_name().unwrap());
        if source.is_dir() {
            copy_tree(&source, &destination);
        } else {
            fs::copy(source, destination).unwrap();
        }
    }
}

fn source_bytes(source: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        let path = entry.path();
        if path.is_dir() {
            for (relative, bytes) in source_bytes(&path) {
                files.insert(format!("{name}/{relative}"), bytes);
            }
        } else {
            files.insert(name, fs::read(path).unwrap());
        }
    }
    files
}

#[test]
fn package_snapshot_retains_exact_validated_bytes_after_source_mutation_and_removal() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("candidate");
    copy_tree(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra"),
        &source,
    );
    let expected = source_bytes(&source);
    let snapshot = inspect(&source).unwrap();
    let before: BTreeMap<_, _> = snapshot
        .files()
        .map(|(path, bytes)| (path.to_string(), bytes.to_vec()))
        .collect();
    assert_eq!(before, expected);
    let identity = serde_json::to_value(snapshot.report()).unwrap();
    fs::write(source.join("package.yaml"), "not a declaration").unwrap();
    assert!(matches!(
        inspect(&source),
        Err(InspectionError::DeclarationInvalid)
    ));
    fs::remove_dir_all(source).unwrap();
    let retained: BTreeMap<_, _> = snapshot
        .files()
        .map(|(path, bytes)| (path.to_string(), bytes.to_vec()))
        .collect();
    assert_eq!(retained, before);
    assert_eq!(serde_json::to_value(snapshot.report()).unwrap(), identity);
    for entry in &snapshot.report.inventory {
        assert_eq!(entry.digest, digest_of_bytes(&retained[&entry.path]));
        assert_eq!(entry.bytes, retained[&entry.path].len());
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "retained_capture_tests.rs"]
mod retained_capture;

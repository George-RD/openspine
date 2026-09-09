//! Baseline seam: compose today's identity verification with a later inspection.
//! This is test-only, not an assertion that a shipped command uses this sequence.
use super::{copy_tree, source_bytes};
use crate::package::{
    inspect, install_types::InstallError, install_types::PackageIdentity,
    object_store::PackageObjects, InspectionError, PackageSnapshot,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

struct Fixture {
    _root: tempfile::TempDir,
    objects: PackageObjects,
    identity: PackageIdentity,
    path: PathBuf,
    expected: BTreeMap<String, Vec<u8>>,
}

// RED adapter: metadata verification cannot retain bytes for later validation.
struct PathCapture(PathBuf);

impl PathCapture {
    fn validate(self) -> Result<PackageSnapshot, InspectionError> {
        inspect(&self.0)
    }
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        fs::create_dir(&data).unwrap();
        let objects = PackageObjects::open(&data, &root.path().join("active")).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra");
        let snapshot = inspect(&source).unwrap();
        let expected = source_bytes(&source);
        let identity = snapshot.identity();
        objects.publish(ulid::Ulid::new(), &snapshot).unwrap();
        let path = data.join("packages/objects").join(
            identity
                .content_digest
                .as_str()
                .strip_prefix("sha256:")
                .unwrap(),
        );
        Self {
            _root: root,
            objects,
            identity,
            path,
            expected,
        }
    }

    fn capture(&self) -> Result<PathCapture, InstallError> {
        self.objects.verify(&self.identity)?;
        Ok(PathCapture(self.path.clone()))
    }

    fn assert_original(&self, snapshot: PackageSnapshot) {
        assert_eq!(snapshot.identity(), self.identity);
        let actual: BTreeMap<_, _> = snapshot
            .files()
            .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
            .collect();
        assert_eq!(actual, self.expected);
    }

    // Historical recorded bytes, not a newly installable/validated snapshot.
    fn incompatible() -> Self {
        let mut fixture = Self::new();
        let mut manifest: serde_yaml::Value =
            serde_yaml::from_slice(&fixture.expected["package.yaml"]).unwrap();
        manifest.as_mapping_mut().unwrap().insert(
            serde_yaml::Value::from("schema_version"),
            serde_yaml::Value::from(999),
        );
        let bytes = serde_yaml::to_string(&manifest).unwrap().into_bytes();
        fixture.identity.manifest_digest = openspine_schemas::digest::digest_of_bytes(&bytes);
        fixture.expected.insert("package.yaml".into(), bytes);
        fixture.identity.content_digest = crate::package::inventory_of(&fixture.expected).1;
        let old_path = fixture.path.clone();
        fixture.path = old_path.parent().unwrap().join(
            fixture
                .identity
                .content_digest
                .as_str()
                .strip_prefix("sha256:")
                .unwrap(),
        );
        fs::rename(old_path, &fixture.path).unwrap();
        fs::write(
            fixture.path.join("package.yaml"),
            &fixture.expected["package.yaml"],
        )
        .unwrap();
        fixture
    }
}

#[test]
fn retained_capture_validates_original_bytes_without_mutating_object() {
    let fixture = Fixture::new();
    let inventory =
        serde_json::to_value(fixture.objects.inventory(&fixture.identity).unwrap()).unwrap();
    fixture.assert_original(fixture.capture().unwrap().validate().unwrap());
    assert_eq!(source_bytes(&fixture.path), fixture.expected);
    assert_eq!(
        serde_json::to_value(fixture.objects.inventory(&fixture.identity).unwrap()).unwrap(),
        inventory
    );
}

#[test]
fn retained_capture_survives_object_removal_before_validation() {
    let fixture = Fixture::new();
    let capture = fixture.capture().unwrap();
    fs::remove_dir_all(&fixture.path).unwrap();
    fixture.assert_original(capture.validate().unwrap());
    assert!(
        fixture.capture().is_err(),
        "fresh capture must still detect loss"
    );
}

#[test]
fn retained_capture_does_not_validate_later_valid_substitution() {
    let fixture = Fixture::new();
    let capture = fixture.capture().unwrap();
    fs::write(
        fixture.path.join("later.txt"),
        "different but valid package bytes",
    )
    .unwrap();
    assert!(
        inspect(&fixture.path).is_ok(),
        "substitution must be structurally valid"
    );
    fixture.assert_original(capture.validate().unwrap());
    assert!(fixture.capture().is_err());
}

#[test]
fn retained_capture_does_not_validate_later_corrupt_manifest() {
    let fixture = Fixture::new();
    let capture = fixture.capture().unwrap();
    fs::write(fixture.path.join("package.yaml"), "not a declaration").unwrap();
    fixture.assert_original(capture.validate().unwrap());
    assert!(fixture.capture().is_err());
}

#[test]
fn retained_capture_does_not_follow_replaced_object_directory() {
    let fixture = Fixture::new();
    let capture = fixture.capture().unwrap();
    let moved = fixture.path.with_file_name("saved-object");
    fs::rename(&fixture.path, &moved).unwrap();
    copy_tree(&moved, &fixture.path);
    fs::write(fixture.path.join("later.txt"), "replacement directory").unwrap();
    fixture.assert_original(capture.validate().unwrap());
    assert!(fixture.capture().is_err());
}

#[test]
fn retained_capture_does_not_follow_later_object_symlink() {
    let fixture = Fixture::new();
    let capture = fixture.capture().unwrap();
    let moved = fixture.path.with_file_name("saved-object");
    fs::rename(&fixture.path, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &fixture.path).unwrap();
    fixture.assert_original(capture.validate().unwrap());
    assert!(fixture.capture().is_err());
}

#[test]
fn retained_capture_keeps_historical_incompatibility_separate_from_integrity() {
    let fixture = Fixture::incompatible();
    let capture = fixture.capture().unwrap();
    assert!(matches!(
        capture.validate(),
        Err(InspectionError::DeclarationInvalid)
    ));
    fixture.objects.verify(&fixture.identity).unwrap();
    assert_eq!(source_bytes(&fixture.path), fixture.expected);
}

#[test]
fn retained_capture_cannot_hide_incompatibility_with_later_compatible_bytes() {
    let fixture = Fixture::incompatible();
    let capture = fixture.capture().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra/package.yaml");
    fs::copy(source, fixture.path.join("package.yaml")).unwrap();
    assert!(inspect(&fixture.path).is_ok());
    assert!(matches!(
        capture.validate(),
        Err(InspectionError::DeclarationInvalid)
    ));
    assert!(fixture.capture().is_err());
}

#[test]
fn retained_capture_rejects_mutation_before_capture() {
    for mutation in ["append", "missing", "symlink"] {
        let fixture = Fixture::new();
        match mutation {
            "append" => fs::write(fixture.path.join("extra.txt"), "extra").unwrap(),
            "missing" => fs::remove_file(fixture.path.join("package.yaml")).unwrap(),
            "symlink" => {
                fs::remove_file(fixture.path.join("package.yaml")).unwrap();
                let source =
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra/package.yaml");
                std::os::unix::fs::symlink(source, fixture.path.join("package.yaml")).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            matches!(fixture.capture(), Err(InstallError::ObjectCorrupt)),
            "{mutation}"
        );
    }
}

//! Retained identity and current typed validation share one owned byte capture.
use super::{copy_tree, source_bytes};
use crate::package::{
    inspect, install_types::InstallError, install_types::PackageIdentity,
    object_store::PackageObjects, InspectionError, PackageCapture, PackageSnapshot,
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

impl Fixture {
    /// Publish a real validated Lyra package into an isolated object store.
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

    /// Exercise the production capture against the fixture's recorded identity.
    fn capture(&self) -> Result<PackageCapture, InstallError> {
        self.objects.capture(&self.identity)
    }

    /// Require both the complete byte inventory and package identity to survive.
    fn assert_original(&self, snapshot: PackageSnapshot) {
        assert_eq!(snapshot.identity(), self.identity);
        let actual: BTreeMap<_, _> = snapshot
            .files()
            .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
            .collect();
        assert_eq!(actual, self.expected);
    }

    /// Model historical recorded bytes, not a newly installable/validated snapshot.
    fn rewrite_retained(&mut self, member: &str, bytes: Vec<u8>) {
        self.expected.insert(member.to_owned(), bytes);
        self.identity.manifest_digest =
            openspine_schemas::digest::digest_of_bytes(&self.expected["package.yaml"]);
        self.identity.content_digest = crate::package::inventory_of(&self.expected).1;
        let old_path = self.path.clone();
        self.path = old_path.parent().unwrap().join(
            self.identity
                .content_digest
                .as_str()
                .strip_prefix("sha256:")
                .unwrap(),
        );
        fs::rename(old_path, &self.path).unwrap();
        fs::write(self.path.join(member), &self.expected[member]).unwrap();
    }

    /// Retain a matching raw identity whose schema the current loader rejects.
    fn incompatible() -> Self {
        let mut fixture = Self::new();
        let mut manifest: serde_yaml::Value =
            serde_yaml::from_slice(&fixture.expected["package.yaml"]).unwrap();
        manifest.as_mapping_mut().unwrap().insert(
            serde_yaml::Value::from("schema_version"),
            serde_yaml::Value::from(999),
        );
        fixture.rewrite_retained(
            "package.yaml",
            serde_yaml::to_string(&manifest).unwrap().into_bytes(),
        );
        fixture
    }
}

/// Validation must preserve both stored bytes and inspection metadata.
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

/// A prior capture remains usable after removal; a new capture detects the loss.
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

/// Structurally valid replacement bytes must not inherit the earlier identity.
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

/// A later corrupt manifest cannot replace already captured validation inputs.
#[test]
fn retained_capture_does_not_validate_later_corrupt_manifest() {
    let fixture = Fixture::new();
    let capture = fixture.capture().unwrap();
    fs::write(fixture.path.join("package.yaml"), "not a declaration").unwrap();
    fixture.assert_original(capture.validate().unwrap());
    assert!(fixture.capture().is_err());
}

/// Replacing the directory path must not redirect validation to another object.
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

/// Validation stays path-free even when the original name becomes a symlink.
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

/// Schema rejection must not relabel identity-matching retained bytes as corrupt.
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

/// A compatible replacement cannot conceal incompatibility in the captured bytes.
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

/// Capture must reject preexisting additions, missing files and linked members.
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

/// Identical copied content cannot bypass the opened namespace's anchor checks.
#[test]
fn retained_capture_rejects_replaced_namespace_anchors() {
    let fixture = Fixture::new();
    let namespace = fixture.path.parent().unwrap().parent().unwrap();
    let moved = namespace.with_file_name("saved-packages");
    fs::rename(namespace, &moved).unwrap();
    copy_tree(&moved, namespace);
    assert!(fixture.capture().is_err());
}

/// Raw identity verification does not substitute for current typed artifact checks.
#[test]
fn retained_capture_checks_current_typed_artifacts_not_just_declaration() {
    let mut fixture = Fixture::new();
    let member = fixture
        .expected
        .keys()
        .find(|path| path.starts_with("agents/"))
        .unwrap()
        .clone();
    fixture.rewrite_retained(&member, b"not an artifact".to_vec());
    let capture = fixture.capture().unwrap();
    assert!(matches!(
        capture.validate(),
        Err(InspectionError::ArtifactInvalid)
    ));
    fixture.objects.verify(&fixture.identity).unwrap();
    assert_eq!(source_bytes(&fixture.path), fixture.expected);
}

/// An isolated unavailable TMPDIR blocks validation, never retained integrity.
#[test]
fn retained_capture_staging_failure_is_not_corruption() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const ROOT_ENV: &str = "OPENSPINE_TEST_RETAINED_CAPTURE_ROOT";
    const CASE: &str =
        "package::tests::retained_capture::retained_capture_staging_failure_is_not_corruption";
    // Isolate TMPDIR in a subprocess; never mutate the parallel test process.
    if let Some(root) = std::env::var_os(ROOT_ENV) {
        let root = PathBuf::from(root);
        let identity: PackageIdentity =
            serde_json::from_slice(&fs::read(root.join("identity.json")).unwrap()).unwrap();
        let objects = PackageObjects::open(&root.join("data"), &root.join("active")).unwrap();
        let capture = objects.capture(&identity).unwrap();
        objects.verify(&identity).unwrap();
        assert!(matches!(
            capture.validate(),
            Err(InspectionError::StagingUnavailable)
        ));
        objects.verify(&identity).unwrap();
        return;
    }

    let fixture = Fixture::new();
    let root = fixture._root.path();
    let bad_temp = root.join("not-a-directory");
    fs::write(&bad_temp, "ordinary file").unwrap();
    fs::write(
        root.join("identity.json"),
        serde_json::to_vec(&fixture.identity).unwrap(),
    )
    .unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CASE])
        .env(ROOT_ENV, root)
        .env("TMPDIR", bad_temp)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("isolated retained-capture test did not finish: {result:?}");
            }
        }
    }
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed; 0 failed"),
        "isolated retained-capture test failed: {stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(source_bytes(&fixture.path), fixture.expected);
}

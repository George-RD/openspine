//! The committed selection is the only source; every failure is unavailable.
use super::{resolve, ManagedBaseError};
use crate::package::install_types::{InstallError, InstallReceipt, PackageProvenance};
use crate::package::object_store::{PackageObjectReader, PackageObjects};
use crate::package::{inspect, install, InspectionError};
use crate::store::package_selection_fixtures::{decision, select};
use crate::store::Store;
use jiff::Timestamp;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const MARKER: &str = "You are Lyra (selected revision), a concise";

fn lyra_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra")
}

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

fn read_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned();
                out.insert(name, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// A modified Lyra revision whose effective registry differs from the base.
pub(crate) fn variant(root: &Path, revision: u32) -> PathBuf {
    let source = root.join(format!("variant-{revision}"));
    copy(&lyra_dir(), &source);
    let manifest = source.join("package.yaml");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replace("version: 2", &format!("version: {revision}")),
    )
    .unwrap();
    let template = source.join("templates/owner_terminal_template.yaml");
    let text = fs::read_to_string(&template).unwrap();
    fs::write(&template, text.replace("You are Lyra, a concise", MARKER)).unwrap();
    source
}

pub(crate) struct Fixture {
    pub root: tempfile::TempDir,
    pub data: PathBuf,
    pub store: Store,
    pub objects: PackageObjects,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        // Canonical like the CLI's locked root; macOS temp paths contain aliases.
        let data = root.path().canonicalize().unwrap().join("data");
        fs::create_dir(&data).unwrap();
        let store = Store::open(&data.join("kernel.db")).unwrap();
        let objects = PackageObjects::open(&data, &root.path().join("active")).unwrap();
        Self {
            root,
            data,
            store,
            objects,
        }
    }

    pub(crate) fn install(&self, source: &Path) -> InstallReceipt {
        let snapshot = inspect(source).unwrap();
        install::install(
            &self.store,
            &self.objects,
            &snapshot,
            PackageProvenance::LocalUnverified,
        )
        .unwrap()
        .receipt
    }

    pub(crate) fn select(&self, receipt: &InstallReceipt) {
        select(&self.store, receipt, Timestamp::now());
    }

    pub(crate) fn object_dir(&self, receipt: &InstallReceipt) -> PathBuf {
        self.data.join("packages/objects").join(
            receipt
                .content_digest
                .as_str()
                .strip_prefix("sha256:")
                .unwrap(),
        )
    }
}

#[test]
fn an_instance_without_selection_history_stays_legacy_and_needs_no_package_namespace() {
    let fixture = Fixture::new();
    fs::remove_dir_all(fixture.data.join("packages")).unwrap();
    assert!(resolve(&fixture.store, &fixture.data).unwrap().is_none());
}

#[test]
fn the_selection_resolves_the_exact_retained_bytes_after_every_source_is_gone() {
    let fixture = Fixture::new();
    let source = variant(fixture.root.path(), 3);
    let receipt = fixture.install(&source);
    let expected = read_tree(&source);
    fs::remove_dir_all(&source).unwrap();
    fixture.select(&receipt);
    let base = resolve(&fixture.store, &fixture.data).unwrap().unwrap();
    assert_eq!(base.record.generation, 1);
    assert_eq!(base.receipt.installation_id, receipt.installation_id);
    assert_eq!(base.snapshot.identity(), receipt.identity());
    let actual: BTreeMap<_, _> = base
        .snapshot
        .files()
        .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
        .collect();
    assert_eq!(actual, expected);
    let template = &base.snapshot.registry().templates["owner_terminal_template"];
    assert!(template.system_preamble.starts_with(MARKER));
}

#[test]
fn reading_the_selected_object_does_not_weaken_the_publication_overlap_guard() {
    let fixture = Fixture::new();
    let receipt = fixture.install(&variant(fixture.root.path(), 3));
    fixture.select(&receipt);
    let objects = fixture.data.join("packages/objects");
    // A legacy base inside the retained namespace is still refused to publish.
    assert!(matches!(
        PackageObjects::open(&fixture.data, &objects),
        Err(InstallError::Destination)
    ));
    // The read path needs no legacy directory and never creates anything.
    PackageObjectReader::open(&fixture.data)
        .unwrap()
        .capture(&receipt.identity())
        .unwrap();
    PackageObjects::open_managed(&fixture.data).unwrap();
    fs::remove_dir_all(fixture.data.join("packages/staging")).unwrap();
    PackageObjectReader::open(&fixture.data).unwrap();
    assert!(!fixture.data.join("packages/staging").exists());
}

fn unavailable(fixture: &Fixture) -> ManagedBaseError {
    match resolve(&fixture.store, &fixture.data) {
        Err(error) => error,
        Ok(_) => panic!("a damaged selection must never resolve or fall back to another base"),
    }
}

/// A selected, installed variant whose retained object the case then damages.
fn selected() -> (Fixture, PathBuf) {
    let fixture = Fixture::new();
    let receipt = fixture.install(&variant(fixture.root.path(), 3));
    fixture.select(&receipt);
    let object = fixture.object_dir(&receipt);
    (fixture, object)
}

fn assert_object_corrupt(fixture: &Fixture) {
    assert!(matches!(
        unavailable(fixture),
        ManagedBaseError::ObjectCorrupt
    ));
}

#[test]
fn a_missing_selected_object_is_unavailable_never_a_fallback() {
    let (fixture, object) = selected();
    fs::remove_dir_all(object).unwrap();
    assert_object_corrupt(&fixture);
}

#[test]
fn modified_or_extra_selected_bytes_are_unavailable_never_a_fallback() {
    let (fixture, object) = selected();
    let file = object.join("templates/owner_terminal_template.yaml");
    let mut bytes = fs::read(&file).unwrap();
    bytes.push(b'#');
    fs::write(&file, bytes).unwrap();
    assert_object_corrupt(&fixture);
    fs::write(&file, fs::read(&file).unwrap().strip_suffix(b"#").unwrap()).unwrap();
    assert!(resolve(&fixture.store, &fixture.data).is_ok());
    fs::write(object.join("unindexed.yaml"), b"x: 1").unwrap();
    assert_object_corrupt(&fixture);
}

#[cfg(unix)]
#[test]
fn a_linked_selected_object_or_member_is_unavailable_never_a_fallback() {
    let (fixture, object) = selected();
    let template = object.join("templates/owner_terminal_template.yaml");
    let copy = object.join("templates/copy.yaml");
    fs::rename(&template, &copy).unwrap();
    std::os::unix::fs::symlink(&copy, &template).unwrap();
    assert_object_corrupt(&fixture);

    let (fixture, object) = selected();
    let moved = object.with_extension("moved");
    fs::rename(&object, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &object).unwrap();
    assert_object_corrupt(&fixture);
}

#[test]
fn a_missing_package_namespace_under_a_selection_is_unavailable_not_legacy() {
    let (fixture, _) = selected();
    fs::remove_dir_all(fixture.data.join("packages")).unwrap();
    assert_object_corrupt(&fixture);
}

#[test]
fn an_intact_but_incompatible_selected_package_is_a_distinct_unavailable_state() {
    let fixture = Fixture::new();
    let source = variant(fixture.root.path(), 3);
    // Retain bytes the current typed loader rejects, with matching identity:
    // historical bytes, not something `package install` would have accepted.
    let mut files = read_tree(&source);
    files.insert(
        "agents/email_reply_drafter.yaml".into(),
        b"not an artifact".to_vec(),
    );
    let identity = crate::package::install_types::PackageIdentity {
        package_id: "lyra".into(),
        revision: 3,
        inventory_format_version: 1,
        content_digest: crate::package::inventory_of(&files).1,
        manifest_digest: openspine_schemas::digest::digest_of_bytes(&files["package.yaml"]),
    };
    let object = fixture.data.join("packages/objects").join(
        identity
            .content_digest
            .as_str()
            .strip_prefix("sha256:")
            .unwrap(),
    );
    for (path, bytes) in &files {
        fs::create_dir_all(object.join(path).parent().unwrap()).unwrap();
        fs::write(object.join(path), bytes).unwrap();
    }
    let crate::store::package_install::PrepareResult::Prepared(metadata) = fixture
        .store
        .prepare_package_install(identity, PackageProvenance::LocalUnverified)
        .unwrap()
    else {
        panic!("fresh identity prepares");
    };
    let crate::store::package_install::CommitResult::Committed(receipt) =
        fixture.store.commit_package_install(&metadata).unwrap()
    else {
        panic!("prepared identity commits");
    };
    fixture.select(&receipt);
    assert!(matches!(
        unavailable(&fixture),
        ManagedBaseError::Incompatible(InspectionError::ArtifactInvalid)
    ));
}

#[test]
fn a_lost_or_altered_selection_ledger_is_never_legacy_mode() {
    let fixture = Fixture::new();
    let receipt = fixture.install(&variant(fixture.root.path(), 3));
    fixture.select(&receipt);
    fixture
        .store
        .with_immediate_tx(|tx| {
            tx.execute_batch("DELETE FROM package_selection;")?;
            Ok(())
        })
        .unwrap();
    assert!(matches!(unavailable(&fixture), ManagedBaseError::Ledger));
    // And the decision API cannot be replayed to recreate it quietly.
    let other = fixture.install(&variant(fixture.root.path(), 4));
    let stale = decision(&fixture.store, &other, Timestamp::now());
    assert!(fixture
        .store
        .record_package_selection_decision(&stale, Timestamp::now())
        .is_err());
}

use super::*;
use std::fs;
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn key() -> [u8; 32] {
    *b"0123456789abcdef0123456789abcdef"
}

fn request() -> BundleRequestMetadata {
    BundleRequestMetadata {
        request_id: "req_01".into(),
        action_id: "openspine.overlay.export".into(),
        owner_principal_id: "owner".into(),
        grant_id: "grant_01".into(),
        requested_at: "2026-07-23T00:00:00Z".into(),
    }
}

fn ledger() -> TerminalLedgerBaseline {
    TerminalLedgerBaseline {
        // Fixed ULID so fixtures stay deterministic across runs.
        continuity_id: "01J00000000000000000000000".into(),
        sequence: 3,
        erased_counterparty_ids: vec!["cp_a".into(), "cp_b".into()],
        ledger_hmac_sha256: "11".repeat(32),
    }
}

fn write_tree(root: &Path) {
    fs::create_dir_all(root.join("keys")).unwrap();
    fs::create_dir_all(root.join("artifacts")).unwrap();
    fs::create_dir_all(root.join("artifacts.d").join("empty_keep")).unwrap();
    fs::write(root.join("kernel.db"), b"sqlite").unwrap();
    fs::write(root.join("keys").join("scope.wrapped"), b"wrapped-key").unwrap();
    fs::write(root.join("artifacts").join("blob.bin"), b"payload").unwrap();
    // Tombstones are regular files.
    fs::write(root.join("keys").join("cp_a.erased"), b"").unwrap();
}

fn snapshot_layout() -> (TempDir, PathBuf, PathBuf) {
    let temp = TempDir::new().unwrap();
    let snapshots = temp.path().join("snapshots");
    let data = temp.path().join("data-root");
    fs::create_dir_all(&snapshots).unwrap();
    fs::create_dir_all(&data).unwrap();
    write_tree(&data);
    (temp, snapshots, data)
}

#[test]
fn typed_tree_roundtrip_preserves_empty_directories() {
    let (_temp, snapshots, data) = snapshot_layout();
    let manifest =
        publish_bundle(&snapshots, "roundtrip", &data, request(), ledger(), &key()).unwrap();
    assert!(manifest.entries().iter().any(|e| matches!(
        e,
        BundleEntry::Directory { path } if path == "data/artifacts.d/empty_keep"
    )));
    assert!(manifest.entries().iter().any(|e| matches!(
        e,
        BundleEntry::RegularFile { path, .. } if path == "data/keys/cp_a.erased"
    )));

    let staging = snapshots.parent().unwrap().join("staging");
    let staged = stage_bundle(&snapshots.join("roundtrip"), &staging, &key()).unwrap();
    assert_eq!(staged.entries(), manifest.entries());
    assert!(staging.join("artifacts.d").join("empty_keep").is_dir());
    assert_eq!(
        fs::read(staging.join("artifacts").join("blob.bin")).unwrap(),
        b"payload"
    );
    assert_eq!(
        fs::read(staging.join("keys").join("cp_a.erased")).unwrap(),
        b""
    );
}

#[test]
fn rejects_extra_empty_erased_directory() {
    let (_temp, snapshots, data) = snapshot_layout();
    let manifest = publish_bundle(&snapshots, "extra", &data, request(), ledger(), &key()).unwrap();
    let bundle_data = snapshots.join("extra").join("data");
    fs::create_dir_all(bundle_data.join("keys").join("unexpected.erased")).unwrap();
    let err = validate_tree(&bundle_data, manifest.entries()).unwrap_err();
    assert!(matches!(
        err,
        BundleError::TreeMismatch(_) | BundleError::InvalidManifest(_)
    ));
}

#[test]
fn rejects_erased_directory_in_source() {
    let (_temp, snapshots, data) = snapshot_layout();
    fs::create_dir_all(data.join("keys").join("cp_b.erased")).unwrap();
    let err =
        publish_bundle(&snapshots, "erased-dir", &data, request(), ledger(), &key()).unwrap_err();
    assert!(matches!(err, BundleError::TreeMismatch(_)));
    assert!(!snapshots.join("erased-dir").exists());
}

#[test]
fn fixed_permissions_are_applied() {
    let (_temp, snapshots, data) = snapshot_layout();
    publish_bundle(&snapshots, "perms", &data, request(), ledger(), &key()).unwrap();
    let root = snapshots.join("perms");
    assert_eq!(
        fs::metadata(root.join("data")).unwrap().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(root.join("data").join("keys")).unwrap().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(root.join("data").join("kernel.db"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(root.join("manifest.json")).unwrap().mode() & 0o777,
        0o600
    );
}

#[test]
fn rejects_symlink_in_source_tree() {
    let (_temp, snapshots, data) = snapshot_layout();
    std::os::unix::fs::symlink("kernel.db", data.join("alias")).unwrap();
    let err = publish_bundle(&snapshots, "sym", &data, request(), ledger(), &key()).unwrap_err();
    assert!(matches!(err, BundleError::TreeMismatch(_)));
    assert!(!snapshots.join("sym").exists());
}

#[test]
fn rejects_wrong_type_directory_as_file() {
    let (_temp, snapshots, data) = snapshot_layout();
    let mut entries = enumerate_data_root(&data).unwrap();
    entries.retain(|e| !e.path().starts_with("data/keys"));
    entries.push(BundleEntry::RegularFile {
        path: "data/keys".into(),
        byte_length: 1,
        sha256: "00".repeat(32),
    });
    entries.sort_by(|a, b| a.path().cmp(b.path()));
    let dest = snapshots.join("wrong-type-dest");
    fs::create_dir_all(&dest).unwrap();
    let err = copy_entries(&data, &dest, &entries).unwrap_err();
    assert!(matches!(
        err,
        BundleError::TreeMismatch(_) | BundleError::ConcurrentMutation(_)
    ));
}

#[test]
fn rejects_duplicate_and_missing_parent_entries() {
    let duplicates = vec![
        BundleEntry::Directory {
            path: "data".into(),
        },
        BundleEntry::RegularFile {
            path: "data/kernel.db".into(),
            byte_length: 1,
            sha256: "00".repeat(32),
        },
        BundleEntry::RegularFile {
            path: "data/kernel.db".into(),
            byte_length: 1,
            sha256: "00".repeat(32),
        },
    ];
    assert!(matches!(
        validate_entries(&duplicates),
        Err(BundleError::InvalidManifest(_))
    ));

    let missing_parent = vec![
        BundleEntry::Directory {
            path: "data".into(),
        },
        BundleEntry::RegularFile {
            path: "data/keys/scope.wrapped".into(),
            byte_length: 1,
            sha256: "00".repeat(32),
        },
    ];
    assert!(matches!(
        validate_entries(&missing_parent),
        Err(BundleError::InvalidManifest(_))
    ));
}

#[test]
fn rejects_extra_and_missing_files() {
    let (_temp, snapshots, data) = snapshot_layout();
    let manifest =
        publish_bundle(&snapshots, "extrafile", &data, request(), ledger(), &key()).unwrap();
    fs::write(
        snapshots.join("extrafile").join("data").join("surprise"),
        b"!",
    )
    .unwrap();
    assert!(matches!(
        validate_tree(
            &snapshots.join("extrafile").join("data"),
            manifest.entries()
        ),
        Err(BundleError::TreeMismatch(_))
    ));
    fs::remove_file(snapshots.join("extrafile").join("data").join("surprise")).unwrap();
    fs::remove_file(snapshots.join("extrafile").join("data").join("kernel.db")).unwrap();
    assert!(matches!(
        validate_tree(
            &snapshots.join("extrafile").join("data"),
            manifest.entries()
        ),
        Err(BundleError::TreeMismatch(_))
    ));
}

#[test]
fn rejects_digest_mismatch_and_content_mutation() {
    let (_temp, snapshots, data) = snapshot_layout();
    let dest = snapshots.join("mut");
    fs::create_dir_all(&dest).unwrap();
    // Wrong digest.
    let root = open_dir_nofollow(&data).unwrap();
    let (parent, name, display) =
        open_parent_fd(root.as_raw_fd(), &data, "data/kernel.db").unwrap();
    let mut src = openat_file(parent.as_raw_fd(), &name).unwrap();
    let before = src.metadata().unwrap();
    let err = copy_from_open(
        &mut src,
        &display,
        &before,
        &dest.join("k1"),
        6,
        &"ff".repeat(32),
    )
    .unwrap_err();
    assert!(matches!(err, BundleError::ConcurrentMutation(_)));
    drop(src);
    drop(parent);
    drop(root);

    // Content mutation after hash capture.
    let root = open_dir_nofollow(&data).unwrap();
    let (parent, name, display) =
        open_parent_fd(root.as_raw_fd(), &data, "data/kernel.db").unwrap();
    let mut src = openat_file(parent.as_raw_fd(), &name).unwrap();
    let before = src.metadata().unwrap();
    let (len, digest) = hash_open(&mut src, &display, &before).unwrap();
    drop(src);
    drop(parent);
    drop(root);
    fs::write(data.join("kernel.db"), b"changed").unwrap();
    let root = open_dir_nofollow(&data).unwrap();
    let (parent, name, display) =
        open_parent_fd(root.as_raw_fd(), &data, "data/kernel.db").unwrap();
    let mut src = openat_file(parent.as_raw_fd(), &name).unwrap();
    let before = src.metadata().unwrap();
    let err =
        copy_from_open(&mut src, &display, &before, &dest.join("k2"), len, &digest).unwrap_err();
    assert!(matches!(err, BundleError::ConcurrentMutation(_)));
}

#[test]
fn invalid_manifest_hmac_is_rejected() {
    let (_temp, snapshots, data) = snapshot_layout();
    publish_bundle(&snapshots, "hmac", &data, request(), ledger(), &key()).unwrap();
    let path = snapshots.join("hmac").join("manifest.json");
    let mut manifest: BundleManifest = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest.hmac_sha256 = "aa".repeat(32);
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(matches!(
        verify_bundle(&snapshots.join("hmac"), &key()),
        Err(BundleError::InvalidHmac)
    ));
}

#[test]
fn valid_manifest_hmac_verifies() {
    let (_temp, snapshots, data) = snapshot_layout();
    let published =
        publish_bundle(&snapshots, "okhmac", &data, request(), ledger(), &key()).unwrap();
    let verified = verify_bundle(&snapshots.join("okhmac"), &key()).unwrap();
    assert_eq!(verified, published);
    assert_eq!(verified.ledger_baseline().sequence, 3);
    assert_eq!(verified.request().action_id, "openspine.overlay.export");
}

#[test]
fn partial_publication_cleans_temp_after_source_failure() {
    let (_temp, snapshots, data) = snapshot_layout();
    // Induce failure after temp creation: .erased must be a regular file.
    fs::create_dir_all(data.join("keys").join("mid_export.erased")).unwrap();
    let err =
        publish_bundle(&snapshots, "partial", &data, request(), ledger(), &key()).unwrap_err();
    assert!(matches!(err, BundleError::TreeMismatch(_)), "got {err:?}");
    let leftovers: Vec<_> = fs::read_dir(&snapshots)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.contains(".tmp-") || n.starts_with(".partial") || n == "partial")
        .collect();
    assert!(leftovers.is_empty(), "leftover temps/final: {leftovers:?}");
    assert!(!snapshots.join("partial").exists());
}

#[test]
fn refuses_to_replace_existing_final_bundle() {
    let (_temp, snapshots, data) = snapshot_layout();
    publish_bundle(&snapshots, "once", &data, request(), ledger(), &key()).unwrap();
    let err = publish_bundle(&snapshots, "once", &data, request(), ledger(), &key()).unwrap_err();
    assert!(matches!(err, BundleError::AlreadyExists(_)));
}

#[test]
fn stage_rejects_source_mutation_before_install() {
    let (_temp, snapshots, data) = snapshot_layout();
    publish_bundle(&snapshots, "stage", &data, request(), ledger(), &key()).unwrap();
    fs::write(
        snapshots.join("stage").join("data").join("kernel.db"),
        b"tampered",
    )
    .unwrap();
    let staging = snapshots.parent().unwrap().join("stage-out");
    let err = stage_bundle(&snapshots.join("stage"), &staging, &key()).unwrap_err();
    assert!(matches!(
        err,
        BundleError::TreeMismatch(_) | BundleError::ConcurrentMutation(_)
    ));
    assert!(!staging.exists());
}

#[test]
fn master_key_bytes_are_not_written_into_bundle() {
    let (_temp, snapshots, data) = snapshot_layout();
    publish_bundle(&snapshots, "noky", &data, request(), ledger(), &key()).unwrap();
    let walk = walkdir_bytes(&snapshots.join("noky"));
    assert!(!walk.windows(key().len()).any(|w| w == key()));
}

#[test]
fn apply_terminal_erasure_delta_removes_key_and_pending_alias() {
    let (_temp, snapshots, data) = snapshot_layout();
    // Live key + pending publication marker for a baseline id.
    fs::write(data.join("keys").join("cp_b"), b"wrapped-live").unwrap();
    fs::write(data.join("keys").join("cp_b.migpending"), b"pending").unwrap();
    // Unrelated key must survive the authorized delta.
    fs::write(data.join("keys").join("cp_other"), b"wrapped-other").unwrap();
    // Later merged-ledger id (not in baseline) supplied by the facade.
    fs::write(data.join("keys").join("cp_local"), b"wrapped-local").unwrap();

    let manifest = publish_bundle(&snapshots, "delta", &data, request(), ledger(), &key()).unwrap();
    let staging = snapshots.parent().unwrap().join("delta-staging");
    let staged = stage_bundle(&snapshots.join("delta"), &staging, &key()).unwrap();
    assert_eq!(staged.entries(), manifest.entries());
    assert_eq!(
        manifest.ledger_baseline().continuity_id,
        "01J00000000000000000000000"
    );

    // Malformed ids are rejected before any mutation.
    let invalid =
        apply_terminal_erasure_delta(&staging, &manifest, &["bad.id".into()]).unwrap_err();
    assert!(matches!(invalid, BundleError::InvalidManifest(_)));
    assert!(staging.join("keys").join("cp_b").exists());
    assert!(staging.join("keys").join("cp_other").exists());
    assert!(staging.join("keys").join("cp_local").exists());

    // Facade-supplied authorized set may include later merged ids.
    apply_terminal_erasure_delta(&staging, &manifest, &["cp_b".into(), "cp_local".into()]).unwrap();

    assert!(!staging.join("keys").join("cp_b").exists());
    assert!(!staging.join("keys").join("cp_b.migpending").exists());
    assert!(!staging.join("keys").join("cp_local").exists());
    assert!(staging.join("keys").join("cp_b.erased").is_file());
    assert!(staging.join("keys").join("cp_local.erased").is_file());
    assert_eq!(
        fs::read(staging.join("keys").join("cp_b.erased")).unwrap(),
        b""
    );
    assert_eq!(
        fs::read(staging.join("keys").join("cp_local.erased")).unwrap(),
        b""
    );
    assert_eq!(
        fs::read(staging.join("keys").join("cp_other")).unwrap(),
        b"wrapped-other"
    );
    // Pre-existing tombstone from the source tree remains intact.
    assert_eq!(
        fs::read(staging.join("keys").join("cp_a.erased")).unwrap(),
        b""
    );

    // Final tree must match the manifest delta exactly (no extra mutations).
    let expected = enumerate_data_root(&staging).unwrap();
    validate_tree(&staging, &expected).unwrap();
    assert!(expected.iter().any(|entry| {
        matches!(
            entry,
            BundleEntry::RegularFile { path, byte_length: 0, .. }
                if path == "data/keys/cp_b.erased"
        )
    }));
    assert!(expected.iter().any(|entry| {
        matches!(
            entry,
            BundleEntry::RegularFile { path, byte_length: 0, .. }
                if path == "data/keys/cp_local.erased"
        )
    }));
    assert!(!expected
        .iter()
        .any(|entry| entry.path() == "data/keys/cp_b"));
    assert!(!expected
        .iter()
        .any(|entry| entry.path() == "data/keys/cp_b.migpending"));
    assert!(!expected
        .iter()
        .any(|entry| entry.path() == "data/keys/cp_local"));
    assert!(expected
        .iter()
        .any(|entry| entry.path() == "data/keys/cp_other"));
}

#[test]
fn continuity_id_is_hmac_bound_and_validated() {
    let (_temp, snapshots, data) = snapshot_layout();
    publish_bundle(&snapshots, "cont", &data, request(), ledger(), &key()).unwrap();
    let path = snapshots.join("cont").join("manifest.json");
    let raw = fs::read(&path).unwrap();
    let text = String::from_utf8(raw.clone()).unwrap();
    // Mutate the authenticated baseline continuity identity.
    let mutated = text.replace("01J00000000000000000000000", "01J00000000000000000000001");
    assert_ne!(mutated, text);
    fs::write(&path, mutated.as_bytes()).unwrap();
    let err = verify_bundle(&snapshots.join("cont"), &key()).unwrap_err();
    assert!(matches!(
        err,
        BundleError::InvalidHmac | BundleError::InvalidManifest(_)
    ));

    // Malformed continuity_id rejected at publish.
    let mut bad = ledger();
    bad.continuity_id = "not valid!".into();
    let err = publish_bundle(&snapshots, "bad-cont", &data, request(), bad, &key()).unwrap_err();
    assert!(matches!(err, BundleError::InvalidManifest(_)));
    assert!(!snapshots.join("bad-cont").exists());
}

fn walkdir_bytes(root: &Path) -> Vec<u8> {
    let mut out = Vec::new();
    fn walk(path: &Path, out: &mut Vec<u8>) {
        let meta = fs::symlink_metadata(path).unwrap();
        if meta.file_type().is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                walk(&entry.unwrap().path(), out);
            }
        } else if meta.file_type().is_file() {
            out.extend(fs::read(path).unwrap());
        }
    }
    walk(root, &mut out);
    out
}

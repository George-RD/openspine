//! Focused unit tests for bundle authentication, metadata, and durability functions.

use super::manifest::read_manifest;
use super::*;
use std::fs;
use tempfile::TempDir;

fn key() -> Vec<u8> {
    b"openspine-bundle-test-key-32bytes!".to_vec()
}

fn request() -> BundleRequestMetadata {
    BundleRequestMetadata {
        request_id: "req_01".into(),
        action_id: "openspine.overlay.export".into(),
        owner_principal_id: "owner_alice".into(),
        grant_id: "grant_01".into(),
        requested_at: "2026-07-23T12:00:00Z".into(),
    }
}

fn ledger() -> TerminalLedgerBaseline {
    TerminalLedgerBaseline {
        continuity_id: "cont01".into(),
        sequence: 1,
        erased_counterparty_ids: vec![],
        ledger_hmac_sha256: "00".repeat(32),
    }
}

fn snapshot_layout() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = TempDir::new().unwrap();
    let snapshots = temp.path().join("snapshots");
    let data = temp.path().join("data-root");
    fs::create_dir_all(&snapshots).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("kernel.db"), b"sqlite").unwrap();
    fs::create_dir_all(data.join("keys")).unwrap();
    fs::write(data.join("keys").join("k1"), b"wrapped").unwrap();
    (temp, snapshots, data)
}

#[test]
fn verify_named_bundle_rejects_name_mismatch_on_signed_bundle() {
    let (_temp, snapshots, data) = snapshot_layout();
    let manifest = publish_bundle(
        &snapshots,
        "bundle-orig",
        &data,
        request(),
        ledger(),
        &key(),
    )
    .unwrap();
    assert_eq!(manifest.bundle_name(), "bundle-orig");

    // Re-verify under true name: succeeds.
    let verified =
        verify_named_bundle(&snapshots.join("bundle-orig"), "bundle-orig", &key()).unwrap();
    assert_eq!(verified.bundle_name(), "bundle-orig");

    // Verify under wrong name: fails with InvalidBundleName.
    let err =
        verify_named_bundle(&snapshots.join("bundle-orig"), "bundle-other", &key()).unwrap_err();
    assert!(matches!(err, BundleError::InvalidBundleName));
}

#[test]
fn publish_bundle_serializes_and_authenticates_full_request_metadata() {
    let (_temp, snapshots, data) = snapshot_layout();
    let req = request();
    let manifest = publish_bundle(
        &snapshots,
        "meta-test",
        &data,
        req.clone(),
        ledger(),
        &key(),
    )
    .unwrap();

    assert_eq!(manifest.request(), &req);
    assert_eq!(manifest.request().request_id, "req_01");
    assert_eq!(manifest.request().action_id, "openspine.overlay.export");
    assert_eq!(manifest.request().owner_principal_id, "owner_alice");
    assert_eq!(manifest.request().grant_id, "grant_01");
    assert_eq!(manifest.request().requested_at, "2026-07-23T12:00:00Z");

    // Reading raw manifest confirms HMAC verification.
    let read_back =
        read_manifest(&snapshots.join("meta-test").join("manifest.json"), &key()).unwrap();
    assert_eq!(read_back.request(), &req);
}

#[test]
fn sync_existing_bundle_succeeds_on_published_bundle() {
    let (_temp, snapshots, data) = snapshot_layout();
    let _manifest =
        publish_bundle(&snapshots, "sync-test", &data, request(), ledger(), &key()).unwrap();

    // Re-syncing existing bundle directory executes bottom-up data sync and parent sync cleanly.
    sync_existing_bundle(&snapshots, "sync-test").unwrap();
}
#[test]
fn rejects_fifo_in_source_tree() {
    let temp = TempDir::new().unwrap();
    let snapshots = temp.path().join("snapshots");
    let data = temp.path().join("data-root");
    fs::create_dir_all(&snapshots).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("kernel.db"), b"sqlite").unwrap();

    let fifo_path = data.join("fifo_special");
    let c_path = std::ffi::CString::new(fifo_path.to_str().unwrap()).unwrap();
    let rc = unsafe { libc_mkfifo(c_path.as_ptr(), 0o600) };
    assert_eq!(rc, 0);

    let err =
        publish_bundle(&snapshots, "fifo-src", &data, request(), ledger(), &key()).unwrap_err();
    assert!(matches!(err, BundleError::TreeMismatch(_)));
    assert!(!snapshots.join("fifo-src").exists());
}

#[test]
fn rejects_fifo_in_staged_bundle() {
    let temp = TempDir::new().unwrap();
    let snapshots = temp.path().join("snapshots");
    let data = temp.path().join("data-root");
    fs::create_dir_all(&snapshots).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("kernel.db"), b"sqlite").unwrap();

    let manifest =
        publish_bundle(&snapshots, "fifo-stage", &data, request(), ledger(), &key()).unwrap();
    let staged_data = snapshots.join("fifo-stage").join("data");
    let fifo_path = staged_data.join("fifo_special");
    let c_path = std::ffi::CString::new(fifo_path.to_str().unwrap()).unwrap();
    let rc = unsafe { libc_mkfifo(c_path.as_ptr(), 0o600) };
    assert_eq!(rc, 0);

    let err = validate_tree(&staged_data, manifest.entries()).unwrap_err();
    assert!(matches!(err, BundleError::TreeMismatch(_)));

    let staging = snapshots.parent().unwrap().join("fifo-staging");
    let stage_err = stage_bundle(&snapshots.join("fifo-stage"), &staging, &key()).unwrap_err();
    assert!(matches!(
        stage_err,
        BundleError::TreeMismatch(_) | BundleError::ConcurrentMutation(_)
    ));
}

#[test]
fn read_manifest_rejects_fifo_without_blocking() {
    let (_temp, snapshots, data) = snapshot_layout();
    publish_bundle(
        &snapshots,
        "fifo-manifest",
        &data,
        request(),
        ledger(),
        &key(),
    )
    .unwrap();
    let manifest_path = snapshots.join("fifo-manifest").join("manifest.json");
    fs::remove_file(&manifest_path).unwrap();
    let c_path = std::ffi::CString::new(manifest_path.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc_mkfifo(c_path.as_ptr(), 0o600) }, 0);

    let err = read_manifest(&manifest_path, &key()).unwrap_err();
    assert!(matches!(err, BundleError::TreeMismatch(_)));
}

unsafe fn libc_mkfifo(path: *const std::os::raw::c_char, mode: u32) -> i32 {
    unsafe extern "C" {
        fn mkfifo(path: *const std::os::raw::c_char, mode: u32) -> i32;
    }
    unsafe { mkfifo(path, mode) }
}

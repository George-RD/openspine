//! Authenticated overlay export/restore bundle: exact typed tree, HMAC-SHA256,
//! no-follow copy-while-hash, fixed 0700/0600 modes, atomic publish.

mod delta;
mod durable;
mod manifest;
mod tree;
#[cfg(unix)]
mod unix_fd;

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

use durable::{
    atomic_rename_noreplace, create_dir, create_temp_dir, ensure_absent, require_bundle_shape,
    require_dir, sync_dir, write_manifest, Cleanup,
};
use manifest::{read_manifest, sign_manifest, validate_body, validate_bundle_name, ManifestBody};
use tree::{copy_entries, sync_tree_bottom_up};

// Parent-facing API wrappers and type aliases stay on this module so sibling
// modules continue to resolve them as `bundle::<name>`.

/// Apply an authorized terminal-erasure delta to a staged data root.
pub(super) fn apply_terminal_erasure_delta(
    staging_data_root: &Path,
    manifest: &BundleManifest,
    erased_ids: &[String],
) -> Result<(), BundleError> {
    delta::apply_terminal_erasure_delta(staging_data_root, manifest, erased_ids)
}

pub(super) type BundleManifest = manifest::BundleManifest;

#[cfg(test)]
pub(super) fn validate_entries(entries: &[BundleEntry]) -> Result<(), BundleError> {
    tree::validate_entries(entries)
}

#[cfg(test)]
pub(super) fn validate_tree(root: &Path, expected: &[BundleEntry]) -> Result<(), BundleError> {
    tree::validate_tree(root, expected)
}

#[cfg(test)]
pub(super) fn enumerate_data_root(root: &Path) -> Result<Vec<BundleEntry>, BundleError> {
    tree::enumerate_data_root(root)
}

#[cfg(test)]
pub(super) fn copy_from_open(
    source: &mut std::fs::File,
    source_path: &Path,
    before: &std::fs::Metadata,
    destination_path: &Path,
    expected_len: u64,
    expected_digest: &str,
) -> Result<(), BundleError> {
    tree::copy_from_open(
        source,
        source_path,
        before,
        destination_path,
        expected_len,
        expected_digest,
    )
}

#[cfg(test)]
pub(super) fn hash_open(
    file: &mut std::fs::File,
    path: &Path,
    before: &std::fs::Metadata,
) -> Result<(u64, String), BundleError> {
    tree::hash_open(file, path, before)
}

#[cfg(all(test, unix))]
pub(super) fn open_dir_nofollow(path: &Path) -> Result<std::os::fd::OwnedFd, BundleError> {
    unix_fd::open_dir_nofollow(path)
}

#[cfg(all(test, unix))]
pub(super) fn open_parent_fd(
    root_fd: std::os::unix::io::RawFd,
    root_display: &Path,
    manifest_path: &str,
) -> Result<(std::os::fd::OwnedFd, String, std::path::PathBuf), BundleError> {
    unix_fd::open_parent_fd(root_fd, root_display, manifest_path)
}

#[cfg(all(test, unix))]
pub(super) fn openat_file(
    dir_fd: std::os::unix::io::RawFd,
    name: &str,
) -> Result<std::fs::File, std::io::Error> {
    unix_fd::openat_file(dir_fd, name)
}

const BUNDLE_VERSION: u32 = 1;
const MANIFEST_FILE: &str = "manifest.json";
const DATA_DIR: &str = "data";

#[derive(Debug, Error)]
pub(super) enum BundleError {
    #[error("bundle path already exists: {0}")]
    AlreadyExists(std::path::PathBuf),
    #[error("invalid bundle name")]
    InvalidBundleName,
    #[error("invalid bundle manifest: {0}")]
    InvalidManifest(String),
    #[error("bundle manifest authentication failed")]
    InvalidHmac,
    #[error("bundle tree mismatch: {0}")]
    TreeMismatch(String),
    #[error("source changed while it was copied: {0}")]
    ConcurrentMutation(std::path::PathBuf),
    #[error("{operation} failed for {path}: {source}")]
    Io {
        operation: &'static str,
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BundleRequestMetadata {
    pub(super) request_id: String,
    pub(super) action_id: String,
    pub(super) owner_principal_id: String,
    pub(super) grant_id: String,
    pub(super) requested_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TerminalLedgerBaseline {
    pub(super) continuity_id: String,
    pub(super) sequence: u64,
    pub(super) erased_counterparty_ids: Vec<String>,
    pub(super) ledger_hmac_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum BundleEntry {
    Directory {
        path: String,
    },
    RegularFile {
        path: String,
        byte_length: u64,
        sha256: String,
    },
}

impl BundleEntry {
    pub(super) fn path(&self) -> &str {
        match self {
            Self::Directory { path } | Self::RegularFile { path, .. } => path,
        }
    }
}

pub(super) fn publish_bundle(
    snapshot_root: &Path,
    bundle_name: &str,
    data_root: &Path,
    request: BundleRequestMetadata,
    terminal_ledger_baseline: TerminalLedgerBaseline,
    master_key: &[u8],
) -> Result<BundleManifest, BundleError> {
    validate_bundle_name(bundle_name)?;
    require_dir(snapshot_root)?;
    if master_key.is_empty() {
        return Err(BundleError::InvalidManifest("empty master key".into()));
    }
    let final_path = snapshot_root.join(bundle_name);
    ensure_absent(&final_path)?;
    let temp = create_temp_dir(snapshot_root, bundle_name)?;
    let mut cleanup = Cleanup::new(temp.clone());
    let entries = tree::enumerate_data_root(data_root)?;
    let body = ManifestBody {
        version: BUNDLE_VERSION,
        bundle_name: bundle_name.to_owned(),
        request,
        terminal_ledger_baseline,
        entries,
    };
    validate_body(&body)?;
    let staged = temp.join(DATA_DIR);
    create_dir(&staged)?;
    copy_entries(data_root, &staged, &body.entries)?;
    tree::validate_tree(data_root, &body.entries)?;
    tree::validate_tree(&staged, &body.entries)?;
    sync_tree_bottom_up(&staged)?;
    let manifest = sign_manifest(body, master_key)?;
    write_manifest(&temp.join(MANIFEST_FILE), &manifest)?;
    sync_dir(&temp)?;
    atomic_rename_noreplace(&temp, &final_path)?;
    cleanup.keep();
    sync_dir(snapshot_root)?;
    Ok(manifest)
}

pub(super) fn verify_bundle(
    bundle_dir: &Path,
    master_key: &[u8],
) -> Result<BundleManifest, BundleError> {
    require_bundle_shape(bundle_dir)?;
    let manifest = read_manifest(&bundle_dir.join(MANIFEST_FILE), master_key)?;
    tree::validate_tree(&bundle_dir.join(DATA_DIR), &manifest.body.entries)?;
    Ok(manifest)
}
pub(super) fn verify_named_bundle(
    bundle_dir: &Path,
    expected_name: &str,
    master_key: &[u8],
) -> Result<BundleManifest, BundleError> {
    let manifest = verify_bundle(bundle_dir, master_key)?;
    if manifest.bundle_name() != expected_name {
        return Err(BundleError::InvalidBundleName);
    }
    Ok(manifest)
}
pub(super) fn sync_existing_bundle(
    snapshot_root: &Path,
    bundle_name: &str,
) -> Result<(), BundleError> {
    let bundle_dir = snapshot_root.join(bundle_name);
    tree::sync_tree_bottom_up(&bundle_dir.join(DATA_DIR))?;
    durable::sync_dir(&bundle_dir)?;
    durable::sync_dir(snapshot_root)?;
    Ok(())
}

pub(super) fn stage_bundle(
    bundle_dir: &Path,
    staging_data_root: &Path,
    master_key: &[u8],
) -> Result<BundleManifest, BundleError> {
    ensure_absent(staging_data_root)?;
    let manifest = verify_bundle(bundle_dir, master_key)?;
    create_dir(staging_data_root)?;
    let mut cleanup = Cleanup::new(staging_data_root.to_path_buf());
    copy_entries(
        &bundle_dir.join(DATA_DIR),
        staging_data_root,
        &manifest.body.entries,
    )?;
    tree::validate_tree(&bundle_dir.join(DATA_DIR), &manifest.body.entries)?;
    tree::validate_tree(staging_data_root, &manifest.body.entries)?;
    sync_tree_bottom_up(staging_data_root)?;
    cleanup.keep();
    Ok(manifest)
}

#[cfg(test)]
#[path = "bundle_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "bundle_hardening_tests.rs"]
mod hardening_tests;

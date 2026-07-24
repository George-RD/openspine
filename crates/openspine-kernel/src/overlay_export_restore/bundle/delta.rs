//! Authorized terminal-erasure deltas applied to staged overlay data trees.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;

use sha2::{Digest as _, Sha256};

use super::durable::{
    create_dir, io_err, path_exists_nofollow, require_dir, sync_dir, valid_name, write_empty_file,
};
use super::manifest::{decode_hex_32, hex};
use super::tree::{relative_data_path, sync_tree_bottom_up, validate_tree};
use super::{BundleEntry, BundleError, BundleManifest};

/// Apply an authorized terminal-erasure delta to a staged data root.
///
/// For each requested counterparty id this:
/// 1. writes and fsyncs a regular `keys/<id>.erased` tombstone,
/// 2. fsyncs the keys directory,
/// 3. deletes the staged wrapped key and pending-alias marker if present,
/// 4. fsyncs the keys directory again after unlinks.
///
/// `erased_ids` is the already-authorized terminal-erasure set supplied by
/// the facade (the verified live merged ledger may be a superset of the
/// bundle baseline). This helper validates id format only and applies the
/// deterministic typed-tree delta. The final staged typed tree must equal
/// the source manifest tree plus exactly those tombstones and minus exactly
/// the removed key and pending-alias files for those ids.
pub(super) fn apply_terminal_erasure_delta(
    staging_data_root: &Path,
    manifest: &BundleManifest,
    erased_ids: &[String],
) -> Result<(), BundleError> {
    require_dir(staging_data_root)?;
    let ids = normalize_erased_ids(erased_ids)?;
    if ids.is_empty() {
        validate_tree(staging_data_root, manifest.entries())?;
        return Ok(());
    }

    let expected = expected_entries_after_delta(manifest.entries(), &ids)?;
    let keys_dir = staging_data_root.join("keys");
    if !path_exists_nofollow(&keys_dir)? {
        create_dir(&keys_dir)?;
    } else {
        require_dir(&keys_dir)?;
    }

    // Crash-safe key-ring ordering: durable tombstones first, then unlinks.
    for id in &ids {
        let tombstone = keys_dir.join(format!("{id}.erased"));
        if path_exists_nofollow(&tombstone)? {
            let metadata = fs::symlink_metadata(&tombstone)
                .map_err(|source| io_err("inspect tombstone", &tombstone, source))?;
            if !metadata.is_file() {
                return Err(BundleError::TreeMismatch(format!(
                    "tombstone is not a regular file: {}",
                    tombstone.display()
                )));
            }
        } else {
            write_empty_file(&tombstone)?;
        }
    }
    sync_dir(&keys_dir)?;

    for id in &ids {
        remove_if_present(&keys_dir.join(id))?;
        remove_if_present(&keys_dir.join(format!("{id}.migpending")))?;
    }
    sync_dir(&keys_dir)?;
    sync_tree_bottom_up(staging_data_root)?;
    validate_tree(staging_data_root, &expected)?;
    Ok(())
}

fn normalize_erased_ids(erased_ids: &[String]) -> Result<Vec<String>, BundleError> {
    let mut unique = BTreeSet::new();
    for id in erased_ids {
        if id.is_empty() || !valid_name(id) || id.contains('.') {
            return Err(BundleError::InvalidManifest(format!(
                "invalid terminal counterparty id: {id}"
            )));
        }
        unique.insert(id.clone());
    }
    Ok(unique.into_iter().collect())
}

fn expected_entries_after_delta(
    entries: &[BundleEntry],
    erased_ids: &[String],
) -> Result<Vec<BundleEntry>, BundleError> {
    let mut remove = BTreeSet::new();
    let mut add = BTreeSet::new();
    for id in erased_ids {
        remove.insert(format!("data/keys/{id}"));
        remove.insert(format!("data/keys/{id}.migpending"));
        add.insert(format!("data/keys/{id}.erased"));
    }

    let empty_digest = hex(&Sha256::digest([]));
    let mut expected = Vec::with_capacity(entries.len() + add.len());
    let mut has_keys_dir = false;
    for entry in entries {
        let path = entry.path();
        if path == "data/keys" {
            has_keys_dir = true;
        }
        if remove.contains(path) {
            continue;
        }
        if add.contains(path) {
            // Replace any pre-existing non-tombstone entry for this path with
            // the authorized empty regular-file tombstone definition.
            expected.push(BundleEntry::RegularFile {
                path: path.to_owned(),
                byte_length: 0,
                sha256: empty_digest.clone(),
            });
            add.remove(path);
            continue;
        }
        expected.push(entry.clone());
    }
    if !add.is_empty() && !has_keys_dir {
        expected.push(BundleEntry::Directory {
            path: "data/keys".into(),
        });
    }
    for path in add {
        expected.push(BundleEntry::RegularFile {
            path,
            byte_length: 0,
            sha256: empty_digest.clone(),
        });
    }
    expected.sort_by(|left, right| left.path().cmp(right.path()));

    // Ensure every added tombstone path is a normal data path under keys/.
    for entry in &expected {
        if let BundleEntry::RegularFile { path, .. } = entry {
            if path.ends_with(".erased") {
                let _ = relative_data_path(path)?;
            }
        }
    }
    // Keep decode helper used so empty-digest stays validated as hex.
    let _ = decode_hex_32(&empty_digest)
        .ok_or_else(|| BundleError::InvalidManifest("invalid empty digest".into()))?;
    Ok(expected)
}

fn remove_if_present(path: &Path) -> Result<(), BundleError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_err("remove staged key path", path, source)),
    }
}

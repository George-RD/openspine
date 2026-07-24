//! Path helpers and crash-recoverable data-root rename install for restore.
//!
//! Every rename/remove is followed by a parent-directory fsync. Continuity is
//! written via temp → file sync → rename → control-dir fsync.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

use super::types::OverlayOperationError;

const CONTINUITY_FILE: &str = "portable-continuity.json";
const CONTINUITY_TEMP: &str = ".portable-continuity.tmp";
const STAGED_SUFFIX: &str = ".openspine-restore-new";
const PREVIOUS_SUFFIX: &str = ".openspine-restore-old";
const CLEANUP_SUFFIX: &str = ".openspine-restore-cleanup";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum InstallState {
    Clean,
    StagedOnly,
    Swapped,
    PreviousOnly,
    CleanupCommitted,
    Ambiguous,
}

pub(super) fn continuity_path(control_root: &Path) -> PathBuf {
    control_root.join(CONTINUITY_FILE)
}

pub(super) fn staged_path(data_root: &Path, request_id: &str) -> PathBuf {
    sibling(data_root, request_id, STAGED_SUFFIX)
}

pub(super) fn previous_path(data_root: &Path, request_id: &str) -> PathBuf {
    sibling(data_root, request_id, PREVIOUS_SUFFIX)
}

pub(super) fn cleanup_path(data_root: &Path, request_id: &str) -> PathBuf {
    sibling(data_root, request_id, CLEANUP_SUFFIX)
}

pub(super) fn inspect_install(
    data_root: &Path,
    request_id: &str,
) -> Result<InstallState, OverlayOperationError> {
    let staged = exists(&staged_path(data_root, request_id))?;
    let previous = exists(&previous_path(data_root, request_id))?;
    let cleanup = exists(&cleanup_path(data_root, request_id))?;
    let live = exists(data_root)?;
    Ok(match (staged, previous, cleanup, live) {
        (false, false, false, _) => InstallState::Clean,
        (true, false, false, true) => InstallState::StagedOnly,
        (false, true, false, true) => InstallState::Swapped,
        (true, true, false, false) => InstallState::PreviousOnly,
        (false, false, true, true) => InstallState::CleanupCommitted,
        _ => InstallState::Ambiguous,
    })
}

/// Install a verified staged tree into `data_root`, recovering from rename
/// failpoints by request-id sibling existence only (pathless recovery).
pub(super) fn install_or_recover(
    data_root: &Path,
    request_id: &str,
) -> Result<(), OverlayOperationError> {
    let staged = staged_path(data_root, request_id);
    let previous = previous_path(data_root, request_id);
    let parent = parent_dir(data_root)?;
    match inspect_install(data_root, request_id)? {
        InstallState::Clean => {
            if !exists(&staged)? {
                return Err(OverlayOperationError::MissingInstallState);
            }
            rename_and_sync(data_root, &previous, &parent)?;
            rename_and_sync(&staged, data_root, &parent)?;
            Ok(())
        }
        InstallState::StagedOnly => {
            if !exists(data_root)? {
                return Err(OverlayOperationError::MissingInstallState);
            }
            rename_and_sync(data_root, &previous, &parent)?;
            rename_and_sync(&staged, data_root, &parent)?;
            Ok(())
        }
        InstallState::PreviousOnly => {
            if exists(&staged)? {
                rename_and_sync(&staged, data_root, &parent)?;
            } else {
                return Err(OverlayOperationError::MissingInstallState);
            }
            Ok(())
        }
        InstallState::Swapped => Ok(()),
        InstallState::Ambiguous => Err(OverlayOperationError::AmbiguousInstallState),
        InstallState::CleanupCommitted => Err(OverlayOperationError::MissingInstallState),
    }
}

/// Roll back a swapped install using only request-id sibling existence.
pub(super) fn rollback_or_recover(
    data_root: &Path,
    request_id: &str,
) -> Result<(), OverlayOperationError> {
    let staged = staged_path(data_root, request_id);
    let previous = previous_path(data_root, request_id);
    let parent = parent_dir(data_root)?;
    match inspect_install(data_root, request_id)? {
        InstallState::Clean => Err(OverlayOperationError::MissingInstallState),
        InstallState::Swapped => {
            if exists(data_root)? {
                rename_and_sync(data_root, &staged, &parent)?;
            }
            if exists(&previous)? {
                rename_and_sync(&previous, data_root, &parent)?;
            } else {
                return Err(OverlayOperationError::MissingInstallState);
            }
            Ok(())
        }
        InstallState::PreviousOnly => {
            if exists(&previous)? {
                rename_and_sync(&previous, data_root, &parent)?;
            } else {
                return Err(OverlayOperationError::MissingInstallState);
            }
            Ok(())
        }
        InstallState::StagedOnly => Ok(()),
        InstallState::CleanupCommitted => Err(OverlayOperationError::MissingInstallState),
        InstallState::Ambiguous => Err(OverlayOperationError::AmbiguousInstallState),
    }
}

pub(super) fn cleanup_install(
    data_root: &Path,
    request_id: &str,
) -> Result<(), OverlayOperationError> {
    let staged = staged_path(data_root, request_id);
    let previous = previous_path(data_root, request_id);
    let cleanup = cleanup_path(data_root, request_id);
    let parent = parent_dir(data_root)?;
    match inspect_install(data_root, request_id)? {
        InstallState::Clean => Ok(()),
        InstallState::StagedOnly => remove_tree_and_sync(&staged, &parent),
        InstallState::Swapped => {
            rename_and_sync(&previous, &cleanup, &parent)?;
            remove_tree_and_sync(&cleanup, &parent)
        }
        InstallState::CleanupCommitted => remove_tree_and_sync(&cleanup, &parent),
        InstallState::PreviousOnly | InstallState::Ambiguous => {
            Err(OverlayOperationError::AmbiguousInstallState)
        }
    }
}

/// Atomic continuity write: temp → fsync file → rename → fsync control dir.
pub(super) fn write_continuity(
    control_root: &Path,
    bytes: &[u8],
) -> Result<(), OverlayOperationError> {
    let temp = control_root.join(CONTINUITY_TEMP);
    let final_path = continuity_path(control_root);
    let _ = fs::remove_file(&temp);
    let result = (|| {
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        opts.mode(0o600);
        let mut file = opts.open(&temp).map_err(|source| io(&temp, source))?;
        file.write_all(bytes).map_err(|source| io(&temp, source))?;
        file.sync_all().map_err(|source| io(&temp, source))?;
        drop(file);
        fs::rename(&temp, &final_path).map_err(|source| io(&final_path, source))?;
        #[cfg(unix)]
        {
            fs::set_permissions(&final_path, fs::Permissions::from_mode(0o600))
                .map_err(|source| io(&final_path, source))?;
        }
        sync_dir(control_root)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub(super) fn load_continuity(
    control_root: &Path,
) -> Result<Option<Vec<u8>>, OverlayOperationError> {
    let path = continuity_path(control_root);
    match fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(io(&path, source)),
    }
}

pub(super) fn remove_continuity(control_root: &Path) -> Result<(), OverlayOperationError> {
    let path = continuity_path(control_root);
    match fs::remove_file(&path) {
        Ok(()) => sync_dir(control_root),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io(&path, source)),
    }
}

fn sibling(data_root: &Path, request_id: &str, suffix: &str) -> PathBuf {
    let parent = data_root
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let name = data_root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "data".to_owned());
    parent.join(format!("{name}.{request_id}{suffix}"))
}

fn parent_dir(path: &Path) -> Result<PathBuf, OverlayOperationError> {
    path.parent()
        .map(Path::to_path_buf)
        .ok_or(OverlayOperationError::MissingInstallState)
}

fn exists(path: &Path) -> Result<bool, OverlayOperationError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(io(path, source)),
    }
}

fn rename_and_sync(from: &Path, to: &Path, parent: &Path) -> Result<(), OverlayOperationError> {
    fs::rename(from, to).map_err(|source| io(from, source))?;
    sync_dir(parent)
}

fn remove_tree_and_sync(path: &Path, parent: &Path) -> Result<(), OverlayOperationError> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|source| io(path, source))?;
    sync_dir(parent)
}

fn sync_dir(path: &Path) -> Result<(), OverlayOperationError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|source| io(path, source))
}

fn io(path: &Path, source: std::io::Error) -> OverlayOperationError {
    OverlayOperationError::Io {
        path: path.to_path_buf(),
        source,
    }
}

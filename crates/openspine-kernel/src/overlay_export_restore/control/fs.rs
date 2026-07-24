//! Filesystem helpers for overlay control: atomic writes, directory creation,
//! permission hardening, temp cleanup, and sync.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use super::wire::io;
use super::ControlError;

/// Remove a temp file if it exists; no-op if absent.
pub(super) fn cleanup_temp(path: &Path) -> Result<(), ControlError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io(path, source)),
    }
}

/// Atomically write `bytes` to `root/temp_name`, then rename to
/// `root/final_name`. Cleans up the temp on failure.
pub(super) fn atomic_write(
    root: &Path,
    temp_name: &str,
    final_name: &str,
    bytes: &[u8],
) -> Result<(), ControlError> {
    let temp = root.join(temp_name);
    let final_path = root.join(final_name);
    cleanup_temp(&temp)?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|source| io(&temp, source))?;
        file.write_all(bytes).map_err(|source| io(&temp, source))?;
        file.sync_all().map_err(|source| io(&temp, source))?;
        fs::rename(&temp, &final_path).map_err(|source| io(&final_path, source))?;
        sync_dir(root)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Remove a durable file and sync its parent directory.
pub(super) fn remove_durable(path: &Path, parent: &Path) -> Result<(), ControlError> {
    fs::remove_file(path).map_err(|source| io(path, source))?;
    sync_dir(parent)
}

/// Sync a directory by opening it and calling `sync_all`.
pub(super) fn sync_dir(path: &Path) -> Result<(), ControlError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|source| io(path, source))
}

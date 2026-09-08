//! Descriptor-relative storage primitives. Never follow destination links.
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use super::install_types::InstallError as Error;

pub(super) fn root(path: &Path) -> Result<File, Error> {
    let file = OpenOptions::new().read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path).map_err(|_| Error::Destination)?;
    check_directory(&file)?;
    Ok(file)
}

pub(super) fn open_directory(parent: &File, name: &str) -> Result<File, Error> {
    let file = super::source::open_at(parent, name, true).map_err(|_| Error::Destination)?;
    check_directory(&file)?;
    Ok(file)
}

pub(super) fn ensure_directory(parent: &File, name: &str) -> Result<File, Error> {
    let c_name = name_c(name)?;
    // SAFETY: parent is open and the validated child name is NUL terminated.
    let result = unsafe { libc::mkdirat(parent.as_raw_fd(), c_name.as_ptr(), 0o700) };
    if result != 0 && io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
        return Err(Error::Destination);
    }
    let child = open_directory(parent, name)?;
    sync(&child)?;
    sync(parent)?;
    Ok(child)
}

pub(super) fn create_directory(parent: &File, name: &str) -> Result<File, Error> {
    let c_name = name_c(name)?;
    // SAFETY: same as ensure_directory; EEXIST is deliberately not accepted.
    if unsafe { libc::mkdirat(parent.as_raw_fd(), c_name.as_ptr(), 0o700) } != 0 {
        return Err(Error::Publication);
    }
    let child = open_directory(parent, name)?;
    sync(parent)?;
    Ok(child)
}

pub(super) fn write_new(parent: &File, name: &str, bytes: &[u8]) -> Result<(), Error> {
    let name = name_c(name)?;
    // SAFETY: the live parent descriptor and single-component name are valid;
    // O_EXCL refuses any existing file, hard link or symlink.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(),
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC, 0o600) };
    if fd < 0 { return Err(Error::Publication); }
    // SAFETY: openat returned a unique owned descriptor.
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes).map_err(|_| Error::Publication)?;
    sync(&file)
}

pub(super) fn sync(file: &File) -> Result<(), Error> {
    file.sync_all().map_err(|_| Error::Publication)
}

pub(super) fn same(left: &File, right: &File) -> Result<(), Error> {
    let left = left.metadata().map_err(|_| Error::Destination)?;
    let right = right.metadata().map_err(|_| Error::Destination)?;
    if (left.dev(), left.ino()) != (right.dev(), right.ino()) { return Err(Error::Destination); }
    Ok(())
}

fn check_directory(file: &File) -> Result<(), Error> {
    let meta = file.metadata().map_err(|_| Error::Destination)?;
    // SAFETY: geteuid has no preconditions and returns the calling process UID.
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o022 != 0 {
        return Err(Error::Destination);
    }
    Ok(())
}

fn name_c(name: &str) -> Result<CString, Error> {
    if name.is_empty() || matches!(name, "." | "..") || name.contains('/') {
        return Err(Error::Destination);
    }
    CString::new(name).map_err(|_| Error::Destination)
}

/// True means published, false means a destination already exists. Never
/// emulate no-replace with a racy exists() + overwriting rename().
pub(super) fn publish(parent: &File, name: &str, objects: &File, digest: &str) -> Result<bool, Error> {
    let name = name_c(name)?;
    let digest = name_c(digest)?;
    #[cfg(target_os = "linux")]
    // SAFETY: both directory descriptors are live and both names are valid.
    let result = unsafe { libc::renameat2(parent.as_raw_fd(), name.as_ptr(), objects.as_raw_fd(), digest.as_ptr(), libc::RENAME_NOREPLACE) };
    #[cfg(target_os = "macos")]
    // SAFETY: as above. RENAME_EXCL is Darwin's atomic no-replace operation.
    let result = unsafe { libc::renameatx_np(parent.as_raw_fd(), name.as_ptr(), objects.as_raw_fd(), digest.as_ptr(), libc::RENAME_EXCL) };
    if result != 0 {
        return if io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
            Ok(false)
        } else { Err(Error::Publication) };
    }
    super::install_types::crash_at("after-rename-before-sync");
    sync(objects)?;
    sync(parent)?;
    Ok(true)
}

pub(super) fn remove_stage(parent: &File, name: &str) -> Result<(), Error> {
    let c_name = name_c(name)?;
    // Open with no-follow. ENOENT is the post-publish/already-cleaned case;
    // every other error is refused, including symlinks and unexpected files.
    // SAFETY: descriptor and name valid; no creation or write access requested.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), c_name.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
    if fd < 0 {
        return if io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
            sync(parent)
        } else { Err(Error::Destination) };
    }
    // SAFETY: the successful openat descriptor is owned here.
    let directory = unsafe { File::from_raw_fd(fd) };
    check_directory(&directory)?;
    let mut remaining = super::MAX_FILES + super::FAMILIES.len() + 1;
    for entry in super::source::names(&directory, &mut remaining).map_err(|_| Error::Destination)? {
        if super::FAMILIES.contains(&entry.as_str()) || entry == "docs" {
            let child = open_directory(&directory, &entry)?;
            for file in super::source::names(&child, &mut remaining).map_err(|_| Error::Destination)? {
                unlink(&child, &file, false)?;
            }
            unlink(&directory, &entry, true)?;
        } else {
            unlink(&directory, &entry, false)?;
        }
    }
    unlink(parent, name, true)?;
    sync(parent)
}

fn unlink(parent: &File, name: &str, directory: bool) -> Result<(), Error> {
    let name = name_c(name)?;
    // SAFETY: live descriptor, valid child name. unlinkat never follows the
    // final symlink, and AT_REMOVEDIR refuses links in place of directories.
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), if directory { libc::AT_REMOVEDIR } else { 0 }) } != 0 {
        return Err(Error::Publication);
    }
    Ok(())
}

/// Resolve an existing ancestor before appending absent components. This
/// catches aliases to an active tree even when its leaf does not exist yet.
pub(super) fn resolve(path: &Path) -> Result<PathBuf, Error> {
    match std::fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let name = path.file_name().ok_or(Error::Destination)?;
            let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
            Ok(resolve(parent)?.join(name))
        }
        Err(_) => Err(Error::Destination),
    }
}

/// Re-durabilize a verified orphan, including its files, before indexing it.
/// Paths come only from the retained validated snapshot; the candidate is not read.
pub(super) fn sync_snapshot(root: &File, snapshot: &super::PackageSnapshot) -> Result<(), Error> {
    let mut directories = std::collections::BTreeMap::new();
    for (path, _) in snapshot.files() {
        let (parent, file) = match path.split_once('/') {
            Some((family, file)) => {
                if !directories.contains_key(family) {
                    directories.insert(family, open_directory(root, family)?);
                }
                (&directories[family], file)
            }
            None => (root, path),
        };
        let file = super::source::open_at(parent, file, false).map_err(|_| Error::ObjectCorrupt)?;
        sync(&file)?;
    }
    for directory in directories.values() { sync(directory)?; }
    sync(root)
}

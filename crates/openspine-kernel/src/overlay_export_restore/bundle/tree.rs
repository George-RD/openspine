//! Typed-tree enumeration, copy-while-hash, and exact validation for bundles.

use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
use std::os::fd::AsRawFd as _;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt as _;
#[cfg(unix)]
use std::os::unix::io::RawFd;

use super::durable::{
    create_dir, io_err, mutation_or_io, path_exists_nofollow, require_dir, same_state, set_mode,
    sync_dir, valid_name,
};
use super::manifest::{decode_hex_32, hex, validate_manifest_path};
use super::{BundleEntry, BundleError, DATA_DIR};

#[cfg(unix)]
use super::unix_fd::{
    is_eloop, open_dir_nofollow, open_parent_fd, open_path_dir_fd, openat_dir, openat_file,
    read_names_fd,
};

pub(super) fn validate_entries(entries: &[BundleEntry]) -> Result<(), BundleError> {
    if entries.first().map(BundleEntry::path) != Some(DATA_DIR) {
        return Err(BundleError::InvalidManifest(
            "typed tree must start with data directory".into(),
        ));
    }
    let mut prior: Option<&str> = None;
    let directories: BTreeSet<&str> = entries
        .iter()
        .filter_map(|entry| match entry {
            BundleEntry::Directory { path } => Some(path.as_str()),
            _ => None,
        })
        .collect();
    for entry in entries {
        let path = entry.path();
        validate_manifest_path(path)?;
        if prior.is_some_and(|previous| previous >= path) {
            return Err(BundleError::InvalidManifest(
                "entries are not sorted and unique".into(),
            ));
        }
        prior = Some(path);
        if path != DATA_DIR {
            let parent = path
                .rsplit_once('/')
                .map(|(parent, _)| parent)
                .unwrap_or(DATA_DIR);
            if !directories.contains(parent) {
                return Err(BundleError::InvalidManifest(format!(
                    "undeclared parent for {path}"
                )));
            }
        }
        match entry {
            BundleEntry::Directory { path }
                if path
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.ends_with(".erased")) =>
            {
                return Err(BundleError::InvalidManifest(
                    "tombstone path is a directory".into(),
                ));
            }
            BundleEntry::RegularFile { path, sha256, .. } if decode_hex_32(sha256).is_none() => {
                return Err(BundleError::InvalidManifest(format!(
                    "invalid digest for {path}"
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn enumerate_data_root(root: &Path) -> Result<Vec<BundleEntry>, BundleError> {
    require_dir(root)?;
    let mut entries = vec![BundleEntry::Directory {
        path: DATA_DIR.into(),
    }];
    #[cfg(unix)]
    {
        let directory = open_dir_nofollow(root)?;
        enum_dir_fd(directory.as_raw_fd(), root, DATA_DIR, &mut entries)?;
    }
    #[cfg(not(unix))]
    {
        return Err(BundleError::TreeMismatch(
            "requires Unix no-follow traversal".into(),
        ));
    }
    entries.sort_by(|left, right| left.path().cmp(right.path()));
    Ok(entries)
}

#[cfg(unix)]
fn enum_dir_fd(
    dir_fd: RawFd,
    display: &Path,
    prefix: &str,
    entries: &mut Vec<BundleEntry>,
) -> Result<(), BundleError> {
    let mut names = read_names_fd(dir_fd, display)?;
    names.sort();
    for name in names {
        if !valid_name(&name) {
            return Err(BundleError::TreeMismatch(format!(
                "non-normal filesystem name: {name}"
            )));
        }
        let child = display.join(&name);
        let manifest_path = format!("{prefix}/{name}");
        match openat_dir(dir_fd, &name) {
            Ok(child_dir) => {
                if name.ends_with(".erased") {
                    return Err(BundleError::TreeMismatch(format!(
                        "tombstone is not a regular file: {manifest_path}"
                    )));
                }
                entries.push(BundleEntry::Directory {
                    path: manifest_path.clone(),
                });
                enum_dir_fd(child_dir.as_raw_fd(), &child, &manifest_path, entries)?;
            }
            Err(dir_error) => match openat_file(dir_fd, &name) {
                Ok(mut file) => {
                    let before = file
                        .metadata()
                        .map_err(|source| io_err("inspect open file", &child, source))?;
                    if !before.is_file() {
                        return Err(BundleError::TreeMismatch(format!(
                            "special entry: {}",
                            child.display()
                        )));
                    }
                    let (length, digest) = hash_open(&mut file, &child, &before)?;
                    entries.push(BundleEntry::RegularFile {
                        path: manifest_path,
                        byte_length: length,
                        sha256: digest,
                    });
                }
                Err(file_error) => {
                    if is_eloop(&dir_error) || is_eloop(&file_error) {
                        return Err(BundleError::TreeMismatch(format!(
                            "symlink entry: {}",
                            child.display()
                        )));
                    }
                    return Err(BundleError::TreeMismatch(format!(
                        "symlink or special entry: {}",
                        child.display()
                    )));
                }
            },
        }
    }
    Ok(())
}

pub(super) fn validate_tree(root: &Path, expected: &[BundleEntry]) -> Result<(), BundleError> {
    let actual = enumerate_data_root(root)?;
    if actual != expected {
        return Err(BundleError::TreeMismatch(
            "actual typed tree differs from manifest".into(),
        ));
    }
    Ok(())
}

pub(super) fn copy_entries(
    source_root: &Path,
    destination_root: &Path,
    entries: &[BundleEntry],
) -> Result<(), BundleError> {
    #[cfg(unix)]
    {
        let source_root_fd = open_dir_nofollow(source_root)?;
        for entry in entries {
            let relative = relative_data_path(entry.path())?;
            let destination = if relative.as_os_str().is_empty() {
                destination_root.to_path_buf()
            } else {
                destination_root.join(&relative)
            };
            match entry {
                BundleEntry::Directory { path } => {
                    let _source = open_path_dir_fd(source_root_fd.as_raw_fd(), source_root, path)?;
                    if relative.as_os_str().is_empty() {
                        if !path_exists_nofollow(&destination)? {
                            create_dir(&destination)?;
                        }
                    } else {
                        create_dir(&destination)?;
                    }
                }
                BundleEntry::RegularFile {
                    path,
                    byte_length,
                    sha256,
                } => {
                    let (parent_fd, name, display) =
                        open_parent_fd(source_root_fd.as_raw_fd(), source_root, path)?;
                    let mut source = openat_file(parent_fd.as_raw_fd(), &name)
                        .map_err(|error| mutation_or_io(&display, "openat source file", error))?;
                    let before = source.metadata().map_err(|source_error| {
                        io_err("inspect open source file", &display, source_error)
                    })?;
                    if !before.is_file() {
                        return Err(BundleError::TreeMismatch(format!(
                            "{} is not a regular file",
                            display.display()
                        )));
                    }
                    copy_from_open(
                        &mut source,
                        &display,
                        &before,
                        &destination,
                        *byte_length,
                        sha256,
                    )?;
                }
            }
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (source_root, destination_root, entries);
        Err(BundleError::TreeMismatch("requires Unix".into()))
    }
}

pub(super) fn relative_data_path(manifest_path: &str) -> Result<PathBuf, BundleError> {
    validate_manifest_path(manifest_path)?;
    if manifest_path == DATA_DIR {
        return Ok(PathBuf::new());
    }
    let stripped = manifest_path
        .strip_prefix("data/")
        .ok_or_else(|| BundleError::InvalidManifest(format!("non-data path: {manifest_path}")))?;
    let mut out = PathBuf::new();
    for component in Path::new(stripped).components() {
        match component {
            Component::Normal(part) => out.push(part),
            _ => {
                return Err(BundleError::InvalidManifest(format!(
                    "non-normal path: {manifest_path}"
                )));
            }
        }
    }
    Ok(out)
}

pub(super) fn copy_from_open(
    source: &mut File,
    source_path: &Path,
    before: &fs::Metadata,
    destination_path: &Path,
    expected_len: u64,
    expected_digest: &str,
) -> Result<(), BundleError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut destination = options
        .open(destination_path)
        .map_err(|source| io_err("create destination file", destination_path, source))?;
    let mut hasher = Sha256::new();
    let mut length = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = source
            .read(&mut buffer)
            .map_err(|error| io_err("read source file", source_path, error))?;
        if count == 0 {
            break;
        }
        destination
            .write_all(&buffer[..count])
            .map_err(|error| io_err("write destination file", destination_path, error))?;
        hasher.update(&buffer[..count]);
        length += count as u64;
    }
    destination
        .sync_all()
        .map_err(|source| io_err("sync destination file", destination_path, source))?;
    set_mode(destination_path, 0o600)?;
    let after = source
        .metadata()
        .map_err(|error| io_err("reinspect source file", source_path, error))?;
    let digest = hex(&hasher.finalize());
    if !same_state(before, &after) || length != expected_len || digest != expected_digest {
        return Err(BundleError::ConcurrentMutation(source_path.to_path_buf()));
    }
    Ok(())
}

pub(super) fn hash_open(
    file: &mut File,
    path: &Path,
    before: &fs::Metadata,
) -> Result<(u64, String), BundleError> {
    let mut hasher = Sha256::new();
    let mut length = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|source| io_err("hash file", path, source))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        length += count as u64;
    }
    let after = file
        .metadata()
        .map_err(|source| io_err("reinspect file", path, source))?;
    if !same_state(before, &after) || length != before.len() {
        return Err(BundleError::ConcurrentMutation(path.to_path_buf()));
    }
    Ok((length, hex(&hasher.finalize())))
}

pub(super) fn sync_tree_bottom_up(root: &Path) -> Result<(), BundleError> {
    let mut directories: Vec<PathBuf> = enumerate_data_root(root)?
        .into_iter()
        .filter_map(|entry| match entry {
            BundleEntry::Directory { path } => Some(path),
            _ => None,
        })
        .map(|path| {
            let relative = path.strip_prefix("data").unwrap().trim_start_matches('/');
            if relative.is_empty() {
                root.to_path_buf()
            } else {
                root.join(relative)
            }
        })
        .collect();
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        sync_dir(&directory)?;
    }
    Ok(())
}

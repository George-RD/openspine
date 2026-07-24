//! Internal descriptor-relative open and lock helpers for overlay control.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::RawFd;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::error::ControlError;
use super::wire::io;

fn is_allowed_platform_redirect(source: &Path, target: &Path) -> bool {
    // Documented OS prefix redirects only. Same-basename absolute user
    // aliases (e.g. root/foo -> /attacker/foo) are NOT allowed.
    // macOS stores these as relative links under / (private/var, private/tmp).
    matches!(
        (source.to_str(), target.to_str()),
        (Some("/var"), Some("/private/var"))
            | (Some("/var"), Some("private/var"))
            | (Some("/tmp"), Some("/private/tmp"))
            | (Some("/tmp"), Some("private/tmp"))
            | (Some("/etc"), Some("/private/etc"))
            | (Some("/etc"), Some("private/etc"))
    )
}
const O_RDONLY: i32 = 0;
const O_RDWR: i32 = 2;

fn o_creat() -> i32 {
    #[cfg(target_os = "linux")]
    {
        64
    }
    #[cfg(target_os = "macos")]
    {
        0x0200
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

fn o_nofollow() -> i32 {
    #[cfg(target_os = "linux")]
    {
        0o400000
    }
    #[cfg(target_os = "macos")]
    {
        0x0100
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

fn o_directory() -> i32 {
    #[cfg(target_os = "linux")]
    {
        0o200000
    }
    #[cfg(target_os = "macos")]
    {
        0x0010_0000
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

fn o_cloexec() -> i32 {
    #[cfg(target_os = "linux")]
    {
        0o2000000
    }
    #[cfg(target_os = "macos")]
    {
        0x0100_0000
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

unsafe extern "C" {
    fn openat(dirfd: i32, pathname: *const std::os::raw::c_char, flags: i32, ...) -> i32;
    fn flock(fd: i32, operation: i32) -> i32;
    fn unlinkat(dirfd: i32, pathname: *const std::os::raw::c_char, flags: i32) -> i32;
    fn fchmod(fd: i32, mode: u32) -> i32;
    fn mkdirat(dirfd: i32, pathname: *const std::os::raw::c_char, mode: u32) -> i32;
}

const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;

pub(super) struct ResolvedRootIdentity {
    pub(super) canonical_parent: PathBuf,
    pub(super) canonical_data_root: PathBuf,
    pub(super) control_root_name: String,
    pub(super) control_root: PathBuf,
}

pub(super) fn resolve_root_identity(
    data_root: &Path,
) -> Result<ResolvedRootIdentity, ControlError> {
    if data_root.as_os_str().is_empty() {
        return Err(ControlError::NotDirectory(data_root.to_path_buf()));
    }

    if data_root
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(ControlError::SymlinkDataRoot(data_root.to_path_buf()));
    }

    let anchored = if data_root.is_absolute() {
        data_root.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| io(data_root, source))?
            .join(data_root)
    };
    let mut normalized_data_root = PathBuf::new();
    for component in anchored.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                return Err(ControlError::SymlinkDataRoot(data_root.to_path_buf()));
            }
            _ => normalized_data_root.push(component.as_os_str()),
        }
    }

    let parent = normalized_data_root
        .parent()
        .ok_or_else(|| ControlError::NotDirectory(data_root.to_path_buf()))?;
    // Reject any symlink in the parent path except documented platform prefix
    // redirects (macOS /var -> /private/var and /tmp -> /private/tmp). User-controlled
    // absolute or relative aliases are rejected even when basenames match.
    {
        let mut cur = PathBuf::new();
        for component in parent.components() {
            cur.push(component);
            if cur.as_os_str().is_empty() || cur == Path::new("/") {
                continue;
            }
            match std::fs::symlink_metadata(&cur) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    let target =
                        std::fs::read_link(&cur).map_err(|source| io(data_root, source))?;
                    if !is_allowed_platform_redirect(&cur, &target) {
                        return Err(ControlError::SymlinkDataRoot(data_root.to_path_buf()));
                    }
                    // Continue walking from the redirect target so subsequent
                    // components are checked under the real prefix.
                    cur = if target.is_absolute() {
                        target
                    } else {
                        cur.parent().unwrap_or(Path::new("/")).join(target)
                    };
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => return Err(io(data_root, source)),
            }
        }
    }
    let canonical_parent = std::fs::canonicalize(parent).map_err(|source| io(data_root, source))?;

    let file_name = normalized_data_root
        .file_name()
        .ok_or_else(|| ControlError::NotDirectory(data_root.to_path_buf()))?;

    let lexical_data_root = canonical_parent.join(file_name);

    match std::fs::symlink_metadata(&lexical_data_root) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(ControlError::SymlinkDataRoot(data_root.to_path_buf()));
        }
        Ok(meta) if !meta.is_dir() => {
            return Err(ControlError::NotDirectory(data_root.to_path_buf()));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => return Err(io(data_root, source)),
    }

    let canonical_data_root = if lexical_data_root.exists() {
        let canonical =
            std::fs::canonicalize(&lexical_data_root).map_err(|source| io(data_root, source))?;
        if canonical != lexical_data_root {
            return Err(ControlError::SymlinkDataRoot(data_root.to_path_buf()));
        }
        canonical
    } else {
        lexical_data_root
    };

    let digest = Sha256::digest(canonical_data_root.as_os_str().as_bytes());
    let control_root_name = format!(".openspine-control-{}", super::wire::hex(&digest));
    let control_root = canonical_parent.join(&control_root_name);

    Ok(ResolvedRootIdentity {
        canonical_parent,
        canonical_data_root,
        control_root_name,
        control_root,
    })
}

pub(super) fn open_canonical_dir_nofollow(path: &Path) -> Result<File, ControlError> {
    use std::ffi::CString;
    use std::path::Component;

    let mut current = {
        let mut options = OpenOptions::new();
        options.read(true);
        options.custom_flags(o_nofollow() | o_directory() | o_cloexec());
        options
            .open(Path::new("/"))
            .map_err(|source| io(path, source))?
    };
    for component in path.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let c_name = CString::new(name.as_bytes()).map_err(|_| {
            io(
                path,
                io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"),
            )
        })?;
        let fd = unsafe {
            openat(
                current.as_raw_fd(),
                c_name.as_ptr(),
                o_nofollow() | o_directory() | o_cloexec() | O_RDONLY,
            )
        };
        if fd < 0 {
            return Err(io(path, io::Error::last_os_error()));
        }
        current = unsafe { File::from_raw_fd(fd) };
    }
    Ok(current)
}

pub(super) fn open_or_create_control_dir_relative(
    parent_fd: RawFd,
    name: &str,
    path: &Path,
) -> Result<File, ControlError> {
    use std::ffi::CString;

    let c_name = CString::new(name).map_err(|_| {
        io(
            path,
            io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"),
        )
    })?;
    let flags = o_nofollow() | o_directory() | o_cloexec() | O_RDONLY;
    let mut fd = unsafe { openat(parent_fd, c_name.as_ptr(), flags) };
    if fd < 0 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::NotFound {
            let mk_res = unsafe { mkdirat(parent_fd, c_name.as_ptr(), 0o700) };
            if mk_res != 0 {
                return Err(io(path, io::Error::last_os_error()));
            }
            fd = unsafe { openat(parent_fd, c_name.as_ptr(), flags) };
            if fd < 0 {
                return Err(io(path, io::Error::last_os_error()));
            }
        } else {
            return Err(io(path, err));
        }
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let meta = file.metadata().map_err(|e| io(path, e))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(ControlError::UnsafeControlPath(path.to_path_buf()));
    }
    secure_dir_fd(&file, path)?;
    Ok(file)
}

pub(super) fn openat_file_nofollow(
    dir_fd: RawFd,
    name: &str,
    create: bool,
) -> Result<File, io::Error> {
    use std::ffi::CString;

    let c_name = CString::new(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"))?;
    let mut flags = o_nofollow() | o_cloexec() | O_RDWR;
    if create {
        flags |= o_creat();
    }
    let fd = unsafe { openat(dir_fd, c_name.as_ptr(), flags, 0o600) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let meta = file.metadata()?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe non-regular file",
        ));
    }
    Ok(file)
}

pub(super) fn try_lock_exclusive(file: &File, path: &Path) -> Result<(), ControlError> {
    let fd = file.as_raw_fd();
    let res = unsafe { flock(fd, LOCK_EX | LOCK_NB) };
    if res == 0 {
        Ok(())
    } else {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::WouldBlock
            || err.raw_os_error() == Some(35) // EWOULDBLOCK on macOS/Linux
            || err.raw_os_error() == Some(11)
        // EAGAIN on Linux
        {
            Err(ControlError::AlreadyLocked(path.to_path_buf()))
        } else {
            Err(io(path, err))
        }
    }
}

pub(super) fn verify_entry_is_nofollow_dir(
    dir_fd: RawFd,
    name: &str,
) -> Result<OwnedFd, ControlError> {
    use std::ffi::CString;

    let path = Path::new(name);
    let c_name = CString::new(name).map_err(|_| {
        io(
            path,
            io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"),
        )
    })?;
    let flags = o_nofollow() | o_directory() | o_cloexec() | O_RDONLY;
    let fd = unsafe { openat(dir_fd, c_name.as_ptr(), flags) };
    if fd < 0 {
        let err = io::Error::last_os_error();
        return Err(io(path, err));
    }
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    let file = File::from(owned.try_clone().map_err(|e| io(path, e))?);
    let meta = file.metadata().map_err(|e| io(path, e))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(ControlError::NotDirectory(PathBuf::from(name)));
    }
    Ok(owned)
}

pub(super) fn secure_dir_fd(dir_file: &File, path: &Path) -> Result<(), ControlError> {
    let res = unsafe { fchmod(dir_file.as_raw_fd(), 0o700) };
    if res == 0 {
        Ok(())
    } else {
        Err(io(path, io::Error::last_os_error()))
    }
}

pub(super) fn secure_or_create_sub_dir_at(
    dir_fd: RawFd,
    name: &str,
    path: &Path,
) -> Result<File, ControlError> {
    use std::ffi::CString;

    let c_name = CString::new(name).map_err(|_| {
        io(
            path,
            io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"),
        )
    })?;
    let flags = o_nofollow() | o_directory() | o_cloexec() | O_RDONLY;
    let mut fd = unsafe { openat(dir_fd, c_name.as_ptr(), flags) };
    if fd < 0 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::NotFound {
            let mk_res = unsafe { mkdirat(dir_fd, c_name.as_ptr(), 0o700) };
            if mk_res != 0 {
                return Err(io(path, io::Error::last_os_error()));
            }
            fd = unsafe { openat(dir_fd, c_name.as_ptr(), flags) };
            if fd < 0 {
                return Err(io(path, io::Error::last_os_error()));
            }
        } else {
            return Err(io(path, err));
        }
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let meta = file.metadata().map_err(|e| io(path, e))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(ControlError::UnsafeControlPath(path.to_path_buf()));
    }
    secure_dir_fd(&file, path)?;
    Ok(file)
}

pub(super) fn cleanup_temp_at(dir_fd: RawFd, name: &str, path: &Path) -> Result<(), ControlError> {
    use std::ffi::CString;

    let c_name = CString::new(name).map_err(|_| {
        io(
            path,
            io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"),
        )
    })?;
    let res = unsafe { unlinkat(dir_fd, c_name.as_ptr(), 0) };
    if res == 0 {
        Ok(())
    } else {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::NotFound {
            Ok(())
        } else {
            Err(io(path, err))
        }
    }
}

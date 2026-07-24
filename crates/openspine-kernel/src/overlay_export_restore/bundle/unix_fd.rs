//! Unix openat/no-follow directory traversal helpers for typed bundle trees.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};

use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
use std::os::unix::fs::OpenOptionsExt as _;
use std::os::unix::io::RawFd;

use super::durable::{io_err, mutation_or_io};
use super::tree::relative_data_path;
use super::{BundleError, DATA_DIR};

pub(super) fn open_dir_nofollow(path: &Path) -> Result<OwnedFd, BundleError> {
    let mut options = OpenOptions::new();
    options.read(true);
    options.custom_flags(o_nofollow() | o_directory());
    let file = options
        .open(path)
        .map_err(|source| io_err("open directory", path, source))?;
    if !file
        .metadata()
        .map_err(|source| io_err("inspect open directory", path, source))?
        .is_dir()
    {
        return Err(BundleError::TreeMismatch(format!(
            "{} is not a directory",
            path.display()
        )));
    }
    Ok(OwnedFd::from(file))
}

pub(super) fn open_path_dir_fd(
    root_fd: RawFd,
    root_display: &Path,
    manifest_path: &str,
) -> Result<OwnedFd, BundleError> {
    let mut current = dup_fd(root_fd, root_display)?;
    if manifest_path == DATA_DIR {
        return Ok(current);
    }
    let relative = relative_data_path(manifest_path)?;
    let mut display = root_display.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(BundleError::InvalidManifest(format!(
                "non-normal path: {manifest_path}"
            )));
        };
        let name = name
            .to_str()
            .ok_or_else(|| BundleError::TreeMismatch("non-UTF-8 path".into()))?;
        display.push(name);
        current = openat_dir(current.as_raw_fd(), name)
            .map_err(|error| mutation_or_io(&display, "openat directory", error))?;
    }
    Ok(current)
}

pub(super) fn open_parent_fd(
    root_fd: RawFd,
    root_display: &Path,
    manifest_path: &str,
) -> Result<(OwnedFd, String, PathBuf), BundleError> {
    let relative = relative_data_path(manifest_path)?;
    let name = relative
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| BundleError::InvalidManifest(format!("non-normal path: {manifest_path}")))?
        .to_owned();
    let parent_relative = relative.parent().unwrap_or_else(|| Path::new(""));
    let parent_manifest = if parent_relative.as_os_str().is_empty() {
        DATA_DIR.to_owned()
    } else {
        format!("data/{}", parent_relative.to_string_lossy())
    };
    let parent = open_path_dir_fd(root_fd, root_display, &parent_manifest)?;
    Ok((parent, name, root_display.join(relative)))
}

pub(super) fn openat_dir(dir_fd: RawFd, name: &str) -> Result<OwnedFd, io::Error> {
    use std::ffi::CString;

    let c_name =
        CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL"))?;
    let fd = unsafe {
        libc_openat(
            dir_fd,
            c_name.as_ptr(),
            o_nofollow() | o_directory() | o_cloexec(),
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

pub(super) fn openat_file(dir_fd: RawFd, name: &str) -> Result<File, io::Error> {
    use std::ffi::CString;

    let c_name =
        CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL"))?;
    let fd = unsafe {
        libc_openat(
            dir_fd,
            c_name.as_ptr(),
            o_nofollow() | o_nonblock() | o_cloexec(),
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata()?;
    if metadata.is_dir() {
        return Err(io::Error::new(io::ErrorKind::IsADirectory, "directory"));
    }
    if !metadata.is_file() {
        return Err(io::Error::other("special"));
    }
    Ok(file)
}

pub(super) fn read_names_fd(dir_fd: RawFd, path: &Path) -> Result<Vec<String>, BundleError> {
    // Dup so fdopendir takes ownership of a distinct descriptor.
    let owned = libc_dup_checked(dir_fd, path)?;
    let directory = unsafe { libc_fdopendir(owned) };
    if directory.is_null() {
        let error = io::Error::last_os_error();
        unsafe {
            libc_close(owned);
        }
        return Err(io_err("read directory", path, error));
    }
    let mut names = Vec::new();
    loop {
        // readdir: NULL + errno 0 means end; NULL + errno != 0 means error.
        unsafe {
            *libc_errno_location() = 0;
        }
        let entry = unsafe { libc_readdir(directory) };
        if entry.is_null() {
            let errno = unsafe { *libc_errno_location() };
            unsafe {
                libc_closedir(directory);
            }
            if errno != 0 {
                return Err(io_err(
                    "read directory entry",
                    path,
                    io::Error::from_raw_os_error(errno),
                ));
            }
            break;
        }
        let name = unsafe {
            let c_name = (*entry).d_name.as_ptr();
            std::ffi::CStr::from_ptr(c_name)
                .to_string_lossy()
                .into_owned()
        };
        if name != "." && name != ".." {
            names.push(name);
        }
    }
    Ok(names)
}

pub(super) fn is_eloop(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(40) | Some(62))
        || error.kind() == io::ErrorKind::TooManyLinks
}

#[repr(C)]
struct Dirent {
    // Portable-enough prefix: we only need d_name. Layout differs by OS; use
    // getdents via readdir which fills the platform dirent. We declare d_name at
    // the end after padding used by macOS/Linux.
    #[cfg(target_os = "macos")]
    d_ino: u64,
    #[cfg(target_os = "macos")]
    d_seekoff: u64,
    #[cfg(target_os = "macos")]
    d_reclen: u16,
    #[cfg(target_os = "macos")]
    d_namlen: u16,
    #[cfg(target_os = "macos")]
    d_type: u8,
    #[cfg(target_os = "macos")]
    d_name: [std::os::raw::c_char; 1024],
    #[cfg(target_os = "linux")]
    d_ino: u64,
    #[cfg(target_os = "linux")]
    d_off: i64,
    #[cfg(target_os = "linux")]
    d_reclen: u16,
    #[cfg(target_os = "linux")]
    d_type: u8,
    #[cfg(target_os = "linux")]
    d_name: [std::os::raw::c_char; 256],
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    d_name: [std::os::raw::c_char; 256],
}

type Dir = std::os::raw::c_void;

unsafe fn libc_fdopendir(fd: RawFd) -> *mut Dir {
    unsafe extern "C" {
        fn fdopendir(fd: RawFd) -> *mut Dir;
    }
    unsafe { fdopendir(fd) }
}

unsafe fn libc_readdir(dir: *mut Dir) -> *mut Dirent {
    unsafe extern "C" {
        fn readdir(dirp: *mut Dir) -> *mut Dirent;
    }
    unsafe { readdir(dir) }
}

unsafe fn libc_closedir(dir: *mut Dir) -> i32 {
    unsafe extern "C" {
        fn closedir(dirp: *mut Dir) -> i32;
    }
    unsafe { closedir(dir) }
}

unsafe fn libc_close(fd: RawFd) -> i32 {
    unsafe extern "C" {
        fn close(fd: RawFd) -> i32;
    }
    unsafe { close(fd) }
}

unsafe fn libc_errno_location() -> *mut i32 {
    #[cfg(target_os = "macos")]
    unsafe extern "C" {
        fn __error() -> *mut i32;
    }
    #[cfg(target_os = "macos")]
    return unsafe { __error() };
    #[cfg(target_os = "linux")]
    unsafe extern "C" {
        fn __errno_location() -> *mut i32;
    }
    #[cfg(target_os = "linux")]
    return unsafe { __errno_location() };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        static mut ERRNO: i32 = 0;
        return &raw mut ERRNO;
    }
}

fn dup_fd(fd: RawFd, path: &Path) -> Result<OwnedFd, BundleError> {
    Ok(unsafe { OwnedFd::from_raw_fd(libc_dup_checked(fd, path)?) })
}

fn libc_dup_checked(fd: RawFd, path: &Path) -> Result<RawFd, BundleError> {
    let duplicated = unsafe { libc_dup(fd) };
    if duplicated < 0 {
        return Err(io_err("dup directory", path, io::Error::last_os_error()));
    }
    Ok(duplicated)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn o_nofollow() -> i32 {
    0x20000
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
fn o_nofollow() -> i32 {
    0x100
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn o_directory() -> i32 {
    0o200000
}

#[cfg(target_os = "macos")]
fn o_directory() -> i32 {
    0x100000
}

#[cfg(all(
    unix,
    not(any(target_os = "linux", target_os = "android", target_os = "macos"))
))]
fn o_directory() -> i32 {
    0
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn o_nonblock() -> i32 {
    0o4000
}

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
fn o_nonblock() -> i32 {
    0x4
}

#[cfg(any(target_os = "illumos", target_os = "solaris"))]
fn o_nonblock() -> i32 {
    0x80
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly",
    target_os = "illumos",
    target_os = "solaris"
)))]
fn o_nonblock() -> i32 {
    0x4
}
#[cfg(any(target_os = "linux", target_os = "android"))]
fn o_cloexec() -> i32 {
    0o2000000
}

#[cfg(target_os = "macos")]
fn o_cloexec() -> i32 {
    0x1000000
}

#[cfg(all(
    unix,
    not(any(target_os = "linux", target_os = "android", target_os = "macos"))
))]
fn o_cloexec() -> i32 {
    0
}

unsafe fn libc_openat(dir_fd: RawFd, path: *const std::os::raw::c_char, flags: i32) -> RawFd {
    unsafe extern "C" {
        fn openat(dirfd: RawFd, pathname: *const std::os::raw::c_char, flags: i32, ...) -> RawFd;
    }
    unsafe { openat(dir_fd, path, flags, 0) }
}

unsafe fn libc_dup(fd: RawFd) -> RawFd {
    unsafe extern "C" {
        fn dup(fd: RawFd) -> RawFd;
    }
    unsafe { dup(fd) }
}

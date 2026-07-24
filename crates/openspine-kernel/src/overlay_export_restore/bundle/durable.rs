//! Durable filesystem publication primitives for authenticated bundles.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::{
    DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};

use super::{BundleError, BundleManifest, DATA_DIR, MANIFEST_FILE};

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

pub(super) fn require_bundle_shape(bundle_dir: &Path) -> Result<(), BundleError> {
    require_dir(bundle_dir)?;
    let mut names = fs::read_dir(bundle_dir)
        .map_err(|source| io_err("read bundle directory", bundle_dir, source))?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| io_err("read bundle entry", bundle_dir, source))?;
    names.sort();
    if names
        != [
            std::ffi::OsString::from(DATA_DIR),
            std::ffi::OsString::from(MANIFEST_FILE),
        ]
    {
        return Err(BundleError::TreeMismatch(
            "bundle root must contain only data and manifest.json".into(),
        ));
    }
    require_dir(&bundle_dir.join(DATA_DIR))?;
    if !nofollow_meta(&bundle_dir.join(MANIFEST_FILE))?.is_file() {
        return Err(BundleError::TreeMismatch(
            "manifest is not a regular file".into(),
        ));
    }
    Ok(())
}

pub(super) fn write_manifest(path: &Path, manifest: &BundleManifest) -> Result<(), BundleError> {
    let bytes = serde_json::to_vec(manifest)
        .map_err(|error| BundleError::InvalidManifest(error.to_string()))?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(path)
        .map_err(|source| io_err("create manifest", path, source))?;
    file.write_all(&bytes)
        .map_err(|source| io_err("write manifest", path, source))?;
    set_mode(path, 0o600)?;
    file.sync_all()
        .map_err(|source| io_err("sync manifest", path, source))
}

pub(super) fn write_empty_file(path: &Path) -> Result<(), BundleError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let file = options
        .open(path)
        .map_err(|source| mutation_or_io(path, "create tombstone", source))?;
    set_mode(path, 0o600)?;
    file.sync_all()
        .map_err(|source| io_err("sync tombstone", path, source))
}

pub(super) fn open_regular_nofollow(
    path: &Path,
    operation: &'static str,
) -> Result<File, BundleError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(o_nofollow() | o_nonblock());
    let file = options
        .open(path)
        .map_err(|source| mutation_or_io(path, operation, source))?;
    if !file
        .metadata()
        .map_err(|source| io_err("inspect open file", path, source))?
        .is_file()
    {
        return Err(BundleError::TreeMismatch(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    Ok(file)
}

pub(super) fn create_temp_dir(root: &Path, name: &str) -> Result<PathBuf, BundleError> {
    for _ in 0..128 {
        let sequence = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = root.join(format!(".{name}.tmp-{}-{sequence}", std::process::id()));
        match create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(BundleError::Io { source, .. })
                if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(BundleError::Io {
        operation: "create unique temporary directory",
        path: root.to_path_buf(),
        source: io::Error::new(io::ErrorKind::AlreadyExists, "temporary name exhaustion"),
    })
}

pub(super) fn create_dir(path: &Path) -> Result<(), BundleError> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    builder
        .create(path)
        .map_err(|source| io_err("create directory", path, source))?;
    set_mode(path, 0o700)
}

pub(super) fn require_dir(path: &Path) -> Result<(), BundleError> {
    if !nofollow_meta(path)?.is_dir() {
        return Err(BundleError::TreeMismatch(format!(
            "{} is not a directory",
            path.display()
        )));
    }
    Ok(())
}

pub(super) fn path_exists_nofollow(path: &Path) -> Result<bool, BundleError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(io_err("inspect destination", path, source)),
    }
}

pub(super) fn ensure_absent(path: &Path) -> Result<(), BundleError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(BundleError::AlreadyExists(path.to_path_buf())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_err("inspect destination", path, source)),
    }
}

pub(super) fn nofollow_meta(path: &Path) -> Result<fs::Metadata, BundleError> {
    fs::symlink_metadata(path).map_err(|source| io_err("inspect path", path, source))
}

pub(super) fn sync_dir(path: &Path) -> Result<(), BundleError> {
    require_dir(path)?;
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|source| io_err("sync directory", path, source))
}

pub(super) fn set_mode(path: &Path, mode: u32) -> Result<(), BundleError> {
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|source| io_err("set permissions", path, source))?;
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

#[cfg(unix)]
pub(super) fn same_state(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.is_file()
        && b.is_file()
        && a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.len() == b.len()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}

#[cfg(not(unix))]
pub(super) fn same_state(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.is_file() && b.is_file() && a.len() == b.len() && a.modified().ok() == b.modified().ok()
}

pub(super) fn valid_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\\')
}

pub(super) fn mutation_or_io(
    path: &Path,
    operation: &'static str,
    source: io::Error,
) -> BundleError {
    if is_eloop(&source) {
        BundleError::ConcurrentMutation(path.to_path_buf())
    } else if source.kind() == io::ErrorKind::IsADirectory
        || source.kind() == io::ErrorKind::NotADirectory
        || source.raw_os_error() == Some(20)
        || source.raw_os_error() == Some(21)
    {
        BundleError::TreeMismatch(format!(
            "{}: wrong entry type during {operation}",
            path.display()
        ))
    } else {
        io_err(operation, path, source)
    }
}

pub(super) fn io_err(operation: &'static str, path: &Path, source: io::Error) -> BundleError {
    BundleError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

fn is_eloop(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(40) | Some(62))
        || error.kind() == io::ErrorKind::TooManyLinks
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

#[cfg(target_os = "linux")]
pub(super) fn atomic_rename_noreplace(src: &Path, dst: &Path) -> Result<(), BundleError> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_uint};
    use std::os::unix::ffi::OsStrExt as _;

    unsafe extern "C" {
        fn renameat2(
            olddirfd: c_int,
            oldpath: *const c_char,
            newdirfd: c_int,
            newpath: *const c_char,
            flags: c_uint,
        ) -> c_int;
    }
    let old = CString::new(src.as_os_str().as_bytes())
        .map_err(|_| BundleError::TreeMismatch("NUL in source path".into()))?;
    let new = CString::new(dst.as_os_str().as_bytes())
        .map_err(|_| BundleError::TreeMismatch("NUL in destination path".into()))?;
    if unsafe { renameat2(-100, old.as_ptr(), -100, new.as_ptr(), 1) } == 0 {
        Ok(())
    } else {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::AlreadyExists {
            Err(BundleError::AlreadyExists(dst.to_path_buf()))
        } else {
            Err(io_err("publish bundle", dst, error))
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn atomic_rename_noreplace(src: &Path, dst: &Path) -> Result<(), BundleError> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_uint};
    use std::os::unix::ffi::OsStrExt as _;

    unsafe extern "C" {
        fn renamex_np(old: *const c_char, new: *const c_char, flags: c_uint) -> c_int;
    }
    let old = CString::new(src.as_os_str().as_bytes())
        .map_err(|_| BundleError::TreeMismatch("NUL in source path".into()))?;
    let new = CString::new(dst.as_os_str().as_bytes())
        .map_err(|_| BundleError::TreeMismatch("NUL in destination path".into()))?;
    if unsafe { renamex_np(old.as_ptr(), new.as_ptr(), 4) } == 0 {
        Ok(())
    } else {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::AlreadyExists {
            Err(BundleError::AlreadyExists(dst.to_path_buf()))
        } else {
            Err(io_err("publish bundle", dst, error))
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn atomic_rename_noreplace(src: &Path, dst: &Path) -> Result<(), BundleError> {
    ensure_absent(dst)?;
    fs::rename(src, dst).map_err(|source| io_err("publish bundle", dst, source))
}

pub(super) struct Cleanup {
    path: PathBuf,
    keep: bool,
}

impl Cleanup {
    pub(super) fn new(path: PathBuf) -> Self {
        Self { path, keep: false }
    }

    pub(super) fn keep(&mut self) {
        self.keep = true;
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

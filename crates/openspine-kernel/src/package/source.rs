//! Bounded descriptor-relative capture. No symlink-following, lossy names,
//! recursive traversal, unchecked second reads, or unbounded directory lists.
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::Read as _;
use std::os::fd::{AsRawFd as _, FromRawFd as _};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

use super::{InspectionError as Error, FAMILIES, MAX_FILES, MAX_FILE_BYTES, MAX_TOTAL_BYTES};

pub(super) fn capture(directory: &Path) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    // Trailing '/' or '/.' otherwise makes the OS follow a final symlink even
    // with O_NOFOLLOW. Remove redundant components without resolving '..'.
    let directory: PathBuf = directory.components().collect();
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(directory)
        .map_err(|_| Error::SourceUnavailable)?;
    capture_opened(&root)
}

pub(super) fn capture_opened(root: &File) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    // Independent directory cursor: fdopendir's duplicate shares its offset.
    let root = open_at(root, ".", true)?;
    let mut capture = Capture {
        files: BTreeMap::new(),
        aliases: BTreeSet::new(),
        total_bytes: 0,
        remaining_entries: MAX_FILES + FAMILIES.len() + 1,
    };
    for name in names(&root, &mut capture.remaining_entries)? {
        if FAMILIES.contains(&name.as_str()) || name == "docs" {
            let child = open_at(&root, &name, true)?;
            for file in names(&child, &mut capture.remaining_entries)? {
                if name == "docs" {
                    if !is_document(&file) {
                        return Err(Error::UnsupportedPayload);
                    }
                } else if !matches!(
                    Path::new(&file).extension().and_then(|e| e.to_str()),
                    Some("yaml" | "yml")
                ) {
                    return Err(Error::UnsupportedPayload);
                }
                capture.file(&child, &file, format!("{name}/{file}"))?;
            }
        } else {
            if name != "package.yaml" && !is_document(&name) {
                return Err(Error::UnsupportedPayload);
            }
            capture.file(&root, &name, name.clone())?;
        }
    }
    Ok(capture.files)
}

struct Capture {
    files: BTreeMap<String, Vec<u8>>,
    aliases: BTreeSet<String>,
    total_bytes: usize,
    remaining_entries: usize,
}

impl Capture {
    fn file(&mut self, parent: &File, name: &str, path: String) -> Result<(), Error> {
        if self.files.len() >= MAX_FILES {
            return Err(Error::LimitExceeded);
        }
        if !self.aliases.insert(path.to_ascii_lowercase()) {
            return Err(Error::InvalidPath);
        }
        let file = open_at(parent, name, false)?;
        let metadata = file.metadata().map_err(|_| Error::SourceUnavailable)?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(Error::SourceUnavailable);
        }
        if metadata.mode() & 0o111 != 0 {
            return Err(Error::UnsupportedPayload);
        }
        let allowance = MAX_FILE_BYTES.min(MAX_TOTAL_BYTES - self.total_bytes);
        if metadata.len() > allowance as u64 {
            return Err(Error::LimitExceeded);
        }
        let mut bytes = Vec::new();
        file.take(allowance as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::SourceUnavailable)?;
        if bytes.len() > allowance {
            return Err(Error::LimitExceeded);
        }
        if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
            return Err(Error::UnsupportedPayload);
        }
        self.total_bytes += bytes.len();
        if self.files.insert(path, bytes).is_some() {
            return Err(Error::InvalidPath);
        }
        Ok(())
    }
}

fn is_document(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("txt")
        })
}

fn portable_name(name: &str) -> bool {
    if name.is_empty()
        || name.len() > 128
        || name.ends_with('.')
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return false;
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_lowercase();
    !matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        && !(stem.len() == 4
            && (stem.starts_with("com") || stem.starts_with("lpt"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

pub(super) fn open_at(parent: &File, name: &str, directory: bool) -> Result<File, Error> {
    let name = CString::new(name).map_err(|_| Error::InvalidPath)?;
    let flags = libc::O_RDONLY
        | libc::O_NOFOLLOW
        | libc::O_CLOEXEC
        | if directory {
            libc::O_DIRECTORY
        } else {
            libc::O_NONBLOCK
        };
    // SAFETY: parent remains open, name is NUL-terminated, no O_CREAT is used.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err(Error::SourceUnavailable);
    }
    // SAFETY: openat returned a new owned descriptor; File closes it once.
    Ok(unsafe { File::from_raw_fd(fd) })
}

struct DirectoryStream(*mut libc::DIR);
impl Drop for DirectoryStream {
    fn drop(&mut self) {
        // SAFETY: this stream is uniquely owned and valid until this drop.
        unsafe {
            libc::closedir(self.0);
        }
    }
}

pub(super) fn names(parent: &File, remaining: &mut usize) -> Result<Vec<String>, Error> {
    // fdopendir takes ownership. Duplicate without leaking across exec.
    // SAFETY: parent is live; fcntl creates an independent owned descriptor.
    let duplicate = unsafe { libc::fcntl(parent.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
    if duplicate < 0 {
        return Err(Error::SourceUnavailable);
    }
    // SAFETY: duplicate is owned and refers to a directory.
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        // SAFETY: fdopendir failed, so ownership did not transfer.
        unsafe {
            libc::close(duplicate);
        }
        return Err(Error::SourceUnavailable);
    }
    let stream = DirectoryStream(stream);
    let mut names = Vec::new();
    loop {
        // SAFETY: errno is thread-local; readdir's pointer is read before the
        // next readdir call and while the owning stream remains live.
        let entry = unsafe {
            *errno_pointer() = 0;
            libc::readdir(stream.0)
        };
        if entry.is_null() {
            // SAFETY: errno is thread-local and read immediately after readdir.
            if unsafe { *errno_pointer() } != 0 {
                return Err(Error::SourceUnavailable);
            }
            break;
        }
        // SAFETY: a successful readdir returns a NUL-terminated d_name.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }
            .to_str()
            .map_err(|_| Error::InvalidPath)?;
        if matches!(name, "." | "..") {
            continue;
        }
        if *remaining == 0 {
            return Err(Error::LimitExceeded);
        }
        *remaining -= 1;
        if !portable_name(name) {
            return Err(Error::InvalidPath);
        }
        names.push(name.to_owned());
    }
    names.sort();
    Ok(names)
}

unsafe fn errno_pointer() -> *mut libc::c_int {
    #[cfg(target_os = "linux")]
    {
        unsafe { libc::__errno_location() }
    }
    #[cfg(target_os = "macos")]
    {
        unsafe { libc::__error() }
    }
}

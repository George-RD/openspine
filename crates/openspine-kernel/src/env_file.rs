//! Key material loading from an `openspine.env` file beside the configuration.
//!
//! `config::artifact_key_bytes`, `config::webhook_hmac_secret`, and
//! `grant_hmac_key` read only the process environment. An installed binary that
//! is launched directly (no shell wrapper sourcing an env file) would therefore
//! fail on absent key material even when the owner has a perfectly good key
//! file next to their configuration. This module closes that gap.
//!
//! Precedence is one-directional: a variable already present in the process
//! environment is never overwritten, so an operator-supplied environment always
//! wins over the file.

use std::path::{Path, PathBuf};

/// The env file name the kernel looks for beside the resolved config path.
pub const ENV_FILE_NAME: &str = "openspine.env";

#[derive(Debug, thiserror::Error)]
pub enum EnvFileError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "{path} holds key material but is readable by other accounts (mode {mode:04o}); \
         run: chmod 600 {path}"
    )]
    Permissions { path: PathBuf, mode: u32 },
    #[error("{path} line {line}: expected NAME=VALUE")]
    Malformed { path: PathBuf, line: usize },
}

/// What [`load_adjacent`] did, for reporting by the setup wizard.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Loaded {
    /// The file that was read, or `None` when no file exists.
    pub path: Option<PathBuf>,
    /// Names taken from the file into the process environment.
    pub applied: Vec<String>,
    /// Names the file defines that the process environment already had.
    pub retained: Vec<String>,
}

/// The env file path for a resolved config path.
pub fn path_for(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join(ENV_FILE_NAME)
}

/// Read the env file beside `config_path` and export every entry the process
/// environment does not already define. A missing file is not an error.
pub fn load_adjacent(config_path: &Path) -> Result<Loaded, EnvFileError> {
    let path = path_for(config_path);
    // Mode is checked before the read: a key file other accounts can read is
    // refused, not consumed and then complained about.
    match reject_shared_readable(&path) {
        Ok(()) => {}
        Err(EnvFileError::Read { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Loaded::default())
        }
        Err(error) => return Err(error),
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Loaded::default())
        }
        Err(source) => {
            return Err(EnvFileError::Read {
                path: path.clone(),
                source,
            })
        }
    };

    let mut loaded = Loaded {
        path: Some(path.clone()),
        ..Loaded::default()
    };
    for (name, value) in parse(&text, &path)? {
        if std::env::var_os(&name).is_some() {
            loaded.retained.push(name);
            continue;
        }
        std::env::set_var(&name, value);
        loaded.applied.push(name);
    }
    Ok(loaded)
}

/// Refuse a key file any account other than its owner can read.
#[cfg(unix)]
fn reject_shared_readable(path: &Path) -> Result<(), EnvFileError> {
    use std::os::unix::fs::PermissionsExt as _;

    let metadata = std::fs::metadata(path).map_err(|source| EnvFileError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(EnvFileError::Permissions {
            path: path.to_path_buf(),
            mode,
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_shared_readable(_path: &Path) -> Result<(), EnvFileError> {
    Ok(())
}

/// Parse `NAME=VALUE` lines. Blank lines and `#` comments are skipped, a
/// leading `export ` is tolerated, and one matched pair of surrounding quotes
/// is stripped. No escape sequences and no variable interpolation: this is key
/// material, not a shell script.
fn parse(text: &str, path: &Path) -> Result<Vec<(String, String)>, EnvFileError> {
    let mut entries = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let malformed = || EnvFileError::Malformed {
            path: path.to_path_buf(),
            line: index + 1,
        };
        let (name, value) = line.split_once('=').ok_or_else(malformed)?;
        let name = name.trim_end();
        if !is_identifier(name) {
            return Err(malformed());
        }
        entries.push((name.to_string(), unquote(value.trim()).to_string()));
    }
    Ok(entries)
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

/// Render an env file for the supplied entries, owner-readable only.
pub fn render(entries: &[(String, String)]) -> String {
    let mut out = String::from(
        "# Generated by `openspine init`. Key material: keep this file at mode 0600.\n",
    );
    for (name, value) in entries {
        out.push_str(name);
        out.push('=');
        out.push_str(value);
        out.push('\n');
    }
    out
}

/// Add the entries the file does not already define, preserving everything in
/// it, and return the names that were added.
///
/// An existing name is never overwritten: its value is what the vault on disk
/// was encrypted under, and replacing it would orphan that vault.
pub fn merge_owner_only(
    path: &Path,
    entries: &[(String, String)],
) -> Result<Vec<String>, EnvFileError> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => {
            reject_shared_readable(path)?;
            text
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(EnvFileError::Read {
                path: path.to_path_buf(),
                source,
            })
        }
    };
    let defined: Vec<String> = parse(&existing, path)?
        .into_iter()
        .map(|(name, _)| name)
        .collect();

    let mut appended = String::new();
    let mut added = Vec::new();
    for (name, value) in entries {
        if defined.iter().any(|held| held == name) {
            continue;
        }
        appended.push_str(&format!("{name}={value}\n"));
        added.push(name.clone());
    }
    if added.is_empty() {
        return Ok(added);
    }

    let mut merged = if existing.is_empty() {
        String::from("# Written by `openspine init`. Key material: keep this file at mode 0600.\n")
    } else {
        let mut text = existing;
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text
    };
    merged.push_str(&appended);
    write_owner_only(path, &merged).map_err(|source| EnvFileError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(added)
}

/// Write `contents` to `path` so that no other account can ever read it.
///
/// Key material is written into a file that is created fresh at mode `0600` and
/// then renamed over the target. The two obvious shorter forms both leak:
/// `fs::write` followed by `chmod` creates at `0666 & !umask` first, and
/// `OpenOptions::mode` is ignored for a file that already exists, so truncating
/// an existing `0666` file would write the secret into it before any `chmod`
/// could run. `create_new` guarantees the mode applies, and the rename carries
/// it onto the target atomically.
pub fn write_owner_only(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    use std::io::Write as _;

    let parent = match path.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(parent) => {
            std::fs::create_dir_all(parent)?;
            parent.to_path_buf()
        }
        None => PathBuf::from("."),
    };
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| ENV_FILE_NAME.to_string());
    let staging = parent.join(format!(".{name}.{}.tmp", ulid::Ulid::new()));

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }

    let write = (|| -> Result<(), std::io::Error> {
        let mut file = options.open(&staging)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&staging, path)
    })();
    if write.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    write
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openspine-env-{tag}-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn env_file_sits_beside_the_config_file() {
        assert_eq!(
            path_for(Path::new("/var/lib/openspine/openspine.yaml")),
            PathBuf::from("/var/lib/openspine/openspine.env")
        );
        assert_eq!(
            path_for(Path::new("openspine.yaml")),
            PathBuf::from("./openspine.env")
        );
    }

    #[test]
    fn parse_reads_comments_exports_and_quotes() {
        let entries = parse(
            "# comment\n\nexport OPENSPINE_ARTIFACT_KEY=abc\nQUOTED=\"has space\"\nSINGLE='v'\n",
            Path::new("t.env"),
        )
        .unwrap();
        assert_eq!(
            entries,
            vec![
                ("OPENSPINE_ARTIFACT_KEY".to_string(), "abc".to_string()),
                ("QUOTED".to_string(), "has space".to_string()),
                ("SINGLE".to_string(), "v".to_string()),
            ]
        );
    }

    #[test]
    fn parse_rejects_a_line_without_an_assignment() {
        let error = parse("GOOD=1\nnonsense\n", Path::new("t.env")).unwrap_err();
        assert!(
            matches!(error, EnvFileError::Malformed { line: 2, .. }),
            "{error}"
        );
    }

    #[test]
    fn parse_rejects_a_name_that_is_not_an_identifier() {
        let error = parse("BAD NAME=1\n", Path::new("t.env")).unwrap_err();
        assert!(
            matches!(error, EnvFileError::Malformed { line: 1, .. }),
            "{error}"
        );
    }

    #[test]
    fn absent_env_file_is_not_an_error() {
        let dir = temp_dir("absent");
        let loaded = load_adjacent(&dir.join("openspine.yaml")).unwrap();
        assert_eq!(loaded, Loaded::default());
    }

    #[cfg(unix)]
    #[test]
    fn group_readable_env_file_is_refused_with_its_mode() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = temp_dir("perm");
        let path = dir.join(ENV_FILE_NAME);
        std::fs::write(&path, "OPENSPINE_ARTIFACT_KEY=abc\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

        let error = load_adjacent(&dir.join("openspine.yaml")).unwrap_err();
        let rendered = error.to_string();
        assert!(rendered.contains("0640"), "{rendered}");
        assert!(rendered.contains("chmod 600"), "{rendered}");
    }

    /// Two distinct leaks are pinned here. Final mode `0600` alone is not
    /// enough: truncating a pre-existing `0666` file writes the secret into it
    /// before any `chmod` runs. The inode assertion proves the secret went into
    /// a freshly created file instead of the wide-open one.
    #[cfg(unix)]
    #[test]
    fn generated_key_file_is_never_readable_by_other_accounts() {
        use std::os::unix::fs::MetadataExt as _;
        use std::os::unix::fs::PermissionsExt as _;

        let dir = temp_dir("umask");
        let path = dir.join(ENV_FILE_NAME);
        std::fs::write(&path, "stale\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        let exposed_inode = std::fs::metadata(&path).unwrap().ino();

        write_owner_only(&path, "OPENSPINE_SMOKE_MODE=v\n").unwrap();

        let metadata = std::fs::metadata(&path).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "mode was {mode:04o}");
        assert_ne!(
            metadata.ino(),
            exposed_inode,
            "secret was written into the pre-existing world-readable file"
        );
        assert!(!dir.read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));
    }
    #[test]
    fn write_owner_only_produces_a_loadable_file() {
        let dir = temp_dir("write");
        let path = dir.join(ENV_FILE_NAME);
        let rendered = render(&[("OPENSPINE_SMOKE_UNSET".to_string(), "value".to_string())]);
        write_owner_only(&path, &rendered).unwrap();

        let loaded = load_adjacent(&dir.join("openspine.yaml")).unwrap();
        assert_eq!(loaded.path, Some(path));
        assert_eq!(loaded.applied, vec!["OPENSPINE_SMOKE_UNSET".to_string()]);
        assert_eq!(
            std::env::var("OPENSPINE_SMOKE_UNSET").ok(),
            Some("value".to_string())
        );
    }

    #[test]
    fn a_set_variable_is_retained_over_the_file() {
        let dir = temp_dir("retain");
        std::env::set_var("OPENSPINE_SMOKE_PRESET", "from-environment");
        write_owner_only(
            &dir.join(ENV_FILE_NAME),
            "OPENSPINE_SMOKE_PRESET=from-file\n",
        )
        .unwrap();

        let loaded = load_adjacent(&dir.join("openspine.yaml")).unwrap();
        assert_eq!(loaded.retained, vec!["OPENSPINE_SMOKE_PRESET".to_string()]);
        assert!(loaded.applied.is_empty());
        assert_eq!(
            std::env::var("OPENSPINE_SMOKE_PRESET").ok(),
            Some("from-environment".to_string())
        );
    }
}

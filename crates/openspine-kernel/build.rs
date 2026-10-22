//! Runtime compatibility identity for package selection (ANO-822).
//!
//! Emits `OPENSPINE_BUILD_INPUT_FINGERPRINT`: one bounded, deterministic digest
//! over every workspace input that can change how a package transition is
//! evaluated or loaded - Rust sources, manifests, the lockfile and toolchain pin,
//! the embedded production workflow YAML - plus the target, profile, enabled
//! features and the format of this fingerprint. It reads no secret, no clock and
//! nothing from `.git`. It names build inputs; it is not a claim that the
//! resulting executable is bit-for-bit reproducible.
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Bump when the set or encoding of fingerprinted inputs changes.
const FORMAT: u32 = 1;
const MAX_FILES: usize = 20_000;
const MAX_BYTES: u64 = 256 * 1024 * 1024;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let root = manifest.join("../..");
    let mut files = BTreeMap::new();
    let mut total = 0u64;
    for name in ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"] {
        add(&root, &root.join(name), &mut files, &mut total);
    }
    for entry in sorted_entries(&root.join("crates")) {
        add(&root, &entry.join("Cargo.toml"), &mut files, &mut total);
        walk(&root, &entry.join("src"), &mut files, &mut total, &|path| {
            path.extension().is_some_and(|extension| extension == "rs")
        });
        add(&root, &entry.join("build.rs"), &mut files, &mut total);
    }
    // The production workflow seeds are embedded with include_str!.
    walk(
        &root,
        &root.join("artifacts/overlay-seeds"),
        &mut files,
        &mut total,
        &|path| {
            path.extension()
                .is_some_and(|extension| extension == "yaml")
        },
    );
    let mut hasher = Sha256::new();
    hasher.update(format!("openspine:build-input-fingerprint:v{FORMAT}\0"));
    for key in [
        "TARGET",
        "PROFILE",
        "CARGO_CFG_TARGET_FEATURE",
        "CARGO_CFG_PANIC",
    ] {
        field(&mut hasher, key.as_bytes());
        field(
            &mut hasher,
            std::env::var(key).unwrap_or_default().as_bytes(),
        );
    }
    let mut features: Vec<String> = std::env::vars()
        .filter_map(|(name, _)| name.strip_prefix("CARGO_FEATURE_").map(str::to_owned))
        .collect();
    features.sort();
    field(&mut hasher, features.join(",").as_bytes());
    for (path, bytes) in &files {
        field(&mut hasher, path.as_bytes());
        field(&mut hasher, bytes);
    }
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    println!("cargo:rustc-env=OPENSPINE_BUILD_INPUT_FINGERPRINT=sha256:{digest}");
    println!("cargo:rerun-if-changed=build.rs");
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "crates",
        "artifacts/overlay-seeds",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
}

fn field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn sorted_entries(directory: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<_> = fs::read_dir(directory)
        .map(|iter| {
            iter.filter_map(Result::ok)
                .map(|entry| entry.path())
                .collect()
        })
        .unwrap_or_default();
    entries.sort();
    entries
}

fn walk(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
    total: &mut u64,
    include: &dyn Fn(&Path) -> bool,
) {
    for path in sorted_entries(directory) {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            walk(root, &path, files, total, include);
        } else if metadata.is_file() && include(&path) {
            add(root, &path, files, total);
        }
    }
}

/// A missing optional input is recorded as absent, never silently skipped.
fn add(root: &Path, path: &Path, files: &mut BTreeMap<String, Vec<u8>>, total: &mut u64) {
    let relative = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let bytes = fs::read(path).ok();
    *total += bytes.as_ref().map_or(0, |bytes| bytes.len() as u64);
    assert!(
        files.len() < MAX_FILES && *total <= MAX_BYTES,
        "build-input fingerprint exceeded its bound"
    );
    let marker = if bytes.is_some() {
        b"present"
    } else {
        b"absent "
    };
    let mut entry = marker.to_vec();
    entry.extend(bytes.unwrap_or_default());
    files.insert(relative, entry);
}

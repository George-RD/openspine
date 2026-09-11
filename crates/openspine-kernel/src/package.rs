//! Read-only local package inspection (#273/#274).
//!
//! Only current typed validation constructs a validated snapshot. Its bytes and
//! report are immutable to callers: installers and reviewers use those bytes,
//! not a reopened source path. Byte integrity alone never implies compatibility,
//! publisher authentication or authority.

use std::collections::BTreeMap;
use std::path::Path;

use openspine_schemas::digest::{digest_of, digest_of_bytes, Digest};
use serde::Serialize;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod source;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod source {
    use super::*;
    pub(super) fn capture(_: &Path) -> Result<BTreeMap<String, Vec<u8>>, InspectionError> {
        Err(InspectionError::SourceUnavailable)
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod compare;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod current_state;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod install;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "package/install_tests.rs"]
mod install_tests;
pub(crate) mod install_types;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod object_fs;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod object_store;
mod validation;

pub(super) const MAX_FILES: usize = 4096;
pub(super) const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const INVENTORY_FORMAT_VERSION: u32 = 1;
pub(super) const FAMILIES: [&str; 7] = [
    "agents",
    "routes",
    "workflows",
    "packs",
    "templates",
    "policies",
    "golden_sets",
];

/// Inventory-v1 hashes canonical JSON `{inventory_format_version: 1, files}`,
/// where files are sorted by normalized relative path. Digests cover raw bytes.
#[derive(Debug, Serialize)]
pub(crate) struct InventoryFile {
    path: String,
    bytes: usize,
    digest: Digest,
}

/// Bounded, content-free reporting. Descriptive text and artifact identifiers
/// are intentionally not echoed: their schemas do not impose display limits.
#[derive(Debug, Serialize)]
pub(crate) struct InspectionReport {
    schema_version: u32,
    inventory_format_version: u32,
    valid: bool,
    provenance: &'static str,
    package_id: String,
    revision: u32,
    content_digest: Digest,
    inventory: Vec<InventoryFile>,
}

/// Owned, bounded source bytes and their inventory; not a validated package.
/// The constructor is private to package capture code. No path, mutable byte
/// access or deserialization interface escapes to consumers.
pub(crate) struct PackageCapture {
    files: BTreeMap<String, Vec<u8>>,
    inventory: Vec<InventoryFile>,
    content_digest: Digest,
}

impl PackageCapture {
    /// Call only with the output of the bounded, no-link source capture.
    fn new(files: BTreeMap<String, Vec<u8>>) -> Self {
        let (inventory, content_digest) = inventory_of(&files);
        Self {
            files,
            inventory,
            content_digest,
        }
    }

    /// Consume only these captured bytes through the current typed loader.
    /// Private staging failures are compatibility-assessment failures, not proof
    /// of corrupt retained bytes. This structural report does not replace the
    /// installation receipt's provenance or authorize selection/activation.
    pub(crate) fn validate(self) -> Result<PackageSnapshot, InspectionError> {
        let declaration = validation::validate(&self.files)?;
        let report = InspectionReport {
            schema_version: 1,
            inventory_format_version: INVENTORY_FORMAT_VERSION,
            valid: true,
            provenance: "local-unverified",
            package_id: declaration.id,
            revision: declaration.version,
            content_digest: self.content_digest,
            inventory: self.inventory,
        };
        Ok(PackageSnapshot {
            files: self.files,
            report,
        })
    }
}

pub(crate) struct PackageSnapshot {
    files: BTreeMap<String, Vec<u8>>,
    report: InspectionReport,
}

impl PackageSnapshot {
    /// Bind package metadata, manifest bytes and complete inventory to one identity.
    /// This is a local integrity identity, not publisher or activation approval.
    pub(crate) fn identity(&self) -> install_types::PackageIdentity {
        install_types::PackageIdentity {
            package_id: self.report.package_id.clone(),
            revision: self.report.revision,
            inventory_format_version: self.report.inventory_format_version,
            content_digest: self.report.content_digest.clone(),
            manifest_digest: digest_of_bytes(&self.files["package.yaml"]),
        }
    }

    /// Borrow the report produced by validation without reopening source files.
    pub(crate) fn report(&self) -> &InspectionReport {
        &self.report
    }

    /// The exact validated bytes. No source path or mutable access escapes.
    pub(crate) fn files(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
    }

    /// Render bounded inspection metadata without echoing candidate document text.
    pub(crate) fn summary(&self) -> String {
        let bytes: usize = self.files().map(|(_, bytes)| bytes.len()).sum();
        format!(
            "Package: {}\nRevision: {}\nContent digest: {}\nInventory: v1, {} files, {} bytes\n\
             Validation: passed (structure only)\nProvenance: local-unverified\n\
             Publisher not verified. This command does not install or select a package.\n",
            self.report.package_id,
            self.report.revision,
            self.report.content_digest,
            self.report.inventory.len(),
            bytes,
        )
    }
}

/// Capture first, then hash and validate exactly those bytes. The temporary
/// loader tree contains only captured files; the live source is never reread.
pub(crate) fn inspect(directory: &Path) -> Result<PackageSnapshot, InspectionError> {
    PackageCapture::new(source::capture(directory)?).validate()
}

/// Byte identity only: no filesystem writes, loader validation, or trusted
/// snapshot construction. Inspection and retained-object checks hash alike.
fn inventory_of(files: &BTreeMap<String, Vec<u8>>) -> (Vec<InventoryFile>, Digest) {
    let inventory: Vec<_> = files
        .iter()
        .map(|(path, bytes)| InventoryFile {
            path: path.clone(),
            bytes: bytes.len(),
            digest: digest_of_bytes(bytes),
        })
        .collect();
    let content_digest = digest_of(&serde_json::json!({
        "inventory_format_version": INVENTORY_FORMAT_VERSION,
        "files": inventory,
    }));
    (inventory, content_digest)
}

/// Deliberately bounded diagnostics. Never relay a parser's candidate text,
/// terminal controls, an untrusted path, or an owner-configuration remedy.
#[derive(Debug, thiserror::Error)]
pub(crate) enum InspectionError {
    #[error("Cannot read package source. Use a readable ordinary directory without links or special files.")]
    SourceUnavailable,
    #[error("Package paths must be unambiguous portable ASCII names of at most 128 bytes per component.")]
    InvalidPath,
    #[error("Unsupported payload. Use package.yaml, flat artifact directories, and non-executable UTF-8 .md/.txt documentation.")]
    UnsupportedPayload,
    #[error("Package exceeds 4096 files, 8 MiB per file, or 64 MiB total.")]
    LimitExceeded,
    #[error("Required package.yaml declaration is missing.")]
    DeclarationMissing,
    #[error("Invalid package.yaml. Check schema-v1 fields, unique keys and IDs, and positive integer revision.")]
    DeclarationInvalid,
    #[error("A captured artifact fails the existing typed loader's validation.")]
    ArtifactInvalid,
    #[error("Duplicate artifact identity/version, or duplicate unversioned golden-set identity.")]
    ArtifactCollision,
    #[error("Declared artifact IDs do not match all captured loadable artifacts.")]
    InventoryMismatch,
    #[error(
        "The entry agent must resolve to an active agent at its highest declared source version."
    )]
    EntryAgentInvalid,
    #[error("Cannot create or read the private temporary validation snapshot. Check temporary-directory access and free space.")]
    StagingUnavailable,
}

impl InspectionError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::SourceUnavailable => "source-unavailable",
            Self::InvalidPath => "path-invalid",
            Self::UnsupportedPayload => "payload-unsupported",
            Self::LimitExceeded => "limit-exceeded",
            Self::DeclarationMissing => "declaration-missing",
            Self::DeclarationInvalid => "declaration-invalid",
            Self::ArtifactInvalid => "artifact-invalid",
            Self::ArtifactCollision => "artifact-collision",
            Self::InventoryMismatch => "inventory-mismatch",
            Self::EntryAgentInvalid => "entry-agent-invalid",
            Self::StagingUnavailable => "staging-unavailable",
        }
    }
}

#[cfg(test)]
#[path = "package/tests.rs"]
mod tests;

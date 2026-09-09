//! Content-free installation identities. None of these types conveys authority.
use openspine_schemas::digest::Digest;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PackageProvenance {
    LocalUnverified,
    RuntimeBundled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageIdentity {
    pub package_id: String,
    pub revision: u32,
    pub inventory_format_version: u32,
    pub content_digest: Digest,
    pub manifest_digest: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstallMetadata {
    pub installation_id: Ulid,
    pub identity: PackageIdentity,
    pub provenance: PackageProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstallReceipt {
    pub installation_id: Ulid,
    pub package_id: String,
    pub revision: u32,
    pub inventory_format_version: u32,
    pub content_digest: Digest,
    pub manifest_digest: Digest,
    pub provenance: PackageProvenance,
    pub installed_at: String,
    pub audit_id: Ulid,
    pub audit_seq: i64,
}

impl InstallReceipt {
    pub(crate) fn identity(&self) -> PackageIdentity {
        PackageIdentity {
            package_id: self.package_id.clone(),
            revision: self.revision,
            inventory_format_version: self.inventory_format_version,
            content_digest: self.content_digest.clone(),
            manifest_digest: self.manifest_digest.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct InstallationResult {
    pub schema_version: u32,
    pub status: &'static str,
    pub selected: bool,
    pub active: bool,
    pub activation_supported: bool,
    pub idempotent_retry: bool,
    pub receipt: InstallReceipt,
    pub cleanup_pending: bool,
}

impl InstallationResult {
    pub(crate) fn new(receipt: InstallReceipt, idempotent_retry: bool) -> Self {
        Self {
            schema_version: 1,
            status: "installed-inactive",
            selected: false,
            active: false,
            activation_supported: false,
            idempotent_retry,
            receipt,
            cleanup_pending: false,
        }
    }
}

/// Fixed diagnostics never include source bytes, parser text, or owner secrets.
#[derive(Debug, thiserror::Error)]
pub(crate) enum InstallError {
    #[error(transparent)]
    Inspection(#[from] super::InspectionError),
    #[error("Cannot read existing operator configuration and keys. Installation does not initialize or replace them.")]
    Configuration,
    #[error("Cannot open the existing kernel store with its normal migrations and integrity checks. No alternate ledger was created.")]
    Ledger,
    #[error("Another process owns the data directory, or its operation lock cannot be verified. Stop the runtime before package maintenance.")]
    Locked,
    #[error("An export or restore is pending. Complete the normal startup recovery before package maintenance.")]
    PendingOperation,
    #[error("Package storage must be an operator-owned, non-linked directory separate from active artifact paths.")]
    Destination,
    #[error("Package storage could not complete a durable filesystem operation. Inspect package receipts before retrying.")]
    Publication,
    #[error("This package ID and revision already identify different content. Existing bytes were not replaced.")]
    RevisionConflict,
    #[error("The retained package object is missing or corrupt. It was not repaired or replaced from the candidate.")]
    ObjectCorrupt,
    #[error("Use exact canonical installation IDs from package list or installation receipts.")]
    InvalidInstallationId,
    #[error("No committed installation matches one of those IDs. Use package list to find exact installation IDs.")]
    InstallationNotFound,
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    #[error("Native package maintenance is supported only on Linux and macOS.")]
    UnsupportedPlatform,
}

impl InstallError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Inspection(error) => error.code(),
            Self::Configuration => "configuration-unavailable",
            Self::Ledger => "ledger-unavailable",
            Self::Locked => "data-directory-locked",
            Self::PendingOperation => "pending-overlay-operation",
            Self::Destination => "destination-invalid",
            Self::Publication => "publication-failed",
            Self::RevisionConflict => "revision-conflict",
            Self::ObjectCorrupt => "object-corrupt",
            Self::InvalidInstallationId => "installation-id-invalid",
            Self::InstallationNotFound => "installation-not-found",
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            Self::UnsupportedPlatform => "unsupported-platform",
        }
    }
}

/// Debug-only crash seam for real subprocess tests. No release-mode switch.
pub(crate) fn crash_at(point: &str) {
    #[cfg(debug_assertions)]
    if std::env::var("OPENSPINE_TEST_PACKAGE_CRASH").as_deref() == Ok(point) {
        std::process::exit(75);
    }
    #[cfg(not(debug_assertions))]
    let _ = point;
}

/// Debug-only rendezvous in a test-owned directory. No release-mode I/O or
/// environment switch; even an abandoned test cannot pause a command forever.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn test_barrier(point: &str) -> Result<(), InstallError> {
    #[cfg(debug_assertions)]
    if std::env::var("OPENSPINE_TEST_PACKAGE_PAUSE").as_deref() == Ok(point) {
        use std::time::{Duration, Instant};
        let directory = std::env::var_os("OPENSPINE_TEST_PACKAGE_BARRIER")
            .map(std::path::PathBuf::from)
            .ok_or(InstallError::Locked)?;
        std::fs::File::create_new(directory.join(format!("{point}.ready")))
            .map_err(|_| InstallError::Locked)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while !directory.join(format!("{point}.release")).is_file() {
            if Instant::now() >= deadline {
                return Err(InstallError::Locked);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = point;
    Ok(())
}

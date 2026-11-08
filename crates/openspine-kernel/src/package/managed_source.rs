//! Resolve the ledger-selected installation into captured, validated bytes.
//!
//! This never consults the legacy configured directory, the bundled package,
//! an earlier revision or an unindexed object: any failure leaves the runtime
//! unavailable. Serving and selection callers hold the data-root lifetime lock.
//! Readiness copies validated selection evidence in a short read-only SQLite
//! snapshot, closes it, then uses the same object-capture validator below.
//! Its point-in-time result is never a startup authorization.
use super::install_types::InstallReceipt;
use super::object_store::PackageObjectReader;
use super::selection_types::SelectionRecord;
use super::{InspectionError, PackageSnapshot};
use crate::store::Store;
use std::path::Path;

pub(crate) struct ManagedBase {
    pub record: SelectionRecord,
    pub receipt: InstallReceipt,
    pub snapshot: PackageSnapshot,
}

/// Distinct fixed reasons: integrity and compatibility stay separate.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ManagedBaseError {
    #[error("The package selection ledger failed validation. No other base was used.")]
    Ledger,
    #[error("The selected package object is missing or corrupt. No other base was used.")]
    ObjectCorrupt,
    #[error(
        "The selected package is intact but today's runtime rejects it. No other base was used."
    )]
    Incompatible(InspectionError),
}

/// `Ok(None)` is legacy mode and occurs only when no selection history exists:
/// ledger validation fails closed on a lost projection, so sticky managed mode
/// can never degrade into it.
pub(crate) fn resolve(
    store: &Store,
    data_root: &Path,
) -> Result<Option<ManagedBase>, ManagedBaseError> {
    store
        .validate_package_selection()
        .map_err(|_| ManagedBaseError::Ledger)?;
    let Some((record, receipt)) = store
        .selected_package()
        .map_err(|_| ManagedBaseError::Ledger)?
    else {
        return Ok(None);
    };
    let snapshot = capture_selected(&receipt, data_root)?;
    Ok(Some(ManagedBase {
        record,
        receipt,
        snapshot,
    }))
}

/// Validate the exact retained identity after ledger evidence has been captured.
/// Serving callers hold the lifetime lock; diagnostics hold no database lock.
pub(crate) fn capture_selected(
    receipt: &InstallReceipt,
    data_root: &Path,
) -> Result<PackageSnapshot, ManagedBaseError> {
    let reader =
        PackageObjectReader::open(data_root).map_err(|_| ManagedBaseError::ObjectCorrupt)?;
    let captured = reader
        .capture(&receipt.identity())
        .map_err(|_| ManagedBaseError::ObjectCorrupt)?;
    let snapshot = captured
        .validate()
        .map_err(ManagedBaseError::Incompatible)?;
    if snapshot.identity() != receipt.identity() {
        return Err(ManagedBaseError::ObjectCorrupt);
    }
    Ok(snapshot)
}

#[cfg(test)]
#[path = "managed_source_tests.rs"]
pub(crate) mod tests;

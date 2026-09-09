//! Exact retained-byte evidence. This is neither a loader nor an approval path.
use super::install_types::{InstallError as Error, InstallReceipt};
use super::object_store::PackageObjects;
use super::InventoryFile;
use crate::store::Store;
use openspine_schemas::digest::Digest;
use serde::Serialize;
use std::collections::BTreeMap;
use ulid::Ulid;

/// Parse here, not in clap: parser diagnostics must never echo selector text.
pub(crate) fn installation_id(value: &str) -> Result<Ulid, Error> {
    if value.len() != 26 {
        return Err(Error::InvalidInstallationId);
    }
    let id: Ulid = value.parse().map_err(|_| Error::InvalidInstallationId)?;
    if id.to_string() != value {
        return Err(Error::InvalidInstallationId);
    }
    Ok(id)
}

#[derive(Serialize)]
pub(crate) struct ComparisonReport {
    schema_version: u32,
    status: &'static str,
    basis: &'static str,
    from: InstallReceipt,
    to: InstallReceipt,
    identical: bool,
    counts: Counts,
    changes: Vec<FileChange>,
    runtime_inputs_changed: bool,
    package_declaration_changed: bool,
    runtime_compatibility: &'static str,
    authority_review: &'static str,
    publisher_verification: &'static str,
    activation_supported: bool,
    active_package_identified: bool,
    notice: &'static str,
}

#[derive(Default, Serialize)]
struct Counts {
    added: usize,
    removed: usize,
    modified: usize,
    unchanged: usize,
}

#[derive(PartialEq, Eq, Serialize)]
struct FileState {
    bytes: usize,
    digest: Digest,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum ChangeKind {
    Added,
    Removed,
    Modified,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum FileCategory {
    PackageDeclaration,
    RuntimeInput,
    Documentation,
}

#[derive(Serialize)]
struct FileChange {
    path: String,
    change: ChangeKind,
    category: FileCategory,
    before: Option<FileState>,
    after: Option<FileState>,
}

/// The caller holds the ordinary maintenance lock and has validated the ledger.
/// No installation recovery, fresh schema validation, or mutation takes place.
pub(crate) fn compare(
    store: &Store,
    objects: &PackageObjects,
    from_id: Ulid,
    to_id: Ulid,
) -> Result<ComparisonReport, Error> {
    let receipts = store.installed_packages().map_err(|_| Error::Ledger)?;
    let lookup = |id| {
        receipts
            .iter()
            .find(|receipt| receipt.installation_id == id)
            .cloned()
            .ok_or(Error::InstallationNotFound)
    };
    let from = lookup(from_id)?;
    let to = lookup(to_id)?;
    // These inventories come from the same captures that passed integrity
    // checks. Even identical selectors must verify; never reopen for the diff.
    let before = objects.inventory(&from.identity())?;
    let after = objects.inventory(&to.identity())?;
    let (counts, changes) = changes(before, after);
    Ok(ComparisonReport {
        schema_version: 1,
        status: "comparison-complete",
        basis: "retained-byte-integrity",
        from,
        to,
        identical: changes.is_empty(),
        counts,
        runtime_inputs_changed: changes
            .iter()
            .any(|change| change.category == FileCategory::RuntimeInput),
        package_declaration_changed: changes
            .iter()
            .any(|change| change.category == FileCategory::PackageDeclaration),
        changes,
        runtime_compatibility: "not-evaluated",
        authority_review: "not-performed",
        publisher_verification: "not-performed",
        activation_supported: false,
        active_package_identified: false,
        notice: "Comparison is not activation approval. Neither endpoint is identified as the active package.",
    })
}

fn changes(before: Vec<InventoryFile>, after: Vec<InventoryFile>) -> (Counts, Vec<FileChange>) {
    // Each verified inventory is bounded to 4,096 paths. This union therefore
    // holds at most 8,192 entries, never package bodies or an unbounded text diff.
    let mut paths: BTreeMap<String, (Option<FileState>, Option<FileState>)> = BTreeMap::new();
    for file in before {
        paths.entry(file.path).or_default().0 = Some(FileState {
            bytes: file.bytes,
            digest: file.digest,
        });
    }
    for file in after {
        paths.entry(file.path).or_default().1 = Some(FileState {
            bytes: file.bytes,
            digest: file.digest,
        });
    }
    let mut counts = Counts::default();
    let mut changes = Vec::new();
    for (path, (before, after)) in paths {
        if before == after {
            counts.unchanged += 1;
            continue;
        }
        let change = match (&before, &after) {
            (None, Some(_)) => {
                counts.added += 1;
                ChangeKind::Added
            }
            (Some(_), None) => {
                counts.removed += 1;
                ChangeKind::Removed
            }
            (Some(_), Some(_)) => {
                counts.modified += 1;
                ChangeKind::Modified
            }
            (None, None) => unreachable!("union entries come from an inventory"),
        };
        changes.push(FileChange {
            category: category(&path),
            path,
            change,
            before,
            after,
        });
    }
    (counts, changes)
}

fn category(path: &str) -> FileCategory {
    if path == "package.yaml" {
        FileCategory::PackageDeclaration
    } else if path.split_once('/').is_some_and(|(family, file)| {
        super::FAMILIES.contains(&family) && (file.ends_with(".yaml") || file.ends_with(".yml"))
    }) {
        FileCategory::RuntimeInput
    } else {
        FileCategory::Documentation
    }
}

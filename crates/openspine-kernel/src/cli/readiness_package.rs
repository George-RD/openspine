//! Snapshot-only package diagnostics. Never acquires the serving lifetime lock.
use super::{absolute, Check};
use crate::config::Config;
use crate::package::install_types::{InstallReceipt, PackageProvenance};
use crate::package::managed_source::ManagedBaseError;
use crate::store::Store;
use std::path::Path;

pub(super) fn checks(config: &Config, config_path: &Path) -> Vec<Check> {
    let configured_root = absolute(&config.data_dir);
    let root = std::fs::symlink_metadata(&configured_root)
        .ok()
        .filter(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        .and_then(|_| configured_root.canonicalize().ok())
        .unwrap_or(configured_root);
    let db = root.join("kernel.db");
    // The wizard may create the vault/data root without bootstrapping a kernel
    // ledger. Only an absent database AND absent package namespace is fresh.
    let absent = |path: &Path| matches!(std::fs::symlink_metadata(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound);
    if absent(&db) && absent(&root.join("packages")) {
        return vec![legacy(config, config_path).0];
    }
    if let Err(error) = crate::managed_startup::require_package_ledger(&root) {
        let remedy = match &error {
            crate::managed_startup::PackageLedgerError::Missing => {
                "Preserve the remaining instance state for investigation. Use a separate empty data_dir only to initialize a deliberately new instance; do not replace the ledger with an older database."
            }
            crate::managed_startup::PackageLedgerError::Inspection(_) => {
                "Preserve the instance state and inspect local filesystem access to the package namespace and ledger. Readiness never repairs or creates a database."
            }
        };
        return vec![Check::fail(
            "package",
            "agent package",
            error.to_string(),
            remedy.into(),
        )];
    }
    let store = match Store::open_for_readiness(&db) {
        Ok(store) => store,
        Err(error) => {
            let remedy = if matches!(error, crate::store::ReadinessStoreError::Busy) {
                "The database is busy, not known to be damaged. Retry readiness shortly."
            } else {
                "Stop the service and use normal runtime startup to migrate an older ledger; for damaged state, preserve the database and investigate its integrity. Readiness never repairs or selects a fallback."
            };
            return vec![Check::fail(
                "package",
                "agent package",
                error.to_string(),
                remedy.into(),
            )];
        }
    };
    let selected = store.selected.as_ref();
    // The Store constructor has already closed its SQLite snapshot. Only
    // copied, validated evidence participates in filesystem/loader work.
    let resolved = selected
        .map(|(_, receipt)| crate::package::managed_source::capture_selected(receipt, &root))
        .transpose();
    let mut legacy_product = None;
    let mut checks = match resolved {
        Ok(Some(_)) => {
            let (record, receipt) = selected.expect("captured selection");
            vec![Check::pass(
                "package", "agent package",
                format!("committed selection generation={} {}; lyra_dir ignored; next startup uses these captured bytes (no hot reload)",
                    record.generation, identity(receipt)),
            )]
        }
        Ok(None) => {
            let (check, product) = legacy(config, config_path);
            legacy_product = product;
            vec![check]
        }
        Err(error) => {
            let code = match error {
                ManagedBaseError::Ledger => "selection-ledger-invalid",
                ManagedBaseError::ObjectCorrupt => "selected-object-corrupt",
                ManagedBaseError::Incompatible(_) => "selected-package-incompatible",
            };
            vec![unavailable(
                code,
                selected.as_ref().map(|(_, receipt)| receipt),
            )]
        }
    };
    let receipts = &store.installations;
    let selected_id = selected
        .as_ref()
        .map(|(_, receipt)| receipt.installation_id);
    let product = selected
        .as_ref()
        .map(|(_, receipt)| receipt.package_id.as_str())
        .or(legacy_product.as_deref());
    // Keep a bounded report even for a long-lived installation index. All exact
    // receipts remain available through the explicit package list command.
    let mut inactive_count = 0;
    for receipt in receipts
        .iter()
        .filter(|r| Some(r.installation_id) != selected_id)
    {
        inactive_count += 1;
        if inactive_count > 32 {
            continue;
        }
        let restriction = if product.is_some_and(|p| p != receipt.package_id) {
            "; different-product: cannot use in this instance"
        } else {
            "; not selected, compatibility and owner review still required"
        };
        checks.push(Check::pass(
            &format!("package.candidate.{}", receipt.installation_id),
            "installed-inactive candidate",
            format!("{}{restriction}", identity(receipt)),
        ));
    }
    if inactive_count > 32 {
        checks.push(Check::pass(
            "package.candidates",
            "installed-inactive candidates",
            format!(
                "{} additional receipts; use `openspine package list --json` for exact identities",
                inactive_count - 32
            ),
        ));
    }
    checks
}

fn identity(receipt: &InstallReceipt) -> String {
    format!("product={} revision={} installation={} content={} manifest={} provenance={} (local source provenance, not publisher authentication)",
        serde_json::to_string(&receipt.package_id).expect("string serialization"), receipt.revision,
        receipt.installation_id, receipt.content_digest, receipt.manifest_digest,
        match receipt.provenance { PackageProvenance::LocalUnverified => "local-unverified", PackageProvenance::RuntimeBundled => "runtime-bundled" })
}

fn unavailable(code: &str, receipt: Option<&InstallReceipt>) -> Check {
    Check::fail("package", "agent package", format!("unavailable selected package: {code}; {}", receipt.map(identity).unwrap_or_else(|| "exact identity cannot be verified".into())),
        "Preserve the database, receipts and retained objects. Resolve the named integrity or runtime compatibility failure through normal operator recovery; do not edit lyra_dir, restore an older database or clear durable-work fences. No fallback was used.".into())
}

fn legacy(config: &Config, config_path: &Path) -> (Check, Option<String>) {
    let mut check = super::legacy_package_check(config, config_path);
    let captured_identity = crate::package::inspect(&config.lyra_dir)
        .ok()
        .map(|snapshot| snapshot.identity());
    let identity = captured_identity.as_ref().map(|identity| {
        format!("product={} revision={} content={} manifest={} installation=none provenance=local-unverified (not publisher authentication)",
            serde_json::to_string(&identity.package_id).expect("string serialization"), identity.revision, identity.content_digest, identity.manifest_digest)
    }).unwrap_or_else(|| "exact declared identity unavailable; installation=none (legacy directory loading)".into());
    check.detail = format!("legacy configured base: {}; {identity}", check.detail);
    (check, captured_identity.map(|identity| identity.package_id))
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "readiness_package_tests.rs"]
mod tests;

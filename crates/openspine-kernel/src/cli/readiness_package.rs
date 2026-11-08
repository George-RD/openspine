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
    // A genuinely fresh bootstrap has no ledger yet. A missing db in an
    // existing state root is not evidence of legacy mode.
    if !root.exists() {
        return vec![legacy(config, config_path)];
    }
    let store = match Store::open_for_readiness(&db) {
        Ok(store) => store,
        Err(error) => return vec![Check::fail(
            "package", "agent package", error.to_string(),
            "Stop the service and use normal runtime startup to migrate an older ledger; for damaged state, preserve the database and investigate its integrity. Readiness never repairs or selects a fallback.".into(),
        )],
    };
    let selected = match store.selected() {
        Ok(selected) => selected,
        Err(_) => return vec![unavailable("selection-ledger-invalid", None)],
    };
    let resolved = store.resolve(&root);
    let mut checks = match resolved {
        Ok(Some(base)) => vec![Check::pass(
            "package", "agent package",
            format!("committed selection generation={} {}; lyra_dir ignored; next startup uses these captured bytes (no hot reload)",
                base.record.generation, identity(&base.receipt)),
        )],
        Ok(None) => vec![legacy(config, config_path)],
        Err(error) => {
            let code = match error {
                ManagedBaseError::Ledger => "selection-ledger-invalid",
                ManagedBaseError::ObjectCorrupt => "selected-object-corrupt",
                ManagedBaseError::Incompatible(_) => "selected-package-incompatible",
            };
            vec![unavailable(code, selected.as_ref().map(|(_, receipt)| receipt))]
        }
    };
    let receipts = match store.installations() {
        Ok(receipts) => receipts,
        Err(_) => {
            checks.push(unavailable("installation-ledger-invalid", None));
            return checks;
        }
    };
    let selected_id = selected
        .as_ref()
        .map(|(_, receipt)| receipt.installation_id);
    let product = selected
        .as_ref()
        .map(|(_, receipt)| receipt.package_id.clone())
        .or_else(|| {
            crate::package::inspect(&config.lyra_dir)
                .ok()
                .map(|snapshot| snapshot.identity().package_id)
        });
    // Keep a bounded report even for a long-lived installation index. All exact
    // receipts remain available through the explicit package list command.
    let inactive: Vec<_> = receipts
        .iter()
        .filter(|r| Some(r.installation_id) != selected_id)
        .collect();
    for receipt in inactive.iter().take(32) {
        let restriction = if product.as_ref().is_some_and(|p| p != &receipt.package_id) {
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
    if inactive.len() > 32 {
        checks.push(Check::pass(
            "package.candidates",
            "installed-inactive candidates",
            format!(
                "{} additional receipts; use `openspine package list --json` for exact identities",
                inactive.len() - 32
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

fn legacy(config: &Config, config_path: &Path) -> Check {
    let mut check = super::legacy_package_check(config, config_path);
    let identity = crate::package::inspect(&config.lyra_dir).ok().map(|snapshot| {
        let identity = snapshot.identity();
        format!("product={} revision={} content={} manifest={} installation=none provenance=local-unverified (not publisher authentication)",
            serde_json::to_string(&identity.package_id).expect("string serialization"), identity.revision, identity.content_digest, identity.manifest_digest)
    }).unwrap_or_else(|| "exact declared identity unavailable; installation=none (legacy directory loading)".into());
    check.detail = format!("legacy configured base: {}; {identity}", check.detail);
    check
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "readiness_package_tests.rs"]
mod tests;

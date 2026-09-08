//! Local operator orchestration: retained bytes -> durable object -> index/audit.
use super::install_types::{
    crash_at, InstallError as Error, InstallationResult, PackageProvenance,
};
use super::{object_store::PackageObjects, PackageSnapshot};
use crate::store::{
    package_install::{CommitResult, PrepareResult},
    Store,
};

pub(crate) fn recover(store: &Store, objects: &PackageObjects) -> Result<(), Error> {
    for id in store
        .recover_package_attempts()
        .map_err(|_| Error::Ledger)?
    {
        objects.cleanup(id)?;
        store
            .finish_package_staging_cleanup(id)
            .map_err(|_| Error::Ledger)?;
    }
    Ok(())
}

pub(crate) fn install(
    store: &Store,
    objects: &PackageObjects,
    snapshot: &PackageSnapshot,
    provenance: PackageProvenance,
) -> Result<InstallationResult, Error> {
    recover(store, objects)?;
    let metadata = match store
        .prepare_package_install(snapshot.identity(), provenance)
        .map_err(|_| Error::Ledger)?
    {
        PrepareResult::Existing(receipt) => {
            objects.verify(&receipt.identity())?;
            return Ok(InstallationResult::new(receipt, true));
        }
        PrepareResult::Conflict => return Err(Error::RevisionConflict),
        PrepareResult::Prepared(metadata) => metadata,
    };
    crash_at("after-prepared");
    let result = (|| {
        objects.publish(metadata.installation_id, snapshot)?;
        objects.check_anchors()?;
        match store
            .commit_package_install(&metadata)
            .map_err(|_| Error::Ledger)?
        {
            CommitResult::Committed(receipt) => Ok(InstallationResult::new(receipt, false)),
            CommitResult::Existing(receipt) => Ok(InstallationResult::new(receipt, true)),
            CommitResult::Conflict => Err(Error::RevisionConflict),
        }
    })();
    if result.is_err() {
        store
            .interrupt_package_install(&metadata)
            .map_err(|_| Error::Ledger)?;
    } else {
        crash_at("after-commit");
    }
    let cleanup = objects.cleanup(metadata.installation_id).and_then(|()| {
        store
            .finish_package_staging_cleanup(metadata.installation_id)
            .map_err(|_| Error::Ledger)
    });
    match result {
        Ok(mut result) => {
            // Publication and the success receipt are already committed. A
            // cleanup failure must not erase that truth; retry recovery owns it.
            result.cleanup_pending = cleanup.is_err();
            Ok(result)
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn list(store: &Store, objects: &PackageObjects) -> Result<serde_json::Value, Error> {
    recover(store, objects)?;
    let receipts = store.installed_packages().map_err(|_| Error::Ledger)?;
    let packages: Vec<_> = receipts
        .into_iter()
        .map(|receipt| {
            let availability = if objects.verify(&receipt.identity()).is_ok() {
                "available"
            } else {
                "unavailable-or-corrupt"
            };
            serde_json::json!({
                "receipt": receipt, "status": "installed-inactive", "availability": availability,
                "selected": false, "active": false, "activation_supported": false,
            })
        })
        .collect();
    Ok(serde_json::json!({"schema_version": 1, "packages": packages}))
}

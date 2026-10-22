//! Startup base selection. A ledger-backed selection is authoritative and is
//! resolved before anything with an external effect starts; legacy `lyra_dir`
//! mode exists only while no selection history exists.
use std::path::Path;

use anyhow::Context as _;

use crate::overlay_startup::BaseSource;
use crate::store::Store;

/// Resolve the effective base. A corrupt, missing or incompatible managed
/// selection is an error: never the legacy directory, the bundled package, an
/// earlier revision or an unindexed object.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn startup_base<'a>(
    store: &Store,
    data_root: &Path,
    lyra_dir: &'a Path,
) -> anyhow::Result<BaseSource<'a>> {
    match crate::package::managed_source::resolve(store, data_root)
        .context("resolving the committed package selection")?
    {
        None => Ok(BaseSource::Legacy(lyra_dir)),
        Some(base) => {
            tracing::info!(
                generation = base.record.generation,
                installation_id = %base.receipt.installation_id,
                package = %base.receipt.package_id,
                revision = base.receipt.revision,
                content_digest = %base.receipt.content_digest,
                "starting from the managed package selection"
            );
            if lyra_dir != crate::config::default_lyra_dir() {
                tracing::warn!(
                    generation = base.record.generation,
                    "managed package selection is authoritative; the configured lyra_dir is ignored"
                );
            }
            Ok(BaseSource::Managed(Box::new(
                base.snapshot.registry().clone(),
            )))
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn startup_base<'a>(
    store: &Store,
    _data_root: &Path,
    lyra_dir: &'a Path,
) -> anyhow::Result<BaseSource<'a>> {
    store
        .validate_package_selection()
        .context("validating package selection")?;
    anyhow::ensure!(
        store.package_selection()?.is_none(),
        "a managed package selection cannot be consumed on this platform"
    );
    Ok(BaseSource::Legacy(lyra_dir))
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "managed_startup_tests.rs"]
mod tests;

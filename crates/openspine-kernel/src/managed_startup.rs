//! Startup base selection. A ledger-backed selection is authoritative and is
//! resolved before anything with an external effect starts; legacy `lyra_dir`
//! mode exists only while no selection history exists.
use std::path::Path;

use anyhow::Context as _;

use crate::overlay_startup::BaseSource;
use crate::store::Store;

/// Run under the lifetime lock, after pending restore recovery and before any
/// store is opened. The namespace alone is conservative evidence of package
/// history: even emptied or damaged storage must not recreate a legacy ledger.
pub(crate) fn require_package_ledger(data_root: &Path) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(data_root.join("packages")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("inspecting the package namespace"),
        Ok(_) => {}
    }
    match std::fs::metadata(data_root.join("kernel.db")) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => anyhow::bail!(
            "package-ledger-missing: package storage exists but kernel.db is missing. \
             Refusing to create a replacement ledger or fall back to lyra_dir. \
             Stop and preserve this instance's remaining state for investigation. \
             To start a new instance, configure a separate empty data_dir and initialize it; \
             do not reuse this package namespace or roll back authority history."
        ),
        Err(error) => Err(error).context("inspecting the existing package ledger"),
    }
}

/// The runtime entry's readiness gate and base capture share one mode decision.
pub(crate) fn base_for_run<'a>(
    store: &Store,
    data_root: &Path,
    config: &'a crate::config::Config,
    config_path: &Path,
) -> anyhow::Result<BaseSource<'a>> {
    let base = startup_base(store, data_root, &config.lyra_dir)?;
    if matches!(&base, BaseSource::Legacy(_)) {
        crate::cli::readiness::require_package(config, config_path)?;
    }
    Ok(base)
}

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

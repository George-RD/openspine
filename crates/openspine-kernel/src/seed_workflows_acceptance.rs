//! D-175: explicit init consent, separate from ordinary runtime discovery.
use anyhow::Context as _;
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::ids::PrincipalId;
use std::path::Path;

use crate::artifact_loader::{self, ArtifactRegistry};
use crate::artifact_store::ArtifactStore;
use crate::store::learned_artifacts::{CompatibilityStatus, Provenance};
use crate::store::seed_acceptance::SeedAcceptance;
use crate::store::Store;

pub(crate) fn accept_from_init(config: &crate::config::Config) -> anyhow::Result<usize> {
    let key = crate::config::artifact_key_bytes()?;
    let operations = crate::overlay_export_restore::acquire(&config.data_dir, &key)
        .context("acquiring the data-root lifetime lock; stop any running OpenSpine before init")?;
    let root = operations.canonical_data_root();
    // Init must not operate on a generation awaiting restore/finalization.
    anyhow::ensure!(
        !operations.has_pending_operation()?,
        "pending overlay export/restore; complete kernel recovery before init"
    );
    operations.process_pre_open(false, jiff::Timestamp::now())?;
    let store = Store::open(&root.join("kernel.db"))?;
    let artifacts = ArtifactStore::open(root.join("artifacts"), key)?;
    crate::validate_startup_and_reconcile_overlay_terminal_erasures(
        &store,
        &artifacts,
        &operations,
        root,
        jiff::Timestamp::now().as_millisecond(),
    )?;
    let owner = crate::identity::bootstrap_owner_principal(
        &store,
        config.owner.telegram_user_id,
        &config.owner.display_name,
    )?;
    let base = artifact_loader::load_base_registry(&config.lyra_dir)?;
    accept_embedded(
        &store,
        &artifacts,
        &root.join("artifacts.d"),
        &base,
        owner.principal_id,
    )
}

pub(super) fn accept_embedded(
    store: &Store,
    artifacts: &ArtifactStore,
    overlay_dir: &Path,
    base: &ArtifactRegistry,
    owner: PrincipalId,
) -> anyhow::Result<usize> {
    super::materialize_missing(store, overlay_dir)?;
    let base_ids = artifact_loader::artifact_identity_pairs(base);
    let epoch = crate::overlay_compat::compatibility_epoch(base, &base_ids);
    let learned = store.list_learned_artifacts()?;
    let mut accepted = 0;
    for manifest in super::parsed()? {
        if base_ids.contains(&("workflow".into(), manifest.id.clone())) {
            continue;
        }
        let path = overlay_dir
            .join("workflows")
            .join(artifact_loader::overlay_filename(
                &manifest.id,
                manifest.version,
            ));
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).context("reading workflow seed for owner consent"),
        };
        let embedded = super::seed_yaml(&manifest.id);
        if bytes != embedded.as_bytes() {
            continue;
        }
        let digest = digest_of_bytes(&bytes);
        if let Some(row) = learned.iter().find(|row| {
            row.kind == "workflow"
                && row.artifact_id == manifest.id
                && row.version == manifest.version
        }) {
            if !matches!(row.provenance, Provenance::LegacyMigration { .. })
                || row.compatibility != CompatibilityStatus::ReconfirmationRequired
                || row.pending_yaml_digest.as_deref() != Some(digest.as_str())
            {
                continue;
            }
        }
        std::fs::File::open(&path)?.sync_all()?;
        std::fs::File::open(path.parent().expect("workflow directory"))?.sync_all()?;
        let parsed = artifact_loader::parse_proposal("workflow", embedded)?;
        let dangling = crate::overlay_compat::dangling_for_parsed(base, &parsed);
        // The encrypted exchange is the receipt of this owner action, not a
        // fabricated conversation or an unbound event identifier.
        let exchange = serde_json::to_vec(&serde_json::json!({
            "ceremony": "openspine.init.seed_acceptance", "decision": "D-175",
            "owner_principal": owner, "kind": "workflow", "id": manifest.id,
            "version": manifest.version, "embedded_digest": digest,
            "accepted_base_epoch": epoch, "accepted_dangling_refs": dangling,
        }))?;
        let exchange = artifacts.put(&exchange)?;
        let yaml = artifacts.put(&bytes)?;
        accepted += usize::from(store.accept_init_seed(SeedAcceptance {
            owner,
            id: &manifest.id,
            version: manifest.version,
            yaml,
            exchange,
            source_path: &path.to_string_lossy(),
            base_epoch: &epoch,
            dangling: &dangling,
        })?);
    }
    Ok(accepted)
}

#[cfg(test)]
#[path = "seed_workflows_acceptance_tests.rs"]
mod tests;

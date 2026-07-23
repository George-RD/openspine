//! Provenance-bound admission for learnable persona overlays.

use anyhow::Context as _;
use std::collections::HashMap;
use std::path::Path;

use crate::artifact_loader::{self, ArtifactRegistry};
use crate::artifact_store::ArtifactStore;
use crate::store::learned_artifacts::{CompatibilityStatus, LearnedArtifact, Provenance};
use crate::store::Store;

/// Admit only persona YAML whose learned row, validated ledger event, exchange,
/// and digest all agree. Invalid rows are quarantined from this boot.
pub(crate) fn admit(
    store: &Store,
    artifacts: &ArtifactStore,
    overlay_dir: &Path,
    learned: &[LearnedArtifact],
    registry: &mut ArtifactRegistry,
) -> anyhow::Result<()> {
    let mut admitted = HashMap::new();
    for row in learned
        .iter()
        .filter(|item| item.kind == "persona" && item.compatibility != CompatibilityStatus::Erased)
    {
        let Provenance::ProducedBy {
            source_event_id,
            source_exchange,
            source_scope,
        } = &row.provenance
        else {
            tracing::warn!(artifact_id = %row.artifact_id, version = row.version,
                "excluding persona with non-ProducedBy provenance");
            continue;
        };
        let Some(event) = store
            .validated_audit_event_by_id(*source_event_id)
            .with_context(|| {
                format!(
                    "resolving persona {} v{} provenance event",
                    row.artifact_id, row.version
                )
            })?
        else {
            tracing::warn!(artifact_id = %row.artifact_id, version = row.version,
                "excluding persona with unavailable provenance event");
            continue;
        };
        if !event
            .payload_refs
            .iter()
            .any(|reference| reference == source_exchange)
        {
            tracing::warn!(artifact_id = %row.artifact_id, version = row.version,
                "excluding persona with unbound provenance exchange");
            continue;
        }
        if artifacts
            .get_scoped(*source_scope, source_exchange)
            .is_err()
        {
            tracing::warn!(artifact_id = %row.artifact_id, version = row.version,
                "excluding persona with unavailable provenance exchange");
            continue;
        }
        if let Some(expected) = row.pending_yaml_digest.as_deref() {
            admitted.insert((row.artifact_id.clone(), row.version), expected.to_string());
        }
    }
    artifact_loader::load_admitted_personas(registry, overlay_dir, &admitted)
        .context("admitting provenance-backed persona overlays")
}

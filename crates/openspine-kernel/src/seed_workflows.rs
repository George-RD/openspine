//! AD-153 minimal seed workflow set, shipped as OVERLAY artifacts.
//!
//! Every seed is a [`WorkflowManifest`] in the D-087..D-090 declarative
//! state-machine shape (exact state/transition ids, approval-required states,
//! digest-bound manifests). The seeds are embedded at compile time and
//! materialized into a fresh install's overlay directory on first boot; from
//! then on they are ordinary learned artifacts — editable, replaceable, and
//! surviving updates untouched per AD-070 — never kernel/base fixtures
//! (AD-071/AD-080 precedent).

use std::path::Path;

use crate::artifact_loader;
use crate::store::Store;
use openspine_schemas::workflow::WorkflowManifest;

const OWNER_CONTROL: &str =
    include_str!("../../../artifacts/overlay-seeds/workflows/owner_control_conversation_seed.yaml");
const EMAIL_DRAFT: &str =
    include_str!("../../../artifacts/overlay-seeds/workflows/email_draft_with_approval_seed.yaml");
const RESEARCH_BRIEF: &str =
    include_str!("../../../artifacts/overlay-seeds/workflows/research_and_brief_seed.yaml");
const CUSTOMER_SERVICE: &str =
    include_str!("../../../artifacts/overlay-seeds/workflows/customer_service_intake_seed.yaml");

/// `(logical_id, manifest_yaml)` for every shipped seed, in stable order.
pub fn all() -> &'static [(&'static str, &'static str)] {
    &[
        ("owner_control_conversation_seed", OWNER_CONTROL),
        ("email_draft_with_approval_seed", EMAIL_DRAFT),
        ("research_and_brief_seed", RESEARCH_BRIEF),
        ("customer_service_intake_seed", CUSTOMER_SERVICE),
    ]
}

/// Parse and validate every embedded seed manifest. Used at boot to fail
/// closed on a malformed seed rather than materializing an invalid overlay
/// file, and by the acceptance tests (parse / validate / Mermaid render).
pub fn parsed() -> Result<Vec<WorkflowManifest>, SeedError> {
    let mut out = Vec::with_capacity(all().len());
    for (id, yaml) in all() {
        let manifest: WorkflowManifest =
            serde_yaml::from_str(yaml).map_err(|source| SeedError::Parse { id, source })?;
        manifest
            .validate()
            .map_err(|msg| SeedError::Invalid { id, msg })?;
        out.push(manifest);
    }
    Ok(out)
}

/// Materialize the seed workflow set into a fresh install's overlay directory.
///
/// Runs once per fresh install: a persisted marker records that seeding has
/// happened, so a seed the owner later deletes outright is NOT re-created, and
/// an owner-upgraded higher version stays live (highest-version-wins). An
/// existing file is never overwritten, so an owner's overlay edits survive.
pub fn materialize_missing(store: &Store, overlay_dir: &Path) -> anyhow::Result<usize> {
    const MARKER: &str = "seed_workflows_materialized";
    if store.get_kv(MARKER)?.is_some() {
        return Ok(0);
    }
    let written = write_seed_files(overlay_dir)?;
    store.set_kv(MARKER, "recorded")?;
    Ok(written)
}

/// Write any absent seed file into the overlay workflows directory (used by
/// `materialize_missing` and directly by tests).
pub fn write_seed_files(overlay_dir: &Path) -> anyhow::Result<usize> {
    let workflows_dir = overlay_dir.join("workflows");
    std::fs::create_dir_all(&workflows_dir)?;
    let manifests = parsed()?;
    let mut written = 0usize;
    for manifest in &manifests {
        let target = workflows_dir.join(artifact_loader::overlay_filename(
            &manifest.id,
            manifest.version,
        ));
        if target.exists() {
            continue;
        }
        std::fs::write(&target, seed_yaml(manifest.id.as_str()))?;
        written += 1;
    }
    Ok(written)
}

/// Return the embedded YAML for a seed id (used to write the file and by tests).
fn seed_yaml(id: &str) -> &'static str {
    all()
        .iter()
        .find(|(seed_id, _)| *seed_id == id)
        .map(|(_, yaml)| *yaml)
        .expect("seed id must be one of the embedded set")
}

#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    #[error("seed workflow `{id}` failed to parse: {source}")]
    Parse {
        id: &'static str,
        source: serde_yaml::Error,
    },
    #[error("seed workflow `{id}` failed validation: {msg}")]
    Invalid { id: &'static str, msg: String },
}

#[cfg(test)]
#[path = "seed_workflows_tests.rs"]
mod tests;

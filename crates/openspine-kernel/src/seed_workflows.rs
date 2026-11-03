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

#[path = "seed_workflows_acceptance.rs"]
mod acceptance;
pub(crate) use acceptance::accept_from_init;

/// Init-shipped overlay files honor owner deletion rather than being managed
/// crash-recovery files. The policy is anchored to a real audited ceremony.
pub(crate) fn honors_owner_deletion(
    store: &Store,
    row: &crate::store::learned_artifacts::LearnedArtifact,
) -> anyhow::Result<bool> {
    if row.kind != "workflow" || !all().iter().any(|(id, _)| *id == row.artifact_id) {
        return Ok(false);
    }
    let crate::store::learned_artifacts::Provenance::ProducedBy {
        source_event_id, ..
    } = &row.provenance
    else {
        return Ok(false);
    };
    Ok(store
        .validated_audit_event_by_id(*source_event_id)?
        .is_some_and(|event| event.kind.as_str() == "seed_workflows.owner_accepted"))
}

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
    std::fs::File::open(overlay_dir)?.sync_all()?;
    if let Some(parent) = overlay_dir.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
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
        use std::io::Write as _;
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        };
        file.write_all(seed_yaml(manifest.id.as_str()).as_bytes())?;
        file.sync_all()?;
        written += 1;
    }
    if written > 0 {
        std::fs::File::open(&workflows_dir)?.sync_all()?;
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

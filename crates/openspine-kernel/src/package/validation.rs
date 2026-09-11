//! Reuse the base loader on a private tree containing only captured bytes.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::Path;

use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::package::PackageDeclaration;

use super::InspectionError;
use crate::artifact_loader::{load_base_registry, ArtifactLoadError, ArtifactRegistry};

pub(super) struct ValidatedPackage {
    pub declaration: PackageDeclaration,
    pub registry: ArtifactRegistry,
}

pub(super) fn validate(
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<ValidatedPackage, InspectionError> {
    let bytes = files
        .get("package.yaml")
        .ok_or(InspectionError::DeclarationMissing)?;
    let declaration: PackageDeclaration =
        serde_yaml::from_slice(bytes).map_err(|_| InspectionError::DeclarationInvalid)?;
    let staging = tempfile::tempdir().map_err(|_| InspectionError::StagingUnavailable)?;
    for (path, bytes) in files {
        let destination = staging.path().join(path);
        let parent = destination
            .parent()
            .ok_or(InspectionError::StagingUnavailable)?;
        fs::create_dir_all(parent).map_err(|_| InspectionError::StagingUnavailable)?;
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)
            .and_then(|mut file| file.write_all(bytes))
            .map_err(|_| InspectionError::StagingUnavailable)?;
    }
    let registry = load_base_registry(staging.path()).map_err(|error| match error {
        ArtifactLoadError::Collision { .. } => InspectionError::ArtifactCollision,
        ArtifactLoadError::Read { .. } => InspectionError::StagingUnavailable,
        _ => InspectionError::ArtifactInvalid,
    })?;

    // The live loader historically filters some read_dir failures. Our source
    // walk propagates them; this additional check prevents even a skipped file
    // in private staging from being treated as a fully validated snapshot.
    let artifact_count = files
        .keys()
        .filter(|path| {
            path.split_once('/')
                .is_some_and(|(family, _)| super::FAMILIES.contains(&family))
        })
        .count();
    if registry.sources.len() != artifact_count {
        return Err(InspectionError::InventoryMismatch);
    }
    for source in registry.sources.values() {
        let relative = source
            .path
            .strip_prefix(staging.path())
            .ok()
            .and_then(Path::to_str)
            .ok_or(InspectionError::InventoryMismatch)?;
        if files.get(relative) != Some(&source.bytes) {
            return Err(InspectionError::InventoryMismatch);
        }
    }

    let declared = &declaration.artifacts;
    check(&declared.agents, registry.agents.keys().cloned())?;
    check(
        &declared.routes,
        registry.routes.iter().map(|route| route.id.clone()),
    )?;
    check(&declared.workflows, registry.workflows.keys().cloned())?;
    check(&declared.packs, registry.packs.keys().cloned())?;
    check(&declared.templates, registry.templates.keys().cloned())?;
    check(&declared.policies, registry.policies.keys().cloned())?;
    // Golden sets are natively unversioned. Do not expose the legacy loader's
    // source-map sentinel as an invented package artifact version.
    check(&declared.golden_sets, registry.golden_sets.keys().cloned())?;
    if !registry
        .agents
        .get(&declaration.entry_agent)
        .is_some_and(|agent| agent.lifecycle_state == Lifecycle::Active)
    {
        return Err(InspectionError::EntryAgentInvalid);
    }
    Ok(ValidatedPackage {
        declaration,
        registry,
    })
}

fn check(declared: &[String], actual: impl Iterator<Item = String>) -> Result<(), InspectionError> {
    if declared.iter().cloned().collect::<BTreeSet<_>>() != actual.collect::<BTreeSet<_>>() {
        return Err(InspectionError::InventoryMismatch);
    }
    Ok(())
}

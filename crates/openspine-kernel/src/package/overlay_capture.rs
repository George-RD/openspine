//! Bounded, no-link overlay capture for offline review; never invokes recovery.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use openspine_schemas::digest::Digest;

use super::InspectionError;
use crate::artifact_loader::{self, ArtifactRegistry};

pub(crate) struct CapturedOverlayFiles {
    pub registry: ArtifactRegistry,
    /// Complete inventory, including ignored/unadmitted persona files.
    pub source_inventory_digest: Digest,
    pub ignored_persona_files: Vec<String>,
}

pub(crate) fn capture(
    directory: &Path,
    admitted_personas: &HashMap<(String, u32), String>,
) -> anyhow::Result<CapturedOverlayFiles> {
    let files = match fs::symlink_metadata(directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(_) => return Err(InspectionError::SourceUnavailable.into()),
        Ok(_) => super::source::capture_overlay(directory)?,
    };
    let source_inventory_digest = super::inventory_of(&files).1;
    if files.is_empty() {
        return Ok(CapturedOverlayFiles {
            registry: ArtifactRegistry::default(),
            source_inventory_digest,
            ignored_persona_files: Vec::new(),
        });
    }
    // The legacy typed loader sees only our owned byte snapshot. The private
    // staging tree cannot follow an overlay link or reopen the live source.
    let staging = super::validation::stage_captured(&files)?;
    let mut registry = artifact_loader::load_registry_without_personas(staging.path())?;
    artifact_loader::load_admitted_personas(&mut registry, staging.path(), admitted_personas)?;
    retain_review_sources(&mut registry, &files, staging.path())?;
    let mut loaded = BTreeSet::new();
    for source in registry.sources.values_mut() {
        let relative = source
            .path
            .strip_prefix(staging.path())
            .ok()
            .and_then(Path::to_str)
            .ok_or(InspectionError::InventoryMismatch)?;
        anyhow::ensure!(
            files.get(relative) == Some(&source.bytes),
            "overlay loader source differs from its capture"
        );
        loaded.insert(relative.to_string());
        // Retain a diagnostic source location, never a temporary path or a
        // location later reopened by the deterministic review evaluator.
        source.path = directory.join(relative);
    }
    let mut ignored_persona_files = Vec::new();
    for path in files.keys().filter(|path| !loaded.contains(*path)) {
        if path.starts_with("personas/") {
            // Preserve startup's provenance gate: unadmitted/malformed
            // personas are not parsed into the effective registry.
            ignored_persona_files.push(path.clone());
        } else {
            return Err(InspectionError::InventoryMismatch.into());
        }
    }
    Ok(CapturedOverlayFiles {
        registry,
        source_inventory_digest,
        ignored_persona_files,
    })
}

/// The legacy startup loader leaves standing-rule manifests to Store-owned
/// activation and retains only some model-swap versions. Review must account
/// for every captured version. Reuse their existing typed parsers; including a
/// standing-rule manifest here does not activate its separate runtime DB row.
fn retain_review_sources(
    registry: &mut ArtifactRegistry,
    files: &BTreeMap<String, Vec<u8>>,
    staging: &Path,
) -> anyhow::Result<()> {
    let mut exact = BTreeMap::new();
    for (path, bytes) in files {
        let Some((family, _)) = path.split_once('/') else {
            return Err(InspectionError::InventoryMismatch.into());
        };
        let kind = match family {
            "standing_rules" => "standing_rule",
            "model_swaps" => "model_swap",
            _ => continue,
        };
        let parsed = artifact_loader::parse_proposal(kind, std::str::from_utf8(bytes)?)?;
        match &parsed {
            artifact_loader::ParsedProposal::StandingRule(rule) => {
                rule.validate()
                    .map_err(|_| InspectionError::ArtifactInvalid)?;
            }
            artifact_loader::ParsedProposal::ModelSwap(model) => {
                anyhow::ensure!(model.identity_valid(), "model-swap identity is invalid");
            }
            _ => return Err(InspectionError::ArtifactInvalid.into()),
        }
        let key = (
            kind.to_string(),
            parsed.artifact_id().to_string(),
            parsed.version(),
        );
        if exact.insert(key, (parsed, path, bytes)).is_some() {
            return Err(InspectionError::ArtifactCollision.into());
        }
    }
    for (key, (parsed, path, bytes)) in exact {
        if artifact_loader::artifact_version(registry, &key.0, &key.1)
            .is_none_or(|version| version < key.2)
        {
            parsed.insert_into(registry)?;
        }
        registry.sources.insert(
            key,
            artifact_loader::ArtifactSource {
                path: staging.join(path),
                bytes: bytes.clone(),
            },
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "overlay_capture_tests.rs"]
mod tests;

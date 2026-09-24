//! Owned overlay bytes for offline package review; never invokes recovery.
use std::collections::HashMap;
use std::path::Path;

use crate::artifact_loader::{self, ArtifactRegistry};

pub(crate) fn capture(
    directory: &Path,
    admitted_personas: &HashMap<(String, u32), String>,
) -> anyhow::Result<ArtifactRegistry> {
    let mut registry = artifact_loader::load_registry_without_personas(directory)?;
    artifact_loader::load_admitted_personas(&mut registry, directory, admitted_personas)?;
    Ok(registry)
}

#[cfg(test)]
#[path = "overlay_capture_tests.rs"]
mod tests;

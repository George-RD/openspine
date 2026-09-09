//! Test-only adapter over the existing live-Store pruning path.
//!
//! This intentionally does not capture Store controls. The behavioral RED
//! demonstrates why delayed pruning cannot implement captured-state review;
//! it does not assert that the shipped startup path promises such a review.

use std::collections::BTreeSet;

use crate::artifact_loader::ArtifactRegistry;
use crate::store::Store;

type ArtifactVersion = (String, String, u32);

struct CapturedVersionAdmission<'a> {
    registry: ArtifactRegistry,
    store: &'a Store,
}

struct AdmittedOverlay {
    registry: ArtifactRegistry,
    excluded: BTreeSet<ArtifactVersion>,
}

impl<'a> CapturedVersionAdmission<'a> {
    fn capture(registry: &ArtifactRegistry, store: &'a Store) -> anyhow::Result<Self> {
        Ok(Self {
            registry: registry.clone(),
            store,
        })
    }

    fn evaluate(mut self) -> anyhow::Result<AdmittedOverlay> {
        let before: BTreeSet<_> = self.registry.sources.keys().cloned().collect();
        super::prune_non_highest_active(&mut self.registry, self.store)?;
        let after: BTreeSet<_> = self.registry.sources.keys().cloned().collect();
        Ok(AdmittedOverlay {
            registry: self.registry,
            excluded: before.difference(&after).cloned().collect(),
        })
    }
}

#[path = "overlay_version_admission_tests.rs"]
mod tests;

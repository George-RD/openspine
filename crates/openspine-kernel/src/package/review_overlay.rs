//! Pure overlay consequences for a captured base transition. No activation.
use serde::Serialize;

use super::current_state::{ArtifactVersion, CapturedCurrentState};
use crate::artifact_loader::ArtifactRegistry;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct OverlayConsequence {
    pub kind: String,
    pub artifact_id: String,
    pub version: u32,
    pub reason: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct OverlayViewAssessment {
    pub effective_artifacts: Vec<ArtifactVersion>,
    pub consequences: Vec<OverlayConsequence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct OverlayAssessment {
    pub current_base_epoch: String,
    pub candidate_base_epoch: String,
    /// Binds overlay sources, learned provenance and captured proposal controls.
    /// Other durable authority/work controls belong to the Store-owned census.
    pub control_fingerprint: String,
    pub before: OverlayViewAssessment,
    pub after: OverlayViewAssessment,
    pub blockers: Vec<OverlayConsequence>,
    pub reusable_authority_reconfirmation_required: bool,
}

pub(crate) fn assess(
    _current: &CapturedCurrentState,
    _candidate: &ArtifactRegistry,
) -> anyhow::Result<OverlayAssessment> {
    anyhow::bail!("captured overlay transition assessment is not implemented")
}

#[cfg(test)]
#[path = "review_overlay_tests.rs"]
mod tests;

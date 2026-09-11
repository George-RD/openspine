//! Captured configured-state inputs for package transition review (#285).
//!
//! This module is intentionally review-only. It must not select, approve,
//! recover, republish, reconfirm or activate anything. Callers are responsible
//! for holding the ordinary package-maintenance lifetime lock while capture
//! runs; owned results must remain stable after that lock is released.
#![allow(dead_code)] // staged #285 precursor; production wiring follows after the capture contract is green

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use openspine_schemas::artifact::Lifecycle;

use super::install_types::PackageIdentity;
use super::InspectionError;
use crate::artifact_loader::ArtifactRegistry;
use crate::artifact_store::ArtifactStore;
use crate::overlay_persona_admission::PersonaProvenanceFindings;
use crate::store::learned_artifacts::LearnedArtifact;
use crate::store::Store;

pub(crate) type ArtifactVersion = (String, String, u32);

pub(crate) struct CapturedCurrentBase {
    pub configured_path: PathBuf,
    pub identity: PackageIdentity,
    pub registry: ArtifactRegistry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CapturedOverlayControl {
    pub lifecycle: Option<Lifecycle>,
    pub highest_active_version: Option<u32>,
    pub source_present: bool,
    pub recoverable_blob_present: bool,
}

pub(crate) struct CapturedOverlayState {
    pub registry: ArtifactRegistry,
    pub learned: Vec<LearnedArtifact>,
    pub controls: BTreeMap<ArtifactVersion, CapturedOverlayControl>,
    pub persona_findings: PersonaProvenanceFindings,
}

pub(crate) struct CapturedCurrentState {
    pub base: CapturedCurrentBase,
    pub base_artifact_ids: HashSet<(String, String)>,
    pub base_compatibility_epoch: String,
    pub overlay: CapturedOverlayState,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CurrentStateCaptureError {
    #[error("configured current base could not be captured")]
    Base(#[from] InspectionError),
    #[error("configured current base has a different package identity")]
    PackageIdMismatch,
    #[error("configured overlay state could not be captured")]
    Overlay,
    #[error("configured overlay controls could not be captured")]
    Control,
    #[error("configured current-state capture is not implemented")]
    NotImplemented,
}

impl CapturedCurrentState {
    pub(crate) fn capture(
        _configured_base: &Path,
        _expected_package_id: &str,
        _data_root: &Path,
        _store: &Store,
        _artifacts: &ArtifactStore,
    ) -> Result<Self, CurrentStateCaptureError> {
        Err(CurrentStateCaptureError::NotImplemented)
    }
}

#[cfg(test)]
#[path = "current_state_tests.rs"]
mod tests;

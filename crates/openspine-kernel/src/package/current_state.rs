//! Captured configured-state inputs for package transition review (#285).
//!
//! This module is intentionally review-only. It must not select, approve,
//! recover, republish, reconfirm or activate anything. Callers are responsible
//! for holding the ordinary package-maintenance lifetime lock while capture
//! runs; owned results must remain stable after that lock is released.
#![allow(dead_code)] // staged #285 precursor; production wiring follows after the capture contract is green

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use openspine_schemas::artifact::{ArtifactRef, Lifecycle};
use openspine_schemas::digest::Digest;

use super::install_types::PackageIdentity;
use super::InspectionError;
use crate::artifact_loader::{self, ArtifactRegistry};
use crate::artifact_store::ArtifactStore;
use crate::counterparty_keys::SYSTEM_SCOPE;
use crate::overlay_compat;
use crate::overlay_persona_admission::{CapturedPersonaProvenance, PersonaProvenanceFindings};
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
}

impl CapturedCurrentState {
    pub(crate) fn capture(
        configured_base: &Path,
        expected_package_id: &str,
        data_root: &Path,
        store: &Store,
        artifacts: &ArtifactStore,
    ) -> Result<Self, CurrentStateCaptureError> {
        // Capture and validate the configured base once. The typed registry is
        // retained from those exact bounded bytes; it is never reconstructed
        // from the live path after validation.
        let snapshot = super::inspect(configured_base)?;
        let identity = snapshot.identity();
        if identity.package_id != expected_package_id {
            return Err(CurrentStateCaptureError::PackageIdMismatch);
        }
        let base_registry = snapshot.registry().clone();
        let base_artifact_ids = artifact_loader::artifact_identity_pairs(&base_registry);
        let base_compatibility_epoch =
            overlay_compat::compatibility_epoch(&base_registry, &base_artifact_ids);

        let overlay_dir = data_root.join("artifacts.d");
        let mut overlay_registry = match std::fs::symlink_metadata(&overlay_dir) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                artifact_loader::load_registry_without_personas(&overlay_dir)
                    .map_err(|_| CurrentStateCaptureError::Overlay)?
            }
            Ok(_) => return Err(CurrentStateCaptureError::Overlay),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ArtifactRegistry::default()
            }
            Err(_) => return Err(CurrentStateCaptureError::Overlay),
        };

        let learned = store
            .list_learned_artifacts()
            .map_err(|_| CurrentStateCaptureError::Control)?;
        let persona_findings =
            CapturedPersonaProvenance::capture_for_review(store, artifacts, &learned)
                .map_err(|_| CurrentStateCaptureError::Control)?
                .evaluate();
        let admitted_personas: HashMap<(String, u32), String> = persona_findings
            .expected_digests
            .iter()
            .map(|(key, digest)| (key.clone(), digest.clone()))
            .collect();
        artifact_loader::load_admitted_personas(
            &mut overlay_registry,
            &overlay_dir,
            &admitted_personas,
        )
        .map_err(|_| CurrentStateCaptureError::Overlay)?;
        overlay_compat::exclude_erased(&mut overlay_registry, &learned);

        let controls = capture_overlay_controls(store, artifacts, &overlay_registry, &learned)?;

        Ok(Self {
            base: CapturedCurrentBase {
                configured_path: configured_base.to_path_buf(),
                identity,
                registry: base_registry,
            },
            base_artifact_ids,
            base_compatibility_epoch,
            overlay: CapturedOverlayState {
                registry: overlay_registry,
                learned,
                controls,
                persona_findings,
            },
        })
    }
}

fn capture_overlay_controls(
    store: &Store,
    artifacts: &ArtifactStore,
    registry: &ArtifactRegistry,
    learned: &[LearnedArtifact],
) -> Result<BTreeMap<ArtifactVersion, CapturedOverlayControl>, CurrentStateCaptureError> {
    // This captures every exact version represented by durable learned state or
    // the effective overlay source set, plus the durable highest Active version
    // for each such identity. The Store-owned exhaustive proposal/work census is
    // a separate #285 phase and must remain an explicit readiness input.
    let mut versions = BTreeSet::new();
    let mut identities = BTreeSet::new();
    for row in learned {
        let identity = (row.kind.clone(), row.artifact_id.clone());
        identities.insert(identity.clone());
        versions.insert((identity.0, identity.1, row.version));
    }
    for (kind, id, version) in registry.sources.keys() {
        identities.insert((kind.clone(), id.clone()));
        versions.insert((kind.clone(), id.clone(), *version));
    }

    let mut highest_active = BTreeMap::new();
    for (kind, id) in identities {
        let highest = store
            .highest_active_version(&kind, &id)
            .map_err(|_| CurrentStateCaptureError::Control)?;
        if let Some(version) = highest {
            versions.insert((kind.clone(), id.clone(), version));
        }
        highest_active.insert((kind, id), highest);
    }

    let learned_by_version: BTreeMap<ArtifactVersion, &LearnedArtifact> = learned
        .iter()
        .map(|row| {
            (
                (row.kind.clone(), row.artifact_id.clone(), row.version),
                row,
            )
        })
        .collect();

    let mut controls = BTreeMap::new();
    for (kind, id, version) in versions {
        let proposal = store
            .find_proposed_artifact_state(&kind, &id, version)
            .map_err(|_| CurrentStateCaptureError::Control)?;
        let lifecycle = proposal.as_ref().map(|(state, _)| *state);
        let reviewed_digest = proposal
            .as_ref()
            .map(|(_, digest)| digest.as_str())
            .or_else(|| {
                learned_by_version
                    .get(&(kind.clone(), id.clone(), version))
                    .and_then(|row| row.pending_yaml_digest.as_deref())
            });
        let source_present = registry
            .sources
            .contains_key(&(kind.clone(), id.clone(), version));
        let recoverable_blob_present = reviewed_digest.is_some_and(|digest| {
            Digest::parse(digest.to_owned()).is_ok_and(|digest| {
                artifacts
                    .get_scoped_without_recovery(
                        SYSTEM_SCOPE,
                        &ArtifactRef {
                            digest,
                            schema_version: 1,
                        },
                    )
                    .is_ok()
            })
        });
        controls.insert(
            (kind.clone(), id.clone(), version),
            CapturedOverlayControl {
                lifecycle,
                highest_active_version: highest_active.get(&(kind, id)).copied().flatten(),
                source_present,
                recoverable_blob_present,
            },
        );
    }
    Ok(controls)
}

#[cfg(test)]
#[path = "current_state_tests.rs"]
mod tests;

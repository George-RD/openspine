//! Admission findings over captured overlay inputs (#285).
//!
//! This phase covers namespace collisions, approved-source integrity and
//! missing provenance only. Capture, erasure/persona admission, highest-active
//! version resolution and dependency convergence remain caller obligations.
//! It does not establish compatibility, quiescence, approval or activation.
use crate::artifact_loader::{self, ArtifactRegistry};
use crate::overlay_compat::{missing_provenance, OrphanedArtifact};
use crate::store::learned_artifacts::{CompatibilityStatus, LearnedArtifact};
use std::collections::HashSet;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AdmissionFindings {
    pub collisions: Vec<(String, String)>,
    pub collision_orphans: Vec<OrphanedArtifact>,
    pub digest_invalid: Vec<OrphanedArtifact>,
    pub missing: Vec<OrphanedArtifact>,
}

/// Evaluate without reopening sources or mutating the supplied inputs.
/// The caller supplies the overlay registry before merging with the base.
/// It must come from the same validated capture as its source bytes.
/// Source paths and learned-row paths are not trusted byte fallbacks.
pub(crate) fn evaluate(
    overlay: &ArtifactRegistry,
    learned: &[LearnedArtifact],
    base_ids: &HashSet<(String, String)>,
) -> AdmissionFindings {
    let overlay_ids = artifact_loader::artifact_identity_pairs(overlay);
    let mut collisions: Vec<_> = overlay_ids.intersection(base_ids).cloned().collect();
    collisions.sort();
    let mut collision_orphans: Vec<_> = collisions
        .iter()
        .filter_map(|(kind, artifact_id)| {
            let version = artifact_loader::artifact_version(overlay, kind, artifact_id)?;
            learned
                .iter()
                .find(|item| {
                    item.kind == *kind
                        && item.artifact_id == *artifact_id
                        && item.version == version
                })
                .map(|_| OrphanedArtifact {
                    kind: kind.clone(),
                    artifact_id: artifact_id.clone(),
                    version,
                    dangling_references: vec!["base_overlay_collision".into()],
                })
        })
        .collect();
    let mut digest_invalid: Vec<_> = learned
        .iter()
        .filter(|item| {
            matches!(
                item.compatibility,
                CompatibilityStatus::Compatible | CompatibilityStatus::OwnerAccepted
            ) && artifact_loader::artifact_version(overlay, &item.kind, &item.artifact_id)
                == Some(item.version)
        })
        .filter_map(|item| {
            let Some(source) = overlay.sources.get(&(
                item.kind.clone(),
                item.artifact_id.clone(),
                item.version,
            )) else {
                return Some(finding(item, "approved_overlay_source_missing"));
            };
            let Some(expected) = item.pending_yaml_digest.as_deref() else {
                return Some(finding(item, "approved_overlay_digest_missing"));
            };
            let actual = openspine_schemas::digest::digest_of_bytes(&source.bytes);
            (expected != actual.as_str()).then(|| finding(item, "approved_overlay_digest_mismatch"))
        })
        .collect();
    // Match startup's existing order without cloning the entire registry:
    // integrity exclusion precedes provenance discovery; collisions stay until
    // legacy recovery. Filter the same identities that startup will exclude.
    let invalid_ids: HashSet<_> = digest_invalid
        .iter()
        .map(|item| (item.kind.clone(), item.artifact_id.clone()))
        .collect();
    let mut missing: Vec<_> = missing_provenance(overlay, learned)
        .into_iter()
        .filter(|item| !invalid_ids.contains(&(item.kind.clone(), item.artifact_id.clone())))
        .collect();
    // Canonical order is independent of registry/learned-row enumeration.
    // Keep duplicate evidence rather than concealing damaged capture inputs.
    for findings in [&mut collision_orphans, &mut digest_invalid, &mut missing] {
        findings.sort_by(|a, b| {
            (&a.kind, &a.artifact_id, a.version, &a.dangling_references).cmp(&(
                &b.kind,
                &b.artifact_id,
                b.version,
                &b.dangling_references,
            ))
        });
    }
    AdmissionFindings {
        collisions,
        collision_orphans,
        digest_invalid,
        missing,
    }
}

fn finding(item: &LearnedArtifact, reason: &str) -> OrphanedArtifact {
    OrphanedArtifact {
        kind: item.kind.clone(),
        artifact_id: item.artifact_id.clone(),
        version: item.version,
        dangling_references: vec![reason.into()],
    }
}

#[cfg(test)]
#[path = "overlay_admission_startup_tests.rs"]
mod startup_tests;
#[cfg(test)]
#[path = "overlay_admission_tests.rs"]
mod tests;

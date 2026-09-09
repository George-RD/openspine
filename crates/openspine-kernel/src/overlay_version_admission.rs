//! Shared version-admission phase for startup and captured-state review (#285).
//!
//! This is not complete overlay admission or a protected filesystem capture.
//! The supplied typed registry and raw sources must already be consistent.
//! Erasure, producing-event admission, approved digests, missing-state census,
//! collisions and dependency convergence remain separate caller obligations.
//! No result here proves compatibility, quiescence or permission to activate.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use openspine_schemas::digest::digest_of_bytes;

use crate::artifact_loader::{self, ArtifactRegistry};
use crate::store::learned_artifacts::{LearnedArtifact, Provenance};
use crate::store::Store;

type ArtifactVersion = (String, String, u32);

/// Owned inputs to one version-pruning phase. No Store reference or mutable
/// access escapes; later activation/provenance writes cannot change evaluation.
pub(crate) struct CapturedVersionAdmission {
    registry: ArtifactRegistry,
    learned: Vec<LearnedArtifact>,
    highest_active: BTreeMap<(String, String), Option<u32>>,
}

/// Version-phase output only, with canonical exact-version exclusions. A
/// missing DB-highest source is not recovered, nor certified safe by omission.
pub(crate) struct AdmittedOverlay {
    pub(crate) registry: ArtifactRegistry,
    pub(crate) excluded: BTreeSet<ArtifactVersion>,
}

impl CapturedVersionAdmission {
    /// Retain source bytes and the controls consulted by startup's existing
    /// pruning rules. Capture must run before runtime writers start, or under
    /// a maintenance lifetime lock with all writers excluded. These individual
    /// Store reads are not a transaction against arbitrary concurrent writers.
    /// Only identities represented in supplied sources are captured; a full
    /// reviewer must separately account for missing sources and damaged state.
    pub(crate) fn capture(registry: &ArtifactRegistry, store: &Store) -> anyhow::Result<Self> {
        let registry = registry.clone();
        let learned = store.list_learned_artifacts()?;
        let identities: BTreeSet<_> = registry
            .sources
            .keys()
            .filter(|(kind, _, _)| kind != "persona")
            .map(|(kind, id, _)| (kind.clone(), id.clone()))
            .collect();
        let mut highest_active = BTreeMap::new();
        for (kind, id) in identities {
            let highest = store.highest_active_version(&kind, &id)?;
            // Keep absence explicit: a later activation cannot fill it in.
            highest_active.insert((kind, id), highest);
        }
        Ok(Self {
            registry,
            learned,
            highest_active,
        })
    }

    /// Evaluate only owned captured inputs. No Store access, filesystem reads,
    /// recovery, audit writes or approval identifiers are involved. Retained
    /// paths remain provenance metadata; rehydration parses retained bytes.
    pub(crate) fn evaluate(self) -> anyhow::Result<AdmittedOverlay> {
        let Self {
            mut registry,
            learned,
            highest_active,
        } = self;
        let before: BTreeSet<_> = registry.sources.keys().cloned().collect();
        let eligible_personas: HashSet<(String, u32)> = registry
            .sources
            .iter()
            .filter_map(|((kind, id, version), source)| {
                if kind != "persona" {
                    return None;
                }
                let digest = digest_of_bytes(&source.bytes);
                learned
                    .iter()
                    .any(|row| {
                        row.kind == "persona"
                            && row.artifact_id == *id
                            && row.version == *version
                            && matches!(&row.provenance, Provenance::ProducedBy { .. })
                            && row.pending_yaml_digest.as_deref() == Some(digest.as_str())
                    })
                    .then_some((id.clone(), *version))
            })
            .collect();
        // Personas have row/digest backing, not a proposal lifecycle. Reuse
        // the loader's exact-version pruning and retained-byte rehydration.
        artifact_loader::exclude_unbacked_persona_versions(&mut registry, &eligible_personas)?;

        for ((kind, id), highest) in highest_active {
            let loaded: Vec<u32> = registry
                .sources
                .keys()
                .filter(|(k, artifact_id, _)| k == &kind && artifact_id == &id)
                .map(|(_, _, version)| *version)
                .collect();
            for version in loaded {
                if Some(version) != highest {
                    super::remove_loaded_version(&mut registry, &kind, &id, version);
                }
            }
            if let Some(highest) = highest {
                if artifact_loader::artifact_version(&registry, &kind, &id) != Some(highest) {
                    if let Some(source) = registry
                        .sources
                        .get(&(kind.clone(), id.clone(), highest))
                        .cloned()
                    {
                        artifact_loader::rehydrate_source(&mut registry, &kind, &source)?;
                    }
                }
            }
        }
        let after: BTreeSet<_> = registry.sources.keys().cloned().collect();
        Ok(AdmittedOverlay {
            registry,
            excluded: before.difference(&after).cloned().collect(),
        })
    }
}

#[cfg(test)]
#[path = "overlay_version_admission_tests.rs"]
mod tests;

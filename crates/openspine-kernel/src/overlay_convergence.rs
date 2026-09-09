//! Dependency evaluation over an already-admitted, in-memory registry.
//!
//! This is not a complete package compatibility check. Callers must establish
//! trusted capture, provenance, erasure and version admission first. Evaluation
//! changes only the supplied scratch registry; it does not approve or activate
//! anything, establish quiescence, or make its untrusted labels safe to render.
use super::{
    exclude_orphans, find_orphans, owner_accepted_newly_dangling, registry_entry_active_at,
    ArtifactRegistry, CompatibilityStatus, LearnedArtifact, OrphanedArtifact,
};
use crate::store::learned_artifacts::dependency_fingerprint_allows;
use openspine_schemas::digest::digest_of_bytes;
use std::collections::{BTreeMap, HashSet};

/// Exact-version bytes supplied by the caller, never a path to reopen.
pub(crate) type CapturedOwnerAcceptedSources = BTreeMap<(String, String, u32), Vec<u8>>;

/// Converge ordinary and owner-accepted dependencies using only captured state.
///
/// Returns canonical `(ordinary_orphans, owner_accepted_invalid)` consequences.
/// Previously accepted dangling references survive; new dangling references or
/// absent/mismatched reviewed bytes invalidate the owner-accepted artifact.
/// Exact-version exclusion never removes a colliding base artifact. The caller
/// must have excluded `existing_invalid` from the scratch registry already,
/// except where doing so would remove a base collision. Missing captured bytes
/// never fall back to registry sources or learned-row paths.
pub(crate) fn evaluate_captured_dependencies(
    registry: &mut ArtifactRegistry,
    learned: &[LearnedArtifact],
    base_ids: &HashSet<(String, String)>,
    existing_invalid: &[OrphanedArtifact],
    sources: &CapturedOwnerAcceptedSources,
) -> (Vec<OrphanedArtifact>, Vec<OrphanedArtifact>) {
    let ordinary_candidates: Vec<_> = learned
        .iter()
        .filter(|item| !base_ids.contains(&(item.kind.clone(), item.artifact_id.clone())))
        .cloned()
        .collect();
    let mut ordinary = evaluate_compatibility(registry, &ordinary_candidates);
    let mut invalid = existing_invalid.to_vec();
    loop {
        let mut newly_invalid = Vec::new();
        for item in learned.iter().filter(|item| {
            item.compatibility == CompatibilityStatus::OwnerAccepted
                && !invalid.iter().any(|orphan| {
                    orphan.kind == item.kind
                        && orphan.artifact_id == item.artifact_id
                        && orphan.version == item.version
                })
        }) {
            let source_bytes =
                sources.get(&(item.kind.clone(), item.artifact_id.clone(), item.version));
            let tampered = match (&item.pending_yaml_digest, source_bytes) {
                (Some(recorded), Some(bytes)) => recorded != digest_of_bytes(bytes).as_str(),
                _ => true,
            };
            let current: Vec<String> = source_bytes
                .map(|bytes| {
                    owner_accepted_newly_dangling(registry, &item.kind, Some(bytes))
                        .into_iter()
                        .filter(|reference| reference != "owner_accepted_source_missing")
                        .collect()
                })
                .unwrap_or_default();
            let newly_dangling = !dependency_fingerprint_allows(
                &current,
                item.accepted_dependency_fingerprint.as_deref(),
            );
            if tampered || newly_dangling {
                newly_invalid.push(OrphanedArtifact {
                    kind: item.kind.clone(),
                    artifact_id: item.artifact_id.clone(),
                    version: item.version,
                    dangling_references: if tampered {
                        vec!["owner_accepted_digest_tampered".into()]
                    } else {
                        current
                    },
                });
            }
        }
        if newly_invalid.is_empty() {
            break;
        }
        // Alternate passes to a fixed point: removing an owner-accepted
        // dependency can invalidate ordinary dependents and vice versa.
        let registry_invalid: Vec<_> = newly_invalid
            .iter()
            .filter(|orphan| !base_ids.contains(&(orphan.kind.clone(), orphan.artifact_id.clone())))
            .cloned()
            .collect();
        exclude_orphans(registry, &registry_invalid);
        invalid.extend(newly_invalid);
        for orphan in evaluate_compatibility(registry, &ordinary_candidates) {
            if !ordinary.iter().any(|old| {
                old.kind == orphan.kind
                    && old.artifact_id == orphan.artifact_id
                    && old.version == orphan.version
            }) {
                ordinary.push(orphan);
            }
        }
    }
    canonicalize_orphans(&mut ordinary);
    canonicalize_orphans(&mut invalid);
    (ordinary, invalid)
}

pub(super) fn evaluate_compatibility(
    registry: &mut ArtifactRegistry,
    learned: &[LearnedArtifact],
) -> Vec<OrphanedArtifact> {
    let mut all = Vec::new();
    let pending: Vec<_> = learned
        .iter()
        .filter(|item| {
            item.compatibility == CompatibilityStatus::ReconfirmationRequired
                && registry_entry_active_at(registry, &item.kind, &item.artifact_id, item.version)
        })
        .map(|item| OrphanedArtifact {
            kind: item.kind.clone(),
            artifact_id: item.artifact_id.clone(),
            version: item.version,
            dangling_references: vec!["reconfirmation_required".into()],
        })
        .collect();
    exclude_orphans(registry, &pending);
    all.extend(pending);
    loop {
        let next = find_orphans(registry, learned)
            .into_iter()
            .filter(|candidate| {
                // Never re-orphan a durably owner-accepted artifact; the owner's
                // single tap endures even with dangling references (AD-070).
                let owner_accepted = learned.iter().any(|item| {
                    item.kind == candidate.kind
                        && item.artifact_id == candidate.artifact_id
                        && item.version == candidate.version
                        && item.compatibility == CompatibilityStatus::OwnerAccepted
                });
                if owner_accepted {
                    return false;
                }
                // Version cutover: a stale learned row for a superseded version
                // must not exclude the active higher version.
                if !registry_entry_active_at(
                    registry,
                    &candidate.kind,
                    &candidate.artifact_id,
                    candidate.version,
                ) {
                    return false;
                }
                !all.iter().any(|existing| {
                    existing.kind == candidate.kind
                        && existing.artifact_id == candidate.artifact_id
                        && existing.version == candidate.version
                })
            })
            .collect::<Vec<_>>();
        if next.is_empty() {
            break;
        }
        exclude_orphans(registry, &next);
        all.extend(next);
    }
    canonicalize_orphans(&mut all);
    all
}

fn canonicalize_orphans(orphans: &mut [OrphanedArtifact]) {
    for orphan in orphans.iter_mut() {
        orphan.dangling_references.sort();
    }
    // Preserve all facts, including duplicates, rather than hide damaged input.
    orphans.sort_by(|a, b| {
        (&a.kind, &a.artifact_id, a.version, &a.dangling_references).cmp(&(
            &b.kind,
            &b.artifact_id,
            b.version,
            &b.dangling_references,
        ))
    });
}

#[cfg(test)]
#[path = "overlay_convergence_tests.rs"]
mod tests;

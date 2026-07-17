use super::{
    apply_compatibility, exclude_orphans, owner_accepted_newly_dangling, ArtifactRegistry,
    CompatibilityStatus, LearnedArtifact, OrphanedArtifact,
};
use crate::store::learned_artifacts::dependency_fingerprint_allows;
use openspine_schemas::digest::digest_of_bytes;
use std::collections::HashSet;
use ulid::Ulid;

/// Converge ordinary and owner-accepted compatibility to a fixed point.
///
/// Returns `(ordinary_orphans, review_ids, owner_accepted_invalid)`. Ordinary
/// candidates drive `apply_compatibility`; owner-accepted artifacts are then
/// revalidated against the current registry. An owner-accepted artifact is
/// invalidated only when its reviewed YAML has been tampered (digest no longer
/// matches the recorded `pending_yaml_digest`) or when its *current* dangling
/// reference set is not a subset of the durably-accepted
/// `accepted_dependency_fingerprint` — so pre-existing accepted dangling refs
/// survive an unrelated restart, while newly-dangling cross-kind dependencies
/// are excluded and re-prompted. Base/overlay identity collisions are never
/// removed from the registry.
pub fn converge_owner_accepted_dependencies(
    registry: &mut ArtifactRegistry,
    learned: &[LearnedArtifact],
    base_ids: &HashSet<(String, String)>,
    existing_invalid: &[OrphanedArtifact],
) -> (Vec<OrphanedArtifact>, Vec<Ulid>, Vec<OrphanedArtifact>) {
    let ordinary_candidates: Vec<_> = learned
        .iter()
        .filter(|item| !base_ids.contains(&(item.kind.clone(), item.artifact_id.clone())))
        .cloned()
        .collect();
    let (mut ordinary, mut requests) = apply_compatibility(registry, &ordinary_candidates);
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
            let source_bytes = item
                .source_path
                .as_deref()
                .and_then(|path| std::fs::read(path).ok())
                .or_else(|| {
                    registry
                        .sources
                        .get(&(item.kind.clone(), item.artifact_id.clone(), item.version))
                        .map(|source| source.bytes.clone())
                });
            // A tampered reviewed YAML must never become effective: only an
            // exact recorded-digest match against the on-disk bytes may
            // proceed. Missing digest or missing source is treated as invalid.
            let tampered = match (&item.pending_yaml_digest, &source_bytes) {
                (Some(recorded), Some(bytes)) => recorded != digest_of_bytes(bytes).as_str(),
                _ => true,
            };
            let current: Vec<String> = source_bytes
                .as_deref()
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
                let refs = if tampered {
                    vec!["owner_accepted_digest_tampered".into()]
                } else {
                    current.clone()
                };
                newly_invalid.push(OrphanedArtifact {
                    kind: item.kind.clone(),
                    artifact_id: item.artifact_id.clone(),
                    version: item.version,
                    dangling_references: refs,
                });
            }
        }
        if newly_invalid.is_empty() {
            break;
        }
        // Exclude only non-collision owner-accepted artifacts, then re-run the
        // ordinary pass so ordinary dependents invalidated by the new removal
        // are caught in the next iteration — alternating to a fixed point.
        let registry_invalid: Vec<_> = newly_invalid
            .iter()
            .filter(|orphan| !base_ids.contains(&(orphan.kind.clone(), orphan.artifact_id.clone())))
            .cloned()
            .collect();
        exclude_orphans(registry, &registry_invalid);
        invalid.extend(newly_invalid);
        let (next, ids) = apply_compatibility(registry, &ordinary_candidates);
        for (orphan, id) in next.into_iter().zip(ids) {
            if !ordinary.iter().any(|old| {
                old.kind == orphan.kind
                    && old.artifact_id == orphan.artifact_id
                    && old.version == orphan.version
            }) {
                ordinary.push(orphan);
                requests.push(id);
            }
        }
    }
    (ordinary, requests, invalid)
}

//! Legacy startup adapter: capture files once, then allocate review IDs only
//! after the shared dependency evaluator has finished. Not a package preview API.
use super::overlay_convergence::{evaluate_captured_dependencies, CapturedOwnerAcceptedSources};
use super::{ArtifactRegistry, CompatibilityStatus, LearnedArtifact, OrphanedArtifact};
use std::collections::HashSet;
use ulid::Ulid;

/// Preserve legacy startup's path-first, registry-source fallback at capture.
///
/// The dependency rules are shared with captured-state review. Files are read
/// once before convergence, not again on each pass. A package reviewer must
/// supply its own verified capture directly to the pure evaluator instead of
/// using this adapter's legacy filesystem fallback or review-ID allocation.
pub fn converge_owner_accepted_dependencies(
    registry: &mut ArtifactRegistry,
    learned: &[LearnedArtifact],
    base_ids: &HashSet<(String, String)>,
    existing_invalid: &[OrphanedArtifact],
) -> (Vec<OrphanedArtifact>, Vec<Ulid>, Vec<OrphanedArtifact>) {
    let mut sources = CapturedOwnerAcceptedSources::new();
    for item in learned.iter().filter(|item| {
        item.compatibility == CompatibilityStatus::OwnerAccepted
            && !existing_invalid.iter().any(|orphan| {
                orphan.kind == item.kind
                    && orphan.artifact_id == item.artifact_id
                    && orphan.version == item.version
            })
    }) {
        let key = (item.kind.clone(), item.artifact_id.clone(), item.version);
        let bytes = item
            .source_path
            .as_deref()
            .and_then(|path| std::fs::read(path).ok())
            .or_else(|| {
                registry
                    .sources
                    .get(&key)
                    .map(|source| source.bytes.clone())
            });
        if let Some(bytes) = bytes {
            sources.insert(key, bytes);
        }
    }
    let (ordinary, invalid) =
        evaluate_captured_dependencies(registry, learned, base_ids, existing_invalid, &sources);
    let requests = reconfirmation_ids(&ordinary, learned);
    (ordinary, requests, invalid)
}

pub(crate) fn reconfirmation_ids(
    orphans: &[OrphanedArtifact],
    learned: &[LearnedArtifact],
) -> Vec<Ulid> {
    orphans
        .iter()
        .map(|item| {
            learned
                .iter()
                .find(|candidate| {
                    candidate.kind == item.kind
                        && candidate.artifact_id == item.artifact_id
                        && candidate.version == item.version
                })
                .and_then(|candidate| candidate.pending_reconfirmation_id)
                .unwrap_or_else(Ulid::new)
        })
        .collect()
}

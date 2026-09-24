//! Deterministic typed runtime-input comparison for package review.
use openspine_schemas::digest::Digest;
use serde::Serialize;
use serde_json::Value;

use crate::artifact_loader::ArtifactRegistry;

pub(crate) const MAX_DETAIL_BYTES: usize = 64 * 1024;

#[derive(Debug, Serialize)]
pub(crate) struct SemanticReview {
    pub before_runtime_digest: Digest,
    pub after_runtime_digest: Digest,
    pub changes: Vec<ArtifactChange>,
    pub action_descriptors: Value,
    pub blockers: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ArtifactChange {
    pub kind: String,
    pub id: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

pub(crate) fn compare(_before: &ArtifactRegistry, _after: &ArtifactRegistry) -> SemanticReview {
    todo!("TDD: implement typed semantic comparison")
}

#[cfg(test)]
#[path = "review_semantics_tests.rs"]
mod tests;

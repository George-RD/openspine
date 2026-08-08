//! Immutable review derivation for Narrow decisions.

use ulid::Ulid;

use crate::digest::digest_of;
use crate::reviewed_scope::ReviewedActionScope;

use super::OwnerReviewRequest;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OwnerReviewNarrowError {
    #[error("narrowed scope must remain valid")]
    InvalidScope,
    #[error("replacement scope is not a strict narrowing of the reviewed scope")]
    NotNarrower,
    #[error("narrowed scope must preserve the reviewed action")]
    ActionChanged,
    #[error("narrowed scope must preserve the descriptor version")]
    DescriptorChanged,
    #[error("narrowed review must have a new id")]
    ReusedId,
}

impl OwnerReviewRequest {
    /// Derive a new immutable review while preserving every untouched field.
    pub fn narrowed_review(
        &self,
        new_id: Ulid,
        narrowed_scope: ReviewedActionScope,
    ) -> Result<Self, OwnerReviewNarrowError> {
        if new_id == self.id {
            return Err(OwnerReviewNarrowError::ReusedId);
        }
        if !narrowed_scope.binding_is_valid() {
            return Err(OwnerReviewNarrowError::InvalidScope);
        }
        if !self.reviewed_scope.is_strict_narrowing(&narrowed_scope) {
            return Err(OwnerReviewNarrowError::NotNarrower);
        }
        if narrowed_scope.action_id() != self.reviewed_scope.action_id() {
            return Err(OwnerReviewNarrowError::ActionChanged);
        }
        if narrowed_scope.descriptor_version() != self.reviewed_scope.descriptor_version() {
            return Err(OwnerReviewNarrowError::DescriptorChanged);
        }
        let mut narrowed = self.clone();
        narrowed.id = new_id;
        narrowed.review_version = self.review_version.saturating_add(1);
        narrowed.reviewed_scope = narrowed_scope;
        narrowed.binding_digest = digest_of(&serde_json::Value::Null);
        narrowed.binding_digest = narrowed.calculate_binding_digest();
        debug_assert!(narrowed.binding_is_valid());
        Ok(narrowed)
    }
}

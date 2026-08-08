//! Containment-preserving edits for immutable reviewed scopes.

use std::collections::{BTreeMap, BTreeSet};

use crate::action::ReviewedScopeDimension;

use super::{ReviewedActionScope, ReviewedScopeValue};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReviewedScopeNarrowError {
    #[error("narrowed scope must preserve every reviewed dimension")]
    DimensionSetChanged,
    #[error("dimension {dimension:?} was widened or changed incompatibly")]
    Widened { dimension: ReviewedScopeDimension },
    #[error("narrowing must tighten at least one dimension")]
    Unchanged,
}

impl ReviewedActionScope {
    /// Produce a new valid scope from a complete replacement value map.
    /// Scalar dimensions are immutable; set/map dimensions may only become
    /// strict subsets. Untouched values remain byte-identical.
    pub fn narrowed(
        &self,
        dimensions: BTreeMap<ReviewedScopeDimension, ReviewedScopeValue>,
    ) -> Result<Self, ReviewedScopeNarrowError> {
        if dimensions.keys().collect::<BTreeSet<_>>()
            != self.dimensions.keys().collect::<BTreeSet<_>>()
        {
            return Err(ReviewedScopeNarrowError::DimensionSetChanged);
        }
        let mut changed = false;
        for (dimension, original) in &self.dimensions {
            let candidate = dimensions
                .get(dimension)
                .expect("dimension sets were proven equal");
            if !narrower_or_equal(candidate, original) {
                return Err(ReviewedScopeNarrowError::Widened {
                    dimension: *dimension,
                });
            }
            changed |= candidate != original;
        }
        if !changed {
            return Err(ReviewedScopeNarrowError::Unchanged);
        }
        let mut narrowed = Self {
            schema_version: self.schema_version,
            scope_version: self.scope_version + 1,
            action_id: self.action_id.clone(),
            descriptor_version: self.descriptor_version,
            dimensions,
            context_class_digest: self.context_class_digest.clone(),
        };
        narrowed.context_class_digest = narrowed.calculate_context_class_digest();
        debug_assert!(narrowed.binding_is_valid());
        Ok(narrowed)
    }

    pub fn is_strict_narrowing(&self, candidate: &Self) -> bool {
        self.narrowed(candidate.dimensions.clone())
            .is_ok_and(|expected| expected == *candidate)
    }
}

fn narrower_or_equal(candidate: &ReviewedScopeValue, original: &ReviewedScopeValue) -> bool {
    match (candidate, original) {
        (ReviewedScopeValue::Target(candidate), ReviewedScopeValue::Target(original)) => candidate
            .refs
            .iter()
            .all(|value| original.refs.contains(value)),
        (
            ReviewedScopeValue::OutputChannels(candidate),
            ReviewedScopeValue::OutputChannels(original),
        ) => candidate.is_subset(original),
        (
            ReviewedScopeValue::BoundParameters(candidate),
            ReviewedScopeValue::BoundParameters(original),
        ) => candidate == original,
        _ => candidate == original,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::ActionId;
    use crate::digest::Digest;

    #[test]
    fn deleting_a_bound_parameter_is_not_narrowing() {
        let mut dimensions = BTreeMap::new();
        dimensions.insert(
            ReviewedScopeDimension::BoundParameters,
            ReviewedScopeValue::BoundParameters(BTreeMap::from([
                ("body".into(), "fixed-body".into()),
                ("subject".into(), "fixed-subject".into()),
            ])),
        );
        let mut original = ReviewedActionScope {
            schema_version: 1,
            scope_version: 1,
            action_id: ActionId::new("test.action"),
            descriptor_version: 1,
            dimensions: dimensions.clone(),
            context_class_digest: Digest::parse(format!("sha256:{}", "0".repeat(64))).unwrap(),
        };
        original.context_class_digest = original.calculate_context_class_digest();
        let ReviewedScopeValue::BoundParameters(parameters) = dimensions
            .get_mut(&ReviewedScopeDimension::BoundParameters)
            .unwrap()
        else {
            unreachable!();
        };
        parameters.remove("body");
        assert!(matches!(
            original.narrowed(dimensions),
            Err(ReviewedScopeNarrowError::Widened {
                dimension: ReviewedScopeDimension::BoundParameters
            })
        ));
    }

    /// `is_strict_narrowing` is the adapter-independent half of "no surface
    /// may add scope": it is what `OwnerReviewRequest::narrowed_review`
    /// consults, so every future narrow surface inherits it whether or not
    /// that surface's own delta parser happens to be strict.
    #[test]
    fn a_widened_candidate_is_not_a_strict_narrowing() {
        let action_id = ActionId::new("test.action");
        let dimensions = BTreeMap::from([
            (
                ReviewedScopeDimension::Action,
                ReviewedScopeValue::Action(action_id.clone()),
            ),
            (
                ReviewedScopeDimension::Descriptor,
                ReviewedScopeValue::DescriptorVersion(1),
            ),
            (
                ReviewedScopeDimension::OutputChannel,
                ReviewedScopeValue::OutputChannels(BTreeSet::from(["telegram".to_string()])),
            ),
        ]);
        let mut original = ReviewedActionScope {
            schema_version: 1,
            scope_version: 1,
            action_id,
            descriptor_version: 1,
            dimensions,
            context_class_digest: Digest::parse(format!("sha256:{}", "0".repeat(64))).unwrap(),
        };
        original.context_class_digest = original.calculate_context_class_digest();

        let mut widened = original.clone();
        widened.dimensions.insert(
            ReviewedScopeDimension::OutputChannel,
            ReviewedScopeValue::OutputChannels(BTreeSet::from([
                "telegram".to_string(),
                "email".to_string(),
            ])),
        );
        widened.context_class_digest = widened.calculate_context_class_digest();
        assert!(
            widened.binding_is_valid(),
            "the widened scope is well-formed"
        );

        let narrowed = ReviewedScopeValue::OutputChannels(BTreeSet::new());
        let mut strictly_narrower = original.clone();
        strictly_narrower
            .dimensions
            .insert(ReviewedScopeDimension::OutputChannel, narrowed);
        strictly_narrower.scope_version = original.scope_version + 1;
        strictly_narrower.context_class_digest = strictly_narrower.calculate_context_class_digest();

        let widening_rejected = !original.is_strict_narrowing(&widened);
        assert!(
            widening_rejected,
            "adding an output channel is widening, not narrowing"
        );
        assert!(
            original.is_strict_narrowing(&strictly_narrower),
            "removing an output channel is a strict narrowing"
        );
    }
}

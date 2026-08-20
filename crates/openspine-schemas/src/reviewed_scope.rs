//! Serializable, protocol-neutral owner-reviewed action scopes.

#[path = "reviewed_scope_narrow.rs"]
mod narrow;
pub use narrow::ReviewedScopeNarrowError;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::action::{ActionId, ActionImplementationId, DataDestination, ReviewedScopeDimension};
use crate::briefcase::RelationshipTier;
use crate::digest::{digest_of, Digest};
use crate::disclosure_policy::DisclosureClass;
use crate::egress::EgressClass;
use crate::event::{AccountRole, TargetRef};
use crate::resolved_context::ResolvedActionContext;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorImplementationScope {
    pub implementation_id: ActionImplementationId,
    pub implementation_version: u32,
    pub connector_kind: String,
    pub executor_id: String,
    pub executor_version: u32,
    pub resolver_id: String,
    pub resolver_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedTargetScope {
    pub refs: Vec<TargetRef>,
}

/// Generic values for every reviewed scope dimension. The matcher switches
/// only on these contract dimensions, never on Gmail, Matrix, Slack, or any
/// other protocol name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ReviewedScopeValue {
    Action(ActionId),
    DescriptorVersion(u32),
    ConnectorImplementation(ConnectorImplementationScope),
    ConnectorInstance(String),
    AccountRole(AccountRole),
    AccountIdentity(Digest),
    Target(ReviewedTargetScope),
    Counterparty(Ulid),
    RelationshipTier(RelationshipTier),
    BoundParameters(BTreeMap<String, String>),
    TargetDigest(Digest),
    PayloadDigest(Digest),
    EffectDestination(DataDestination),
    EgressClass(EgressClass),
    DisclosureClass(DisclosureClass),
    OutputChannels(BTreeSet<String>),
    Workflow(String),
    TaskShape(Digest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedActionScope {
    schema_version: u32,
    scope_version: u32,
    action_id: ActionId,
    descriptor_version: u32,
    dimensions: BTreeMap<ReviewedScopeDimension, ReviewedScopeValue>,
    context_class_digest: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ScopeComparison {
    Matches,
    Mismatch {
        dimensions: BTreeSet<ReviewedScopeDimension>,
    },
    InvalidReviewedScope,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReviewedScopeError {
    #[error("resolved context is missing required scope dimension {dimension:?}")]
    MissingDimension { dimension: ReviewedScopeDimension },
}

impl ReviewedActionScope {
    pub fn derive(context: &ResolvedActionContext) -> Result<Self, ReviewedScopeError> {
        let mut dimensions = BTreeMap::new();
        dimensions.insert(
            ReviewedScopeDimension::Action,
            ReviewedScopeValue::Action(context.action_id().clone()),
        );
        dimensions.insert(
            ReviewedScopeDimension::Descriptor,
            ReviewedScopeValue::DescriptorVersion(context.descriptor_version()),
        );
        for dimension in context.required_scope_dimensions() {
            let value =
                value_for(*dimension, context).ok_or(ReviewedScopeError::MissingDimension {
                    dimension: *dimension,
                })?;
            dimensions.insert(*dimension, value);
        }

        let mut scope = Self {
            schema_version: 1,
            scope_version: 1,
            action_id: context.action_id().clone(),
            descriptor_version: context.descriptor_version(),
            dimensions,
            context_class_digest: digest_of(&serde_json::Value::Null),
        };
        scope.context_class_digest = scope.calculate_context_class_digest();
        Ok(scope)
    }

    pub fn compare(&self, context: &ResolvedActionContext) -> ScopeComparison {
        if !self.binding_is_valid() {
            return ScopeComparison::InvalidReviewedScope;
        }
        let mut mismatches = BTreeSet::new();
        if &self.action_id != context.action_id() {
            mismatches.insert(ReviewedScopeDimension::Action);
        }
        if self.descriptor_version != context.descriptor_version() {
            mismatches.insert(ReviewedScopeDimension::Descriptor);
        }
        for (dimension, reviewed) in &self.dimensions {
            if matches!(
                dimension,
                ReviewedScopeDimension::Action | ReviewedScopeDimension::Descriptor
            ) {
                continue;
            }
            if value_for(*dimension, context).as_ref() != Some(reviewed) {
                mismatches.insert(*dimension);
            }
        }
        if mismatches.is_empty() {
            ScopeComparison::Matches
        } else {
            ScopeComparison::Mismatch {
                dimensions: mismatches,
            }
        }
    }

    pub fn action_id(&self) -> &ActionId {
        &self.action_id
    }

    pub fn descriptor_version(&self) -> u32 {
        self.descriptor_version
    }

    pub fn dimensions(&self) -> &BTreeMap<ReviewedScopeDimension, ReviewedScopeValue> {
        &self.dimensions
    }

    pub fn context_class_digest(&self) -> &Digest {
        &self.context_class_digest
    }

    /// Verify that persisted scope bytes still match the exact dimensions the
    /// owner reviewed. Comparison fails closed when this binding is invalid.
    pub fn binding_is_valid(&self) -> bool {
        if self.schema_version == 0 || self.scope_version == 0 {
            return false;
        }
        if self.dimensions.get(&ReviewedScopeDimension::Action)
            != Some(&ReviewedScopeValue::Action(self.action_id.clone()))
            || self.dimensions.get(&ReviewedScopeDimension::Descriptor)
                != Some(&ReviewedScopeValue::DescriptorVersion(
                    self.descriptor_version,
                ))
        {
            return false;
        }
        self.context_class_digest == self.calculate_context_class_digest()
    }

    fn calculate_context_class_digest(&self) -> Digest {
        let mut value = serde_json::to_value(self)
            .expect("reviewed action scope contains only serializable fields");
        value
            .as_object_mut()
            .expect("reviewed action scope serializes as an object")
            .remove("context_class_digest");
        digest_of(&value)
    }
}

/// Build a `ReviewedScopeValue` for `dimension` from a resolved context.
/// `None` when the context does not carry that instance value (e.g. an
/// unresolvable account identity). `pub(crate)` so the resolved-context
/// scope key can be derived here and the digest helper can be shared.
pub(crate) fn value_for(
    dimension: ReviewedScopeDimension,
    context: &ResolvedActionContext,
) -> Option<ReviewedScopeValue> {
    match dimension {
        ReviewedScopeDimension::Action => {
            Some(ReviewedScopeValue::Action(context.action_id().clone()))
        }
        ReviewedScopeDimension::Descriptor => Some(ReviewedScopeValue::DescriptorVersion(
            context.descriptor_version(),
        )),
        ReviewedScopeDimension::ConnectorImplementation => Some(
            ReviewedScopeValue::ConnectorImplementation(ConnectorImplementationScope {
                implementation_id: context.implementation_id().clone(),
                implementation_version: context.implementation_version(),
                connector_kind: context.connector_kind().to_string(),
                executor_id: context.executor_id().to_string(),
                executor_version: context.executor_version(),
                resolver_id: context.resolver_id().to_string(),
                resolver_version: context.resolver_version(),
            }),
        ),
        ReviewedScopeDimension::ConnectorInstance => Some(ReviewedScopeValue::ConnectorInstance(
            context.connector_instance_id().to_string(),
        )),
        ReviewedScopeDimension::AccountRole => {
            context.account_role().map(ReviewedScopeValue::AccountRole)
        }
        ReviewedScopeDimension::AccountIdentity => context
            .account_identity_digest()
            .cloned()
            .map(ReviewedScopeValue::AccountIdentity),
        ReviewedScopeDimension::Target => Some(ReviewedScopeValue::Target(ReviewedTargetScope {
            refs: context.target_refs().to_vec(),
        })),
        ReviewedScopeDimension::Counterparty => context
            .counterparty_identity_id()
            .map(ReviewedScopeValue::Counterparty),
        ReviewedScopeDimension::RelationshipTier => context
            .relationship_tier()
            .map(ReviewedScopeValue::RelationshipTier),
        ReviewedScopeDimension::BoundParameters => Some(ReviewedScopeValue::BoundParameters(
            context.bound_parameters().clone(),
        )),
        ReviewedScopeDimension::TargetDigest => context
            .target_digest()
            .cloned()
            .map(ReviewedScopeValue::TargetDigest),
        ReviewedScopeDimension::PayloadDigest => context
            .payload_digest()
            .cloned()
            .map(ReviewedScopeValue::PayloadDigest),
        ReviewedScopeDimension::EffectDestination => Some(ReviewedScopeValue::EffectDestination(
            context.effect_destination(),
        )),
        ReviewedScopeDimension::EgressClass => {
            context.egress_class().map(ReviewedScopeValue::EgressClass)
        }
        ReviewedScopeDimension::DisclosureClass => context
            .disclosure_class()
            .map(ReviewedScopeValue::DisclosureClass),
        ReviewedScopeDimension::OutputChannel => Some(ReviewedScopeValue::OutputChannels(
            context.output_channels().clone(),
        )),
        ReviewedScopeDimension::Workflow => context
            .workflow_id()
            .map(|value| ReviewedScopeValue::Workflow(value.to_string())),
        ReviewedScopeDimension::TaskShape => context
            .task_shape_digest()
            .cloned()
            .map(ReviewedScopeValue::TaskShape),
    }
}

/// Sealed digest over the *values* named by the descriptor's required scope
/// dimensions — the standing-rule scope key (design.md §"Two digests"). The
/// pre-image is a canonical-JSON object whose keys are the snake_case
/// dimension names and whose values are the reviewed values.
///
/// Dimension *ordering* is deliberately not load-bearing here: `digest_of`
/// canonicalizes through `canonical_json`, which sorts object keys at every
/// depth, so the insertion order of this map cannot reach the pre-image. Two
/// contexts that agree on every required dimension therefore always produce
/// byte-identical pre-images, whatever order the dimensions were visited in.
///
/// This deliberately excludes every declaration axis the compatibility digest
/// covers (action/descriptor/implementation/executor/resolver/egress), so two
/// different accounts or targets cannot collide into one scope key.
pub fn reviewed_scope_digest_of(
    values: &BTreeMap<ReviewedScopeDimension, ReviewedScopeValue>,
) -> Digest {
    let mut object = serde_json::Map::new();
    for (dimension, value) in values {
        object.insert(
            serde_json::to_value(dimension)
                .expect("reviewed scope dimension serializes as a string")
                .as_str()
                .expect("reviewed scope dimension serializes as a string")
                .to_string(),
            serde_json::to_value(value).expect("reviewed scope value is fully serializable"),
        );
    }
    digest_of(&serde_json::Value::Object(object))
}

/// Compute a per-dimension reviewed value map for a resolved context over
/// exactly the dimensions named by the descriptor's `required_scope_dimensions`.
/// `None` when any required dimension cannot be valued (missing instance), so
/// the caller fails closed rather than sealing a partial scope.
pub fn reviewed_scope_values_of(
    context: &ResolvedActionContext,
) -> Option<BTreeMap<ReviewedScopeDimension, ReviewedScopeValue>> {
    let mut values = BTreeMap::new();
    for dimension in context.required_scope_dimensions() {
        values.insert(*dimension, value_for(*dimension, context)?);
    }
    Some(values)
}

//! Serializable, protocol-neutral owner-reviewed action scopes.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::action::{
    ActionDescriptor, ActionId, ActionImplementationId, DataDestination, ReviewedScopeDimension,
};
use crate::briefcase::RelationshipTier;
use crate::digest::{digest_of, Digest};
use crate::egress::EgressClass;
use crate::event::{AccountRole, DataClassification, TargetRef};
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
    DisclosureClass(DataClassification),
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
    #[error("descriptor and resolved context name different actions")]
    ActionMismatch,
    #[error("descriptor version changed before scope derivation")]
    DescriptorVersionMismatch,
    #[error("resolved context is missing required scope dimension {dimension:?}")]
    MissingDimension { dimension: ReviewedScopeDimension },
}

impl ReviewedActionScope {
    pub fn derive(
        descriptor: &ActionDescriptor,
        context: &ResolvedActionContext,
    ) -> Result<Self, ReviewedScopeError> {
        if &descriptor.action_id != context.action_id() {
            return Err(ReviewedScopeError::ActionMismatch);
        }
        if descriptor.descriptor_version != context.descriptor_version() {
            return Err(ReviewedScopeError::DescriptorVersionMismatch);
        }

        let mut dimensions = BTreeMap::new();
        dimensions.insert(
            ReviewedScopeDimension::Action,
            ReviewedScopeValue::Action(context.action_id().clone()),
        );
        dimensions.insert(
            ReviewedScopeDimension::Descriptor,
            ReviewedScopeValue::DescriptorVersion(context.descriptor_version()),
        );
        for dimension in &descriptor.required_scope_dimensions {
            let value =
                value_for(*dimension, context).ok_or(ReviewedScopeError::MissingDimension {
                    dimension: *dimension,
                })?;
            dimensions.insert(*dimension, value);
        }

        let schema_version = 1;
        let scope_version = 1;
        let context_class_digest = calculate_context_class_digest(
            schema_version,
            scope_version,
            context.action_id(),
            context.descriptor_version(),
            &dimensions,
        );
        Ok(Self {
            schema_version,
            scope_version,
            action_id: context.action_id().clone(),
            descriptor_version: context.descriptor_version(),
            dimensions,
            context_class_digest,
        })
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
        self.context_class_digest
            == calculate_context_class_digest(
                self.schema_version,
                self.scope_version,
                &self.action_id,
                self.descriptor_version,
                &self.dimensions,
            )
    }
}

fn calculate_context_class_digest(
    schema_version: u32,
    scope_version: u32,
    action_id: &ActionId,
    descriptor_version: u32,
    dimensions: &BTreeMap<ReviewedScopeDimension, ReviewedScopeValue>,
) -> Digest {
    digest_of(&serde_json::json!({
        "schema_version": schema_version,
        "scope_version": scope_version,
        "action_id": action_id,
        "descriptor_version": descriptor_version,
        "dimensions": dimensions,
    }))
}

fn value_for(
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

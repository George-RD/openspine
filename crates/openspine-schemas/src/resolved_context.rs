//! Kernel-resolved, protocol-neutral action context.
//!
//! The shell supplies an intent. The kernel resolves and seals the trusted
//! connector/account/target/counterparty context represented here before any
//! reusable-delegation proposal can be derived from it.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use ulid::Ulid;

use crate::action::{
    ActionCatalog, ActionId, ActionImplementationId, DataDestination, DelegationCatalogError,
    ReviewedScopeDimension,
};
use crate::briefcase::{CounterpartyRef, RelationshipTier};
use crate::digest::{digest_of, Digest};
use crate::disclosure_policy::DisclosureClass;
use crate::egress::EgressClass;
use crate::event::{AccountRole, TargetRef, TargetRefKind};

/// Kernel inputs that remain after action and implementation declarations are selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedActionContextInput {
    pub connector_instance_id: String,
    pub account_role: Option<AccountRole>,
    pub account_identity_digest: Option<Digest>,
    pub target_refs: Vec<TargetRef>,
    pub counterparty: Option<CounterpartyRef>,
    pub bound_parameters: BTreeMap<String, String>,
    pub target_digest: Option<Digest>,
    pub payload_digest: Option<Digest>,
    pub workflow_id: Option<String>,
    pub task_shape_digest: Option<Digest>,
}

/// A sealed context class. It can be created only from catalog-selected action
/// semantics, implementation readiness, and catalog-owned effect metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedActionContext {
    schema_version: u32,
    action_id: ActionId,
    descriptor_version: u32,
    delegation_policy_version: u32,
    required_scope_dimensions: BTreeSet<ReviewedScopeDimension>,
    implementation_id: ActionImplementationId,
    implementation_version: u32,
    connector_kind: String,
    connector_instance_id: String,
    executor_id: String,
    executor_version: u32,
    resolver_id: String,
    resolver_version: u32,
    account_role: Option<AccountRole>,
    account_identity_digest: Option<Digest>,
    target_refs: Vec<TargetRef>,
    counterparty_identity_id: Option<Ulid>,
    relationship_tier: Option<RelationshipTier>,
    bound_parameters: BTreeMap<String, String>,
    target_digest: Option<Digest>,
    payload_digest: Option<Digest>,
    effect_destination: DataDestination,
    egress_class: Option<EgressClass>,
    disclosure_class: Option<DisclosureClass>,
    output_channels: BTreeSet<String>,
    workflow_id: Option<String>,
    task_shape_digest: Option<Digest>,
    compatibility_digest: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolvedActionContextError {
    #[error(transparent)]
    InvalidDelegationCatalog(#[from] DelegationCatalogError),
    #[error("action {action_id} has no catalog-owned egress declaration")]
    MissingCatalogEgressDeclaration { action_id: ActionId },
    #[error("required reviewed scope dimension {dimension:?} is missing")]
    MissingScopeDimension { dimension: ReviewedScopeDimension },
    #[error("reusable delegation requires an identity-bound counterparty")]
    CounterpartyMustBeBound,
    #[error("a canonical target reference has no stable id")]
    InvalidTargetReference,
}

impl ResolvedActionContext {
    pub fn try_new(
        catalog: &ActionCatalog,
        action_id: &ActionId,
        implementation_id: &ActionImplementationId,
        input: ResolvedActionContextInput,
    ) -> Result<Self, ResolvedActionContextError> {
        let (descriptor, implementation) =
            catalog.validated_delegation_contract(action_id, implementation_id)?;
        let egress_declaration = catalog.egress_decl_for(action_id).ok_or_else(|| {
            ResolvedActionContextError::MissingCatalogEgressDeclaration {
                action_id: action_id.clone(),
            }
        })?;

        let (counterparty_identity_id, relationship_tier) = match input.counterparty {
            Some(CounterpartyRef::Bound {
                identity_id,
                relationship,
            }) => (Some(identity_id), Some(relationship.into())),
            Some(CounterpartyRef::Unresolved { .. }) => {
                if descriptor
                    .required_scope_dimensions
                    .contains(&ReviewedScopeDimension::Counterparty)
                    || descriptor
                        .required_scope_dimensions
                        .contains(&ReviewedScopeDimension::RelationshipTier)
                {
                    return Err(ResolvedActionContextError::CounterpartyMustBeBound);
                }
                (None, None)
            }
            None => (None, None),
        };

        let mut target_refs = input.target_refs;
        if target_refs
            .iter()
            .any(|target| target.id.as_ref().is_some_and(|id| id.trim().is_empty()))
        {
            return Err(ResolvedActionContextError::InvalidTargetReference);
        }
        target_refs.sort_by(|left, right| {
            target_kind_rank(left.kind)
                .cmp(&target_kind_rank(right.kind))
                .then_with(|| left.id.cmp(&right.id))
        });
        target_refs.dedup();

        let policy_version = descriptor
            .delegation_policy
            .as_ref()
            .map_or(0, |policy| policy.policy_version);
        let output_channels = egress_declaration
            .output_channels
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect();
        let compatibility_digest = digest_of(&serde_json::json!({
            "action_id": descriptor.action_id,
            "descriptor_version": descriptor.descriptor_version,
            "delegation_policy_version": policy_version,
            "implementation_id": implementation.implementation_id,
            "implementation_version": implementation.implementation_version,
            "connector_kind": implementation.connector_kind,
            "executor_id": implementation.executor_id,
            "executor_version": implementation.executor_version,
            "resolver_id": implementation.resolver_id,
            "resolver_version": implementation.resolver_version,
            "effect_destination": descriptor.semantics.destination,
            "required_scope_dimensions": descriptor.required_scope_dimensions,
            "egress_class": egress_declaration.egress_class,
            "output_channels": egress_declaration.output_channels,
        }));

        let context = Self {
            schema_version: 1,
            action_id: descriptor.action_id.clone(),
            descriptor_version: descriptor.descriptor_version,
            delegation_policy_version: policy_version,
            required_scope_dimensions: descriptor.required_scope_dimensions.clone(),
            implementation_id: implementation.implementation_id.clone(),
            implementation_version: implementation.implementation_version,
            connector_kind: implementation.connector_kind.clone(),
            connector_instance_id: input.connector_instance_id,
            executor_id: implementation.executor_id.clone(),
            executor_version: implementation.executor_version,
            resolver_id: implementation.resolver_id.clone(),
            resolver_version: implementation.resolver_version,
            account_role: input.account_role,
            account_identity_digest: input.account_identity_digest,
            target_refs,
            counterparty_identity_id,
            relationship_tier,
            bound_parameters: input.bound_parameters,
            target_digest: input.target_digest,
            payload_digest: input.payload_digest,
            effect_destination: descriptor.semantics.destination,
            egress_class: egress_declaration.egress_class,
            disclosure_class: None,
            output_channels,
            workflow_id: input.workflow_id,
            task_shape_digest: input.task_shape_digest,
            compatibility_digest,
        };
        context.validate_required_dimensions()?;
        Ok(context)
    }

    fn validate_required_dimensions(&self) -> Result<(), ResolvedActionContextError> {
        for dimension in &self.required_scope_dimensions {
            let present = match dimension {
                ReviewedScopeDimension::Action
                | ReviewedScopeDimension::Descriptor
                | ReviewedScopeDimension::ConnectorImplementation
                | ReviewedScopeDimension::EffectDestination
                | ReviewedScopeDimension::BoundParameters => true,
                ReviewedScopeDimension::ConnectorInstance => {
                    !self.connector_instance_id.trim().is_empty()
                }
                ReviewedScopeDimension::AccountRole => self.account_role.is_some(),
                ReviewedScopeDimension::AccountIdentity => self.account_identity_digest.is_some(),
                ReviewedScopeDimension::Target => {
                    !self.target_refs.is_empty()
                        && self.target_refs.iter().all(|target| target.id.is_some())
                }
                ReviewedScopeDimension::Counterparty => self.counterparty_identity_id.is_some(),
                ReviewedScopeDimension::RelationshipTier => self.relationship_tier.is_some(),
                ReviewedScopeDimension::TargetDigest => self.target_digest.is_some(),
                ReviewedScopeDimension::PayloadDigest => self.payload_digest.is_some(),
                ReviewedScopeDimension::EgressClass => self.egress_class.is_some(),
                ReviewedScopeDimension::DisclosureClass => self.disclosure_class.is_some(),
                ReviewedScopeDimension::OutputChannel => !self.output_channels.is_empty(),
                ReviewedScopeDimension::Workflow => self
                    .workflow_id
                    .as_ref()
                    .is_some_and(|workflow| !workflow.trim().is_empty()),
                ReviewedScopeDimension::TaskShape => self.task_shape_digest.is_some(),
            };
            if !present {
                return Err(ResolvedActionContextError::MissingScopeDimension {
                    dimension: *dimension,
                });
            }
        }
        Ok(())
    }

    pub fn action_id(&self) -> &ActionId {
        &self.action_id
    }

    pub fn descriptor_version(&self) -> u32 {
        self.descriptor_version
    }

    pub fn delegation_policy_version(&self) -> u32 {
        self.delegation_policy_version
    }

    pub(crate) fn required_scope_dimensions(&self) -> &BTreeSet<ReviewedScopeDimension> {
        &self.required_scope_dimensions
    }

    pub fn implementation_id(&self) -> &ActionImplementationId {
        &self.implementation_id
    }

    pub fn implementation_version(&self) -> u32 {
        self.implementation_version
    }

    pub fn connector_kind(&self) -> &str {
        &self.connector_kind
    }

    pub fn connector_instance_id(&self) -> &str {
        &self.connector_instance_id
    }

    pub fn executor_id(&self) -> &str {
        &self.executor_id
    }

    pub fn executor_version(&self) -> u32 {
        self.executor_version
    }

    pub fn resolver_id(&self) -> &str {
        &self.resolver_id
    }

    pub fn resolver_version(&self) -> u32 {
        self.resolver_version
    }

    pub fn account_role(&self) -> Option<AccountRole> {
        self.account_role
    }

    pub fn account_identity_digest(&self) -> Option<&Digest> {
        self.account_identity_digest.as_ref()
    }

    pub fn target_refs(&self) -> &[TargetRef] {
        &self.target_refs
    }

    pub fn counterparty_identity_id(&self) -> Option<Ulid> {
        self.counterparty_identity_id
    }

    pub fn relationship_tier(&self) -> Option<RelationshipTier> {
        self.relationship_tier
    }

    pub fn bound_parameters(&self) -> &BTreeMap<String, String> {
        &self.bound_parameters
    }

    pub fn target_digest(&self) -> Option<&Digest> {
        self.target_digest.as_ref()
    }

    pub fn payload_digest(&self) -> Option<&Digest> {
        self.payload_digest.as_ref()
    }

    pub fn effect_destination(&self) -> DataDestination {
        self.effect_destination
    }

    pub fn egress_class(&self) -> Option<EgressClass> {
        self.egress_class
    }

    pub fn disclosure_class(&self) -> Option<DisclosureClass> {
        self.disclosure_class
    }

    pub fn output_channels(&self) -> &BTreeSet<String> {
        &self.output_channels
    }

    pub fn workflow_id(&self) -> Option<&str> {
        self.workflow_id.as_deref()
    }

    pub fn task_shape_digest(&self) -> Option<&Digest> {
        self.task_shape_digest.as_ref()
    }

    pub fn compatibility_digest(&self) -> &Digest {
        &self.compatibility_digest
    }

    /// The standing-rule scope key: a sealed digest over exactly the values
    /// named by the descriptor's `required_scope_dimensions`. Distinct from
    /// [`Self::compatibility_digest`] (the drift epoch over declaration axes
    /// only), so two different accounts or targets never collide into one
    /// pattern and a declaration change is detected by the epoch, never by
    /// the scope key (design.md §"Two digests"). `None` when a required
    /// dimension cannot be valued — the caller must fail closed rather than
    /// seal a partial scope.
    pub fn reviewed_scope_digest(&self) -> Option<Digest> {
        crate::reviewed_scope::reviewed_scope_values_of(self)
            .map(|values| crate::reviewed_scope::reviewed_scope_digest_of(&values))
    }

    /// The reviewed value for one scope dimension, or `None` when the context
    /// does not carry that instance value. Lets a caller persist and compare
    /// individual dimensions (not only the sealed digest), so comparison can
    /// name the exact changed dimensions and narrowing need not re-review the
    /// rest.
    pub fn reviewed_scope_value(
        &self,
        dimension: ReviewedScopeDimension,
    ) -> Option<crate::reviewed_scope::ReviewedScopeValue> {
        crate::reviewed_scope::value_for(dimension, self)
    }
}

fn target_kind_rank(kind: TargetRefKind) -> u8 {
    match kind {
        TargetRefKind::EmailThread => 0,
        TargetRefKind::Conversation => 1,
        TargetRefKind::Project => 2,
        TargetRefKind::Deployment => 3,
        TargetRefKind::SecretSlot => 4,
        TargetRefKind::None => 5,
    }
}

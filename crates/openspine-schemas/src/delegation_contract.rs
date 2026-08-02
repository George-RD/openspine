//! Protocol-neutral declarations for reusable delegation.
//!
//! These types describe whether a canonical action and one concrete action
//! implementation are safe to propose for reuse. They do not grant authority:
//! every live task still receives an ordinary [`crate::grant::TaskGrant`].

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::action::ActionId;
use crate::standing_rule::BudgetWindow;

/// Stable identifier for one resolver/executor implementation of an action.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActionImplementationId(String);

impl ActionImplementationId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ActionImplementationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Semantic effect class used to choose conservative delegation rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    ReadOnly,
    InternalMutation,
    OwnerAccountWrite,
    CounterpartyCommunication,
    SharedWorkspaceWrite,
    PublicPublish,
    SystemOperation,
    SecretOperation,
}

impl EffectKind {
    pub fn is_communication_or_connector_write(self) -> bool {
        matches!(
            self,
            Self::OwnerAccountWrite
                | Self::CounterpartyCommunication
                | Self::SharedWorkspaceWrite
                | Self::PublicPublish
        )
    }
}

/// Whether an effect can be undone after it reaches its destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectReversibility {
    Reversible,
    ConditionallyReversible,
    Irreversible,
}

/// The semantic destination/visibility of an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataDestination {
    InternalOnly,
    OwnerDevice,
    OwnerCloudAccount,
    BoundCounterparty,
    SharedWorkspace,
    Public,
    ExternalService,
}

impl DataDestination {
    pub fn is_communication_or_connector_write(self) -> bool {
        matches!(
            self,
            Self::OwnerCloudAccount
                | Self::BoundCounterparty
                | Self::SharedWorkspace
                | Self::Public
        )
    }
}

/// One trusted context dimension that an owner-reviewed scope may bind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewedScopeDimension {
    Action,
    Descriptor,
    ConnectorImplementation,
    ConnectorInstance,
    AccountRole,
    AccountIdentity,
    Target,
    Counterparty,
    RelationshipTier,
    BoundParameters,
    TargetDigest,
    PayloadDigest,
    EffectDestination,
    EgressClass,
    DisclosureClass,
    OutputChannel,
    Workflow,
    TaskShape,
}

/// Owner-facing language and effect semantics owned by the action catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionSemantics {
    pub owner_verb: String,
    pub owner_object: String,
    pub owner_target: String,
    pub effect_kind: EffectKind,
    pub reversibility: EffectReversibility,
    pub destination: DataDestination,
}

/// Inclusive limits for one configurable sliding-window budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetWindowBounds {
    pub minimum_max: u32,
    pub maximum_max: u32,
    pub minimum_window_secs: i64,
    pub maximum_window_secs: i64,
}

impl BudgetWindowBounds {
    fn contains(self, value: BudgetWindow) -> bool {
        (self.minimum_max..=self.maximum_max).contains(&value.max)
            && (self.minimum_window_secs..=self.maximum_window_secs).contains(&value.window_secs)
    }
}

/// Whether a proposal may prefill reviewed limits from catalog defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationProposalMode {
    ExplicitLimitsRequired,
    DefaultsPermitted,
}

/// Catalog policy for silence/default handling at this effect boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum DarkWindowPolicy {
    #[default]
    Prohibited,
    DenyOnly {
        maximum_timeout_secs: i64,
        maximum_outstanding: u32,
    },
    BoundedAllow {
        maximum_timeout_secs: i64,
        maximum_outstanding: u32,
    },
}

/// Optional owner-review defaults constrained by [`DelegationPolicyBounds`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegationDefaults {
    pub quota: BudgetWindow,
    pub rate: BudgetWindow,
    pub expires_after_secs: i64,
}

/// Risk-tiered limits and fail-closed default posture for one action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegationPolicyBounds {
    pub schema_version: u32,
    pub policy_version: u32,
    pub quota: BudgetWindowBounds,
    pub rate: BudgetWindowBounds,
    pub maximum_lapse_secs: i64,
    pub proposal_mode: DelegationProposalMode,
    #[serde(default)]
    pub defaults: Option<DelegationDefaults>,
    #[serde(default)]
    pub dark_window_policy: DarkWindowPolicy,
    pub fresh_target_selection_required: bool,
}

/// Catalog-owned declaration for a canonical action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionDescriptor {
    pub schema_version: u32,
    pub descriptor_version: u32,
    pub action_id: ActionId,
    pub semantics: ActionSemantics,
    pub reusable_delegation: bool,
    #[serde(default)]
    pub required_scope_dimensions: BTreeSet<ReviewedScopeDimension>,
    #[serde(default)]
    pub delegation_policy: Option<DelegationPolicyBounds>,
}

impl ActionDescriptor {
    pub fn is_communication_or_connector_write(&self) -> bool {
        self.semantics
            .effect_kind
            .is_communication_or_connector_write()
            || self
                .semantics
                .destination
                .is_communication_or_connector_write()
    }
}

/// Catalog-owned resolver/executor declaration for one connector implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionImplementationDescriptor {
    pub schema_version: u32,
    pub implementation_version: u32,
    pub action_id: ActionId,
    pub implementation_id: ActionImplementationId,
    pub connector_kind: String,
    pub executor_id: String,
    pub executor_version: u32,
    pub resolver_id: String,
    pub resolver_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DelegationCatalogError {
    #[error("action {action_id} is absent from the canonical catalog")]
    UnknownAction { action_id: ActionId },
    #[error("action {action_id} has no reusable-delegation descriptor")]
    MissingActionDescriptor { action_id: ActionId },
    #[error("implementation {implementation_id} has no resolver/executor descriptor")]
    MissingImplementationDescriptor {
        implementation_id: ActionImplementationId,
    },
    #[error(transparent)]
    Ineligible(#[from] DelegationEligibilityError),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DelegationEligibilityError {
    #[error("action descriptor field {field} is incomplete")]
    IncompleteDescriptor { field: &'static str },
    #[error("action implementation field {field} is incomplete")]
    IncompleteImplementation { field: &'static str },
    #[error("the action does not support reusable delegation")]
    ReusableDelegationUnsupported,
    #[error("a reusable action requires delegation policy bounds")]
    MissingDelegationPolicy,
    #[error("the action and implementation descriptors name different actions")]
    ImplementationActionMismatch,
    #[error("communication or connector-write delegation has an unsafe action-only scope")]
    CommunicationScopeTooBroad,
    #[error("dark-window Allow is forbidden for communication and connector-write effects")]
    CommunicationDarkWindowAllowForbidden,
    #[error("delegation policy field {field} is invalid")]
    InvalidPolicyBounds { field: &'static str },
    #[error("delegation default {field} falls outside the catalog policy bounds")]
    DefaultOutOfBounds { field: &'static str },
}

/// Pure, fail-closed eligibility check used before any owner proposal exists.
pub fn validate_delegation_contract(
    descriptor: &ActionDescriptor,
    implementation: &ActionImplementationDescriptor,
) -> Result<(), DelegationEligibilityError> {
    validate_descriptor_shape(descriptor)?;
    validate_implementation_shape(implementation)?;
    if descriptor.action_id != implementation.action_id {
        return Err(DelegationEligibilityError::ImplementationActionMismatch);
    }
    if !descriptor.reusable_delegation {
        return Err(DelegationEligibilityError::ReusableDelegationUnsupported);
    }
    let policy = descriptor
        .delegation_policy
        .as_ref()
        .ok_or(DelegationEligibilityError::MissingDelegationPolicy)?;

    if descriptor.is_communication_or_connector_write() {
        let minimum = [
            ReviewedScopeDimension::ConnectorImplementation,
            ReviewedScopeDimension::ConnectorInstance,
            ReviewedScopeDimension::AccountRole,
            ReviewedScopeDimension::AccountIdentity,
            ReviewedScopeDimension::Target,
            ReviewedScopeDimension::EffectDestination,
            ReviewedScopeDimension::Workflow,
            ReviewedScopeDimension::TaskShape,
        ];
        if minimum
            .iter()
            .any(|dimension| !descriptor.required_scope_dimensions.contains(dimension))
        {
            return Err(DelegationEligibilityError::CommunicationScopeTooBroad);
        }
        if matches!(
            policy.dark_window_policy,
            DarkWindowPolicy::BoundedAllow { .. }
        ) {
            return Err(DelegationEligibilityError::CommunicationDarkWindowAllowForbidden);
        }
    }

    validate_policy(policy)
}

fn validate_descriptor_shape(
    descriptor: &ActionDescriptor,
) -> Result<(), DelegationEligibilityError> {
    for (field, missing) in [
        ("schema_version", descriptor.schema_version == 0),
        ("descriptor_version", descriptor.descriptor_version == 0),
        ("action_id", descriptor.action_id.as_str().trim().is_empty()),
        (
            "owner_verb",
            descriptor.semantics.owner_verb.trim().is_empty(),
        ),
        (
            "owner_object",
            descriptor.semantics.owner_object.trim().is_empty(),
        ),
        (
            "owner_target",
            descriptor.semantics.owner_target.trim().is_empty(),
        ),
    ] {
        if missing {
            return Err(DelegationEligibilityError::IncompleteDescriptor { field });
        }
    }
    Ok(())
}

fn validate_implementation_shape(
    implementation: &ActionImplementationDescriptor,
) -> Result<(), DelegationEligibilityError> {
    for (field, missing) in [
        ("schema_version", implementation.schema_version == 0),
        (
            "implementation_version",
            implementation.implementation_version == 0,
        ),
        (
            "action_id",
            implementation.action_id.as_str().trim().is_empty(),
        ),
        (
            "implementation_id",
            implementation.implementation_id.as_str().trim().is_empty(),
        ),
        (
            "connector_kind",
            implementation.connector_kind.trim().is_empty(),
        ),
        ("executor_id", implementation.executor_id.trim().is_empty()),
        ("executor_version", implementation.executor_version == 0),
        ("resolver_id", implementation.resolver_id.trim().is_empty()),
        ("resolver_version", implementation.resolver_version == 0),
    ] {
        if missing {
            return Err(DelegationEligibilityError::IncompleteImplementation { field });
        }
    }
    Ok(())
}

fn validate_policy(policy: &DelegationPolicyBounds) -> Result<(), DelegationEligibilityError> {
    for (field, invalid) in [
        ("schema_version", policy.schema_version == 0),
        ("policy_version", policy.policy_version == 0),
        (
            "quota",
            policy.quota.minimum_max == 0
                || policy.quota.minimum_max > policy.quota.maximum_max
                || policy.quota.minimum_window_secs <= 0
                || policy.quota.minimum_window_secs > policy.quota.maximum_window_secs,
        ),
        (
            "rate",
            policy.rate.minimum_max == 0
                || policy.rate.minimum_max > policy.rate.maximum_max
                || policy.rate.minimum_window_secs <= 0
                || policy.rate.minimum_window_secs > policy.rate.maximum_window_secs,
        ),
        ("maximum_lapse_secs", policy.maximum_lapse_secs <= 0),
    ] {
        if invalid {
            return Err(DelegationEligibilityError::InvalidPolicyBounds { field });
        }
    }

    match policy.dark_window_policy {
        DarkWindowPolicy::Prohibited => {}
        DarkWindowPolicy::DenyOnly {
            maximum_timeout_secs,
            maximum_outstanding,
        }
        | DarkWindowPolicy::BoundedAllow {
            maximum_timeout_secs,
            maximum_outstanding,
        } if maximum_timeout_secs <= 0 || maximum_outstanding == 0 => {
            return Err(DelegationEligibilityError::InvalidPolicyBounds {
                field: "dark_window_policy",
            });
        }
        _ => {}
    }

    if matches!(
        policy.proposal_mode,
        DelegationProposalMode::ExplicitLimitsRequired
    ) && policy.defaults.is_some()
    {
        return Err(DelegationEligibilityError::InvalidPolicyBounds { field: "defaults" });
    }
    if let Some(defaults) = policy.defaults {
        if !policy.quota.contains(defaults.quota) {
            return Err(DelegationEligibilityError::DefaultOutOfBounds { field: "quota" });
        }
        if !policy.rate.contains(defaults.rate) {
            return Err(DelegationEligibilityError::DefaultOutOfBounds { field: "rate" });
        }
        if defaults.expires_after_secs <= 0
            || defaults.expires_after_secs > policy.maximum_lapse_secs
        {
            return Err(DelegationEligibilityError::DefaultOutOfBounds {
                field: "expires_after_secs",
            });
        }
    }
    Ok(())
}

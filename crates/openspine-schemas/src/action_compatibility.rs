//! Catalog-only compatibility epoch derivation for lifecycle revalidation.

use crate::digest::{digest_of, Digest};

use crate::action::{ActionCatalog, ActionId};

impl ActionCatalog {
    /// Re-derive the declaration-axis epoch without requiring live task input.
    pub fn compatibility_digest_for(&self, action_id: &ActionId) -> Option<Digest> {
        let descriptor = self.delegation_descriptor_for(action_id)?;
        let implementation = self.implementation_descriptor_for_action(action_id)?;
        let egress = self.egress_decl_for(action_id)?;
        let policy_version = descriptor
            .delegation_policy
            .as_ref()
            .map_or(0, |policy| policy.policy_version);
        Some(digest_of(&serde_json::json!({
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
            "egress_class": egress.egress_class,
            "output_channels": egress.output_channels,
        })))
    }
}

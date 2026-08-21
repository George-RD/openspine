//! Shared root-owner authorization check for non-delegable owner-admin actions.
//!
//! Destructive owner-administrative actions (overlay export/restore, counterparty
//! erasure) are exposed only as non-delegable root-owner gate-mediated actions.
//! The four checks below are the security-critical invariant that a single caller
//! must not diverge over time, so both handlers call this one function rather than
//! keeping private copies.

use openspine_schemas::action::ActionId;
use openspine_schemas::grant::TaskGrant;

use super::actions::DispatchError;
use crate::pipeline::AppState;

/// Require `grant` to be the configured owner's true root grant, carrying exact
/// effective authority for a non-delegable `action`. `label` names the action
/// class in the rejection message so the caller sees a descriptive error.
pub(super) fn require_root_owner_grant(
    state: &AppState,
    grant: &TaskGrant,
    action: &ActionId,
    label: &str,
) -> Result<(), DispatchError> {
    if grant.user != state.owner.principal_id {
        return Err(DispatchError::BadRequest(format!(
            "{label} requires the configured owner principal"
        )));
    }

    let is_root = grant.parent_grant_id.is_none()
        && grant.root_grant_id == grant.id
        && matches!(
            grant.chain.as_slice(),
            [root]
                if root.grant_id == grant.id
                    && root.parent_grant_id.is_none()
        );
    if !is_root {
        return Err(DispatchError::BadRequest(format!(
            "{label} requires a root grant with no delegated hops"
        )));
    }

    if !state.action_catalog.is_non_delegable(action) {
        return Err(DispatchError::BadRequest(format!(
            "{label} requires non-delegable action classification"
        )));
    }

    if !grant.effectively_allows(action) {
        return Err(DispatchError::BadRequest(format!(
            "{label} requires exact effective authority for the action"
        )));
    }

    Ok(())
}

//! What the canonical catalog declares *about a standing rule* for an action:
//! the reviewed scope dimensions a rule must bind, and whether a dark-window
//! `Allow` default is permitted at all. Split from `action_catalog_data.rs` to
//! keep both files under the 500-line gate.
//!
//! The scope-dimension predicate reads the same descriptor table the catalog
//! is assembled from, so the catalog and activation-time enforcement cannot
//! disagree. The `Allow` eligibility predicate is a standalone allowlist,
//! deliberately not inferred from other declarations: an inferred rule reads
//! as strict while quietly permitting everything it cannot classify.

use openspine_schemas::action::{ActionId, ReviewedScopeDimension};
use std::collections::BTreeSet;

use super::action_catalog_data::delegation_descriptors;

/// The reviewed scope dimensions the canonical descriptor declares for
/// `action`, or `None` when no descriptor declares any. Single source of truth
/// shared by the catalog and by activation-time scope-binding enforcement
/// (standing-rules spec, "Reviewed scope is bound at activation"), so the two
/// can never disagree about what a rule for this action must bind.
pub(crate) fn required_scope_dimensions_for(
    action: &ActionId,
) -> Option<BTreeSet<ReviewedScopeDimension>> {
    delegation_descriptors()
        .into_iter()
        .find(|descriptor| &descriptor.action_id == action)
        .map(|descriptor| descriptor.required_scope_dimensions)
        .filter(|dimensions| !dimensions.is_empty())
}

/// Whether the catalog explicitly declares `action` eligible for a dark-window
/// `Allow` default.
///
/// Fail-closed by construction: eligibility is an **allowlist**, so an action
/// is ineligible unless it is named, and an id the catalog has never heard of
/// is ineligible too. The allowlist is empty today.
///
/// The earlier shape of this predicate inverted the polarity — eligible
/// *unless* the catalog could prove the action was communication or a
/// connector write. That read as strict and was maximally permissive: it
/// refused five ids out of roughly fifty, left `coolify.deploy`,
/// `filesystem.host_write`, `secret.rotate`, `network.raw_egress` and
/// `policy.modify_direct` eligible, and made every uncatalogued id eligible.
/// D-162 states the allowlist rule, so the allowlist is what the code does.
///
/// Adding an id here is an explicit catalog decision needing
/// proposal-specific proof (#133); D-146 forbids one for any communication or
/// connector-write effect regardless.
pub(crate) fn dark_window_allow_eligible(action: &ActionId) -> bool {
    dark_window_allow_eligible_actions().contains(action)
}

/// The dark-window `Allow` eligibility allowlist. Deliberately empty, and the
/// only thing that makes an action eligible. Mirrors the repo's existing
/// `with_non_effect_stub` convention: an explicit, cardinality-pinned set
/// rather than a rule inferred from other declarations.
pub(crate) fn dark_window_allow_eligible_actions() -> BTreeSet<ActionId> {
    BTreeSet::new()
}

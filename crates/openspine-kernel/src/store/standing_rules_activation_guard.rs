//! Activation-time guards for a standing-rule manifest: the reviewed-scope
//! binding must be complete for the action's descriptor (#128), and a
//! dark-window `Allow` default must be permitted for the action (#135, giving
//! the `responsibility-contract` prohibition its enforcing code). Split from
//! `standing_rules.rs` to keep both files under the 500-line gate.
//!
//! Both live here rather than in `StandingRuleManifest::validate` because both
//! need the catalog, which a self-contained manifest cannot reach.

use openspine_schemas::standing_rule::{DarkWindowDefault, StandingRuleManifest};

/// Why a standing rule may not be activated with a dark-window `Allow`
/// default, or `None` when its default is Deny, it has no dark window, or the
/// catalog explicitly declares the action eligible.
///
/// `responsibility-contract` already requires that reusable delegation reject
/// a dark-window Allow for communication and connector-write effects. Until
/// #135 nothing enforced it: a manifest declaring `Allow` on `email.send`
/// activated. Eligibility is fail-closed and catalog-declared, and enforcement
/// lives at activation for the same reason the required-dimension check does —
/// it is the first point with the catalog in scope.
pub(crate) fn dark_window_allow_rejection(manifest: &StandingRuleManifest) -> Option<String> {
    let dark_window = manifest.dark_window?;
    if !matches!(dark_window.default, DarkWindowDefault::Allow) {
        return None;
    }
    if crate::action_catalog::dark_window_allow_eligible(&manifest.action_id) {
        return None;
    }
    Some(format!(
        "standing rule for {} may not declare a dark-window Allow default: the catalog does not \
         declare this action eligible, and D-146 forbids a silence-admitted Allow for any \
         communication or connector-write effect",
        manifest.action_id
    ))
}

pub(crate) fn scope_binding_rejection(manifest: &StandingRuleManifest) -> Option<String> {
    let required = crate::action_catalog::required_scope_dimensions_for(&manifest.action_id)?;
    let Some(binding) = manifest.reviewed_scope.as_ref() else {
        return Some(format!(
            "standing rule for {} must bind a reviewed scope: its descriptor declares {} required \
             dimension(s)",
            manifest.action_id,
            required.len()
        ));
    };
    let missing = required
        .iter()
        .filter(|dimension| !binding.scope.dimensions().contains_key(dimension))
        .map(|dimension| format!("{dimension:?}"))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Some(format!(
            "standing rule for {} omits required reviewed scope dimension(s): {}",
            manifest.action_id,
            missing.join(", ")
        ));
    }
    None
}

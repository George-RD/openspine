//! What the canonical catalog declares *about a standing rule* for an action:
//! the reviewed scope dimensions a rule must bind, and whether a dark-window
//! `Allow` default is permitted at all. Split from `action_catalog_data.rs` to
//! keep both files under the 500-line gate.
//!
//! The scope-dimension predicate reads the same descriptor table the catalog
//! is assembled from, so the catalog and activation-time enforcement cannot
//! disagree. The `Allow` eligibility predicate is fail-closed on two axes: an
//! explicit allowlist (necessary) AND the catalog's approval-narrowing
//! certification (the misconfiguration guard), never inferred from effect
//! declarations — an inference reads as strict while quietly permitting every
//! effect it cannot classify.

use openspine_schemas::action::{ActionCatalog, ActionId, ReviewedScopeDimension};
use std::collections::BTreeSet;
use std::sync::LazyLock;

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

/// The three outcomes of deciding whether an action may mint a dark-window
/// `Allow` default. Making the *misconfigured* case a distinct value — rather
/// than folding it into "not eligible" — is the point of #135: an allowlist
/// entry for an effectful action is a review error that must fail closed, and
/// having a name for it lets the empty / filled / misconfigured states each be
/// asserted directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DarkWindowAllowEligibility {
    /// On the allowlist and the catalog certifies the action reaches no
    /// connector, write, or counterparty. Only this outcome may mint.
    Eligible,
    /// Absent from the allowlist — the empty/default fail-closed state. With
    /// the allowlist empty this is every action, which is why token minting is
    /// inert today.
    NotAllowlisted,
    /// The allowlist itself is misconfigured — it names at least one entry the
    /// safety boundary does not certify (an effectful action such as
    /// `email.send`/`secret.rotate`, or an id the catalog does not know). A
    /// review error in the reviewed policy fails closed for EVERY action, valid
    /// entries included, so the error is noticed rather than silently
    /// half-applied. D-146 forbids a silence-admitted `Allow` for any
    /// communication or connector-write effect regardless of the allowlist.
    Misconfigured,
}

/// Decide dark-window `Allow` eligibility from two independent inputs: the
/// explicit allowlist, and the subset of it the safety boundary certifies as
/// reaching no external effect (`certified_safe`, always a subset of
/// `allowlist`). Pure and total over the empty / filled / misconfigured
/// states, so each is directly testable without mutating the (empty)
/// production allowlist.
///
/// A *misconfigured allowlist* — one whose entries are not all certified — is
/// evaluated first and fails closed for every queried action, not just the bad
/// entry: "the allowlist is misconfigured" is a property of the list, so a
/// review error disables minting entirely rather than trusting the remaining
/// entries.
///
/// The safety boundary is a **positive** certification, not an inference over
/// effect declarations. An "is this an effect?" inference has false negatives —
/// `secret.rotate`, `policy.modify_direct`, `coolify.deploy` and
/// `filesystem.host_write` all declare no egress class and no output channel,
/// so they would slip past one — whereas requiring the catalog's
/// approval-narrowing certification fails closed on every action it has not
/// affirmatively cleared, uncatalogued ids included.
pub(crate) fn dark_window_allow_eligibility(
    action: &ActionId,
    allowlist: &BTreeSet<ActionId>,
    certified_safe: &BTreeSet<ActionId>,
) -> DarkWindowAllowEligibility {
    if allowlist
        .iter()
        .any(|entry| !certified_safe.contains(entry))
    {
        return DarkWindowAllowEligibility::Misconfigured;
    }
    if !allowlist.contains(action) {
        return DarkWindowAllowEligibility::NotAllowlisted;
    }
    DarkWindowAllowEligibility::Eligible
}

/// Whether the catalog explicitly declares `action` eligible for a dark-window
/// `Allow` default.
///
/// Fail-closed on two independent axes: the action must be named on the
/// [`dark_window_allow_eligible_actions`] allowlist (empty today, so nothing
/// qualifies), AND the canonical catalog must certify it as approval-narrowing
/// — the existing, reviewed classification for actions that "reach no
/// connector, write nothing, communicate with nobody". Either axis absent is
/// ineligible, and an id the catalog has never heard of is ineligible too
/// (D-162 states the allowlist rule; the approval-narrowing axis is the #135
/// misconfiguration guard).
///
/// The safe default is therefore *inert*: with an empty allowlist no `Allow`
/// can mint, and even a misconfigured allowlist entry for an effectful action
/// stays closed because it is not approval-narrowing. Adding an id to the
/// allowlist is an explicit catalog decision needing proposal-specific proof
/// (#133); D-146 forbids one for any communication or connector-write effect
/// regardless, which the approval-narrowing gate enforces mechanically.
pub(crate) fn dark_window_allow_eligible(action: &ActionId) -> bool {
    let allowlist = dark_window_allow_eligible_actions();
    let certified_safe: BTreeSet<ActionId> = allowlist
        .iter()
        .filter(|entry| CANONICAL_CATALOG.is_approval_narrowing(entry))
        .cloned()
        .collect();
    matches!(
        dark_window_allow_eligibility(action, &allowlist, &certified_safe),
        DarkWindowAllowEligibility::Eligible
    )
}

/// The dark-window `Allow` eligibility allowlist. Deliberately empty, and a
/// necessary (not sufficient) condition for eligibility. Mirrors the repo's
/// existing `with_non_effect_stub` convention: an explicit, cardinality-pinned
/// set rather than a rule inferred from other declarations.
pub(crate) fn dark_window_allow_eligible_actions() -> BTreeSet<ActionId> {
    BTreeSet::new()
}

/// The canonical catalog, built once. Eligibility is consulted at activation
/// (`dark_window_allow_rejection`) and by the startup sweep
/// (`sweep_ineligible_dark_window_allow_rules`); neither is hot, but rebuilding
/// the whole catalog per call would be wasteful, so it is memoized here.
static CANONICAL_CATALOG: LazyLock<ActionCatalog> = LazyLock::new(super::canonical_catalog);

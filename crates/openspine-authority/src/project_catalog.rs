//! Capability-derived tool catalog projection (spec #209, IT2).
//!
//! `project_catalog` is the pre-inference projection the Immune-system lane
//! calls for: a **pure, deterministic** function of `(TaskGrant, ActionCatalog)`
//! that produces the exact set of model-consumable tool descriptors a worker
//! receives — and nothing for any action the grant did not carry.
//!
//! It sits beside [`crate::compose_authority`] and shares its discipline: no
//! I/O, no state, no clock, no randomness. It is **policy-free** (spec #209 D5):
//! it projects *exactly* what `compose` granted and makes no second authority
//! decision. There is one policy decision point (compose), never a second one
//! inside projection.
//!
//! ## The three-list projection rule (spec #209 D4, invariant I2)
//!
//! - `allowed_actions`           -> presented as callable
//!   ([`CatalogEntryStatus::Callable`]).
//! - `approval_required_actions` -> presented, annotated *"requires owner
//!   approval"* ([`CatalogEntryStatus::RequiresOwnerApproval`]) so the assistant
//!   can still *propose* them; the existing gate/approval flow handles the pause.
//! - `denied_actions` **and any action not in the grant at all** -> structurally
//!   absent: no name, no description, no schema is ever emitted.
//!
//! **Structural absence is attenuation; `gate()` is the sole enforcement**
//! (spec #209, verbatim owner wording). A tool's absence from the catalog is
//! defense-in-depth *before* inference; it is never an enforcement mechanism.
//! `gate()` (`openspine-gate`, exact-match, fail-closed) remains the single
//! mandatory refusal path for any action a worker attempts.
//!
//! A granted action whose id carries no [`ToolDescriptor`] in the catalog is
//! **omitted** (a capability gap, not a security hole — the gate still enforces
//! it if attempted). Catalog completeness is a fail-closed gate test in the
//! kernel (spec #209 D3), not a concern of this pure projection.

use openspine_schemas::action::{ActionCatalog, ActionId, ToolDescriptor};
use openspine_schemas::grant::TaskGrant;

/// Whether a projected tool is directly callable or proposable-pending-approval.
///
/// Deliberately two variants, not a reuse of `GateDecision` (four variants):
/// a projection only ever emits *present* entries, and a present entry is only
/// ever callable or approval-required. `Deny`/`EffectSuppressed` can never
/// appear in a projection, so they are not representable here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogEntryStatus {
    /// The action is in the grant's `allowed_actions`: directly callable.
    Callable,
    /// The action is in the grant's `approval_required_actions`: proposable,
    /// but its effect pauses for owner approval via the existing gate/approval
    /// flow. Aligns with `GateDecision::ApprovalRequired`.
    RequiresOwnerApproval,
}

/// One projected tool: the action id it invokes, the kernel-owned model-facing
/// [`ToolDescriptor`], and its projection [`CatalogEntryStatus`].
///
/// The `action_id` is the catalog map key that paired the descriptor into this
/// view; a worker needs it to actually invoke the tool (the descriptor itself
/// deliberately carries no id — see `catalog_metadata.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub action_id: ActionId,
    pub descriptor: ToolDescriptor,
    pub status: CatalogEntryStatus,
}

/// The projected, ordered model-consumable tool surface for one grant.
///
/// Ordering is deterministic and grant-derived: `allowed_actions` in grant
/// order (each `Callable`), then `approval_required_actions` in grant order
/// (each `RequiresOwnerApproval`). No sorting, no comparison logic — the grant's
/// `Vec<ActionId>` order is already deterministic, which is all the purity
/// invariant requires.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CatalogView {
    entries: Vec<CatalogEntry>,
}

impl CatalogView {
    /// The projected entries, in projection order.
    pub fn entries(&self) -> &[CatalogEntry] {
        &self.entries
    }

    /// How many tools the view projects.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the view projects no tools at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Project the model-consumable tool catalog for a composed grant (spec #209,
/// IT2). Pure and deterministic: a function of `(grant, catalog)` alone, with
/// no I/O, no state, no clock, and no second authority decision.
///
/// See the module docs for the three-list rule. `denied_actions` is never read;
/// any id absent from both allow lists is absent from the view by construction.
pub fn project_catalog(grant: &TaskGrant, catalog: &ActionCatalog) -> CatalogView {
    let mut entries = Vec::new();
    project_list(
        &grant.allowed_actions,
        CatalogEntryStatus::Callable,
        catalog,
        &mut entries,
    );
    project_list(
        &grant.approval_required_actions,
        CatalogEntryStatus::RequiresOwnerApproval,
        catalog,
        &mut entries,
    );
    CatalogView { entries }
}

/// Project one grant list in order: emit an entry for every id that carries a
/// catalog descriptor; omit any id that does not. No policy, no ordering beyond
/// the list's own order.
fn project_list(
    ids: &[ActionId],
    status: CatalogEntryStatus,
    catalog: &ActionCatalog,
    entries: &mut Vec<CatalogEntry>,
) {
    for id in ids {
        if let Some(descriptor) = catalog.tool_descriptor_for(id) {
            entries.push(CatalogEntry {
                action_id: id.clone(),
                descriptor: descriptor.clone(),
                status,
            });
        }
    }
}

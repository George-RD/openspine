//! Integration tests for `project_catalog` — the pure, deterministic
//! capability-derived tool catalog projection (spec #209, IT2). Lives here
//! (not inline in `src/project_catalog.rs`) per the crate's 500-line-per-file
//! convention: these tests exercise only the crate's public API. Fixture
//! builders (`projection_grant`, `projection_catalog`, `tool_descriptor`) live
//! in `tests/common/mod.rs`.
//!
//! These tests assert external behavior only (spec #209 testing decisions):
//! given a composed grant and a catalog, does the projection emit exactly the
//! granted tools and nothing for denied/ungranted ids? They never assert
//! internal call sequencing, and none touches `compose()`, `TaskViewBody`, or
//! the shell.

#[allow(dead_code)]
mod common;

use common::*;
use openspine_authority::{project_catalog, CatalogEntryStatus, CatalogView};
use openspine_schemas::action::ActionId;

fn names(view: &CatalogView) -> Vec<String> {
    view.entries()
        .iter()
        .map(|e| e.descriptor.name.clone())
        .collect()
}

fn entry_for<'a>(
    view: &'a CatalogView,
    action_id: &str,
) -> Option<&'a openspine_authority::CatalogEntry> {
    let id = ActionId::new(action_id);
    view.entries().iter().find(|e| e.action_id == id)
}

/// The three-list projection rule (spec #209 D4): allowed -> present & callable;
/// approval-required -> present & flagged; denied -> absent; ungranted -> absent.
#[test]
fn three_list_projection_rule() {
    let grant = projection_grant(
        vec![ActionId::new("openspine.status.read")],
        vec![ActionId::new("connector.enable")],
        vec![ActionId::new("email.send")],
    );
    let view = project_catalog(&grant, &projection_catalog());

    // allowed -> present and Callable.
    let allowed = entry_for(&view, "openspine.status.read").expect("allowed id present");
    assert_eq!(allowed.status, CatalogEntryStatus::Callable);
    assert_eq!(allowed.descriptor.name, "read_status");

    // approval-required -> present and RequiresOwnerApproval.
    let approval = entry_for(&view, "connector.enable").expect("approval id present");
    assert_eq!(approval.status, CatalogEntryStatus::RequiresOwnerApproval);
    assert_eq!(approval.descriptor.name, "enable_connector");

    // denied -> absent (even though the catalog carries its descriptor).
    assert!(
        entry_for(&view, "email.send").is_none(),
        "denied id must be structurally absent"
    );

    // ungranted (in no grant list) -> absent.
    assert!(
        entry_for(&view, "worker.commission").is_none(),
        "ungranted id must be structurally absent"
    );

    // Exactly the two granted-and-described ids, in grant order (allowed first).
    assert_eq!(names(&view), vec!["read_status", "enable_connector"]);
}

/// Ordering is deterministic and grant-derived: all `allowed_actions` in grant
/// order (Callable), then all `approval_required_actions` in grant order.
#[test]
fn entries_follow_grant_order_allowed_then_approval() {
    let grant = projection_grant(
        vec![
            ActionId::new("telegram.reply:owner_channel"),
            ActionId::new("openspine.status.read"),
        ],
        vec![
            ActionId::new("artifact.activate"),
            ActionId::new("connector.enable"),
        ],
        vec![],
    );
    let view = project_catalog(&grant, &projection_catalog());

    assert_eq!(
        names(&view),
        vec![
            "reply_owner",       // allowed[0]
            "read_status",       // allowed[1]
            "activate_artifact", // approval[0]
            "enable_connector",  // approval[1]
        ]
    );
    assert_eq!(view.entries()[0].status, CatalogEntryStatus::Callable);
    assert_eq!(view.entries()[1].status, CatalogEntryStatus::Callable);
    assert_eq!(
        view.entries()[2].status,
        CatalogEntryStatus::RequiresOwnerApproval
    );
    assert_eq!(
        view.entries()[3].status,
        CatalogEntryStatus::RequiresOwnerApproval
    );
}

/// A granted id lacking a catalog descriptor is omitted (spec #209 D3: a
/// capability gap, not a security hole; the gate still enforces it). Applies to
/// both the allowed and the approval-required list.
#[test]
fn granted_id_without_descriptor_is_omitted() {
    let grant = projection_grant(
        vec![
            ActionId::new("openspine.status.read"),
            ActionId::new("undescribed.action"),
        ],
        vec![ActionId::new("also.undescribed")],
        vec![],
    );
    let view = project_catalog(&grant, &projection_catalog());

    assert_eq!(names(&view), vec!["read_status"]);
    assert!(entry_for(&view, "undescribed.action").is_none());
    assert!(entry_for(&view, "also.undescribed").is_none());
}

/// Invariant I2 (spec #209): a stranger-shaped grant projects a catalog with no
/// name, description, or schema for any denied or ungranted privileged action —
/// asserted positively (the privileged id is *not present*). Absence is by
/// construction: the catalog carries descriptors for these ids, yet the grant
/// never lists them, so the projection never emits them.
#[test]
fn i2_privileged_actions_are_structurally_absent() {
    let privileged = ["email.send", "worker.commission"];
    let grant = projection_grant(
        // A minimal stranger-facing allow list.
        vec![ActionId::new("openspine.status.read")],
        vec![],
        // Some privileged ids denied; others (worker.commission) simply omitted.
        vec![ActionId::new("email.send")],
    );
    let view = project_catalog(&grant, &projection_catalog());

    for id in privileged {
        assert!(
            entry_for(&view, id).is_none(),
            "{id} must have no entry in the projected catalog"
        );
    }
    // No emitted descriptor name matches a privileged tool's name.
    let emitted = names(&view);
    for forbidden in ["send_email", "commission_worker"] {
        assert!(
            !emitted.contains(&forbidden.to_string()),
            "{forbidden} name leaked into the projection"
        );
    }
    assert_eq!(emitted, vec!["read_status"]);
}

/// Projection is a pure function of `(grant, catalog)` (spec #209 D5): identical
/// inputs give identical outputs, and it introduces no id the grant did not
/// already carry in its allow or approval lists.
#[test]
fn projection_is_pure_and_introduces_no_new_ids() {
    let grant = projection_grant(
        vec![ActionId::new("openspine.status.read")],
        vec![ActionId::new("connector.enable")],
        vec![ActionId::new("email.send")],
    );
    let catalog = projection_catalog();

    let first = project_catalog(&grant, &catalog);
    let second = project_catalog(&grant, &catalog);
    assert_eq!(
        first, second,
        "identical inputs must give identical outputs"
    );

    // Every projected id was already carried by the grant's allow/approval lists.
    let carried: Vec<ActionId> = grant
        .allowed_actions
        .iter()
        .chain(grant.approval_required_actions.iter())
        .cloned()
        .collect();
    for entry in first.entries() {
        assert!(
            carried.contains(&entry.action_id),
            "projection introduced an id the grant never carried: {:?}",
            entry.action_id
        );
    }
}

/// An empty grant projects an empty catalog — no entries, not an error. Absence
/// is attenuation with no error channel (spec #209).
#[test]
fn empty_grant_projects_empty_catalog() {
    let grant = projection_grant(vec![], vec![], vec![]);
    let view = project_catalog(&grant, &projection_catalog());
    assert!(view.is_empty());
    assert_eq!(view.len(), 0);
}

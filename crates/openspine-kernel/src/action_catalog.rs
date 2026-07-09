//! The canonical catalog of known action ids (D-053).
//!
//! This is a curated kernel const, not derived from the fixtures: deriving
//! from fixtures would make a fixture typo self-legitimizing. Every id
//! referenced anywhere in `artifacts/lyra/` (agents / workflows / packs /
//! policies) plus the dispatch/stub ids the kernel actually mediates belongs
//! here; it is the review surface for "what actions exist".
//!
//! Composition ([`openspine_authority::compose_authority`]) and the gate
//! ([`openspine_gate::gate`]) both consult this catalog to fail-fast on an
//! id outside the universe.

use openspine_schemas::action::{ActionCatalog, ActionId};

fn id(s: &str) -> ActionId {
    ActionId::new(s)
}

/// Every action id the kernel recognizes, curated (D-053).
///
/// Verified against `artifacts/lyra/**` fixtures: each structured action
/// list (agent `designed_tools` / `approval_required_tools` / `denied_tools`,
/// workflow / pack / policy `candidate_allowed_actions` / `approval_required`
/// / `denied_actions`) names only ids present here. Includes the intentionally
/// unwired PRD ids (`route.activate`, `workflow.activate`,
/// `capability_pack.change`, `policy.change_proposal`, `connector.enable`)
/// so composition accepts them and the gate denies them only when ungranted.
pub fn canonical_catalog() -> ActionCatalog {
    ActionCatalog::new([
        // --- fixtures: main_assistant_agent ---
        id("openspine.status.read"),
        id("workflow.invoke:approved"),
        id("artifact.propose"),
        id("setup.workflow.start"),
        id("memory.read:owner_preferences_limited"),
        id("model.generate:approved_provider"),
        id("lyra.ui.preview"),
        id("telegram.reply:owner_channel"),
        id("connector.enable"),
        id("route.activate"),
        id("capability_pack.change"),
        id("workflow.activate"),
        id("policy.change_proposal"),
        // --- fixtures: main_assistant_agent denied_tools ---
        id("email.read_inbox"),
        id("email.read_thread:unselected"),
        id("email.send"),
        id("email.read_attachment"),
        id("network.raw_egress"),
        id("vault.secret_read"),
        id("policy.modify_direct"),
        id("filesystem.host_read"),
        id("filesystem.host_write"),
        id("coolify.deploy"),
        id("coolify.rollback"),
        id("coolify.secret_modify"),
        // --- fixtures: email_reply_drafter designed_tools ---
        id("email.read_thread:selected_no_attachments"),
        id("memory.read:writing_preferences_scoped"),
        id("artifact.write:task_scratch"),
        id("email.create_draft"),
        // --- fixtures: owner_control_basic_pack approval_required ---
        id("artifact.activate"),
        id("coolify.delete_resource"),
    ])
}

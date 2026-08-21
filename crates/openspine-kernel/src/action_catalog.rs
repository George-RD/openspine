//! The canonical catalog of known action ids (D-053).
//!
//! This is a curated kernel const, not derived from the fixtures: deriving
//! from fixtures would make a fixture typo self-legitimizing. Every id
//! referenced anywhere in `artifacts/lyra/` (agents / workflows / packs /
//! policies) plus the dispatch/stub ids the kernel actually mediates belongs
//! here; it is the review surface for "what actions exist".

use openspine_schemas::action::{ActionCatalog, ActionId, EffectPath, EffectPathClass};
use openspine_schemas::selection::SelectionTokenType;

#[path = "action_catalog_contracts.rs"]
mod action_catalog_contracts;
#[path = "action_catalog_data.rs"]
mod action_catalog_data;
#[path = "action_catalog_tool_descriptors.rs"]
mod action_catalog_tool_descriptors;
/// The canonical declaration of what a standing rule for an action must bind,
/// re-exported so activation-time scope-binding enforcement reads the same
/// descriptor table the catalog itself is assembled from.
pub(crate) use action_catalog_contracts::{
    dark_window_allow_eligible, required_scope_dimensions_for,
};
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
    let ids: &[&str] = &[
        "openspine.status.read",
        "workflow.invoke:approved",
        "artifact.propose",
        "plan.propose",
        "plan.execute",
        "setup.workflow.start",
        "secret.intake",
        "secret.rotate",
        "memory.read:owner_preferences_limited",
        "model.generate:approved_provider",
        "lyra.ui.preview",
        "telegram.reply:owner_channel",
        "terminal.reply:owner_device",
        "connector.enable",
        "route.activate",
        "capability_pack.change",
        "workflow.activate",
        "policy.change_proposal",
        "email.read_inbox",
        "email.read_thread:unselected",
        "email.send",
        "email.read_attachment",
        "network.raw_egress",
        "vault.secret_read",
        "policy.modify_direct",
        "filesystem.host_read",
        "filesystem.host_write",
        "coolify.deploy",
        "coolify.rollback",
        "coolify.secret_modify",
        "email.read_thread:selected_no_attachments",
        "memory.read:writing_preferences_scoped",
        "artifact.write:task_scratch",
        "email.create_draft",
        "artifact.activate",
        "artifact.revoke",
        "artifact.reconfirm",
        "artifact.nominate_upstream",
        "coolify.delete_resource",
        "owner.notify",
        "briefcase.topup",
        "web.search",
        "web.forum_browse",
        "web.form_submit",
        "worker.commission",
        "worker.report_result",
        "worker.failed",
        "skill.context",
        "openspine.overlay.export",
        "openspine.overlay.restore",
        "openspine.counterparty.erase",
    ];
    // Every catalog id receives a literal declaration. `None/None` is a
    // deliberate classification for non-egress actions, not an auto-default;
    // adding an id without adding its row is a review-visible completeness
    // failure and the gate fails closed on the missing entry.
    let decls = action_catalog_data::egress_declarations();
    ActionCatalog::new(ids.iter().map(|s| id(s)))
        .with_kernel_origin([id("owner.notify")])
        .with_non_delegable([
            id("openspine.overlay.export"),
            id("openspine.overlay.restore"),
            id("openspine.counterparty.erase"),
            id("email.send"),
        ])
        // Only a catalogued READ action with no kernel-side implementation
        // and no dedicated production route may return the stub; every write,
        // mutation, or effect id fails closed.
        // Deliberately excluded from this allowlist: artifact.write:task_scratch
        // is a write; model.generate:approved_provider has a dedicated
        // `/v1/model/generate` route that admits spend and stores artifacts;
        // briefcase.topup has a dedicated top-up route that mutates the
        // briefcase and is catalogued EffectPathClass::PostGateApprovedEffect.
        .with_non_effect_stub([
            id("memory.read:owner_preferences_limited"),
            id("memory.read:writing_preferences_scoped"),
            id("email.read_inbox"),
            id("email.read_thread:unselected"),
            id("email.read_attachment"),
            id("filesystem.host_read"),
            id("vault.secret_read"),
        ])
        .with_counterparty_facing([id("email.send")])
        // Actions that may carry a standing rule binding NO reviewed scope
        // (#133). Fail-closed allowlist: everything absent here must bind a
        // reviewed scope before a standing rule may admit it.
        //
        // `connector.enable` qualifies because such a rule narrows an
        // *approval requirement* - the owner pre-agreeing that enabling a
        // connector need not be confirmed each time. It reaches no connector,
        // writes nothing, communicates with nobody, and has no dispatchable
        // executor at all.
        //
        // `openspine.status.read` qualifies from the other direction: a read
        // of kernel status served by a registered handler, with no connector,
        // no write and no counterparty. The reflection miner mines repeated
        // approvals of it into a standing rule.
        //
        // An effectful action such as `email.send`, `network.raw_egress`,
        // `filesystem.host_write`, `secret.rotate`, `coolify.deploy` or
        // `policy.modify_direct` is deliberately absent: admitting one without
        // a reviewed scope is blanket reusable authority over an external
        // effect. Entries are added on demonstrated need, never speculatively.
        .with_approval_narrowing([id("connector.enable"), id("openspine.status.read")])
        .with_token_requiring([(
            id("email.read_thread:selected_no_attachments"),
            SelectionTokenType::email_thread_selection(),
        )])
        .with_egress_declarations(decls)
        .with_tool_descriptors(action_catalog_tool_descriptors::tool_descriptors())
        .with_delegation_descriptors(action_catalog_data::delegation_descriptors())
        .with_implementation_descriptors(action_catalog_data::implementation_descriptors())
        .with_effect_paths([
            EffectPath {
                name: "notify_owner_best_effort".to_string(),
                classification: EffectPathClass::KernelOriginGated,
            },
            EffectPath {
                name: "notify_owner_required".to_string(),
                classification: EffectPathClass::KernelOriginGated,
            },
            EffectPath {
                name: "create_approved_draft".to_string(),
                classification: EffectPathClass::PostGateApprovedEffect,
            },
            EffectPath {
                name: "activate_approved_artifact".to_string(),
                classification: EffectPathClass::PostGateApprovedEffect,
            },
            EffectPath {
                name: "revoke_standing_rule".to_string(),
                classification: EffectPathClass::PostGateApprovedEffect,
            },
            EffectPath {
                name: "resolve_email_counterparty".to_string(),
                classification: EffectPathClass::PreGateOwnerSelectedRead,
            },
            EffectPath {
                name: "briefcase.topup".to_string(),
                classification: EffectPathClass::PostGateApprovedEffect,
            },
            EffectPath {
                name: "dispatch_read_selected_thread".to_string(),
                classification: EffectPathClass::GatedShell,
            },
            EffectPath {
                name: "dispatch_lyra_preview/propose_draft_creation".to_string(),
                classification: EffectPathClass::GatedShell,
            },
            EffectPath {
                name: "dispatch_artifact_propose".to_string(),
                classification: EffectPathClass::GatedShell,
            },
            EffectPath {
                name: "run_model_swap_golden_set".to_string(),
                classification: EffectPathClass::GatedShell,
            },
            EffectPath {
                name: "apply_model_swap_activation".to_string(),
                classification: EffectPathClass::PostGateApprovedEffect,
            },
            EffectPath {
                name: "dispatch_plan_preview".to_string(),
                classification: EffectPathClass::GatedShell,
            },
            EffectPath {
                name: "resolve_approved_plan".to_string(),
                classification: EffectPathClass::PostGateApprovedEffect,
            },
            EffectPath {
                name: "secret_intake::capture".to_string(),
                classification: EffectPathClass::PostGateApprovedEffect,
            },
            EffectPath {
                name: "sweep_expired_grants".to_string(),
                classification: EffectPathClass::InternalMaintenanceNonEffect,
            },
            EffectPath {
                name: "answer_callback_query".to_string(),
                classification: EffectPathClass::InternalMaintenanceNonEffect,
            },
            EffectPath {
                name: "fire_due_workflow_timers".to_string(),
                classification: EffectPathClass::InternalMaintenanceNonEffect,
            },
            EffectPath {
                name: "dispatch_skill_context".to_string(),
                classification: EffectPathClass::GatedShell,
            },
            EffectPath {
                name: "dispatch_overlay_export".to_string(),
                classification: EffectPathClass::GatedShell,
            },
            EffectPath {
                name: "dispatch_overlay_restore".to_string(),
                classification: EffectPathClass::GatedShell,
            },
            EffectPath {
                name: "dispatch_erase_counterparty".to_string(),
                classification: EffectPathClass::GatedShell,
            },
        ])
}

#[cfg(test)]
#[path = "action_catalog_tests.rs"]
mod tests;

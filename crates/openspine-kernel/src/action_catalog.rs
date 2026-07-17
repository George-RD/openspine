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

use openspine_schemas::action::{ActionCatalog, ActionId, EffectPath, EffectPathClass};
use openspine_schemas::selection::SelectionTokenType;

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
        id("plan.propose"),
        id("plan.execute"),
        id("setup.workflow.start"),
        id("secret.intake"),
        id("secret.rotate"),
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
        id("artifact.reconfirm"),
        id("artifact.nominate_upstream"),
        id("coolify.delete_resource"),
        id("owner.notify"),
        // --- AD-060: egress-class rated web endpoints ---
        id("web.search"),
        id("web.forum_browse"),
        id("web.form_submit"),
    ])
    .with_kernel_origin([id("owner.notify")])
    .with_counterparty_facing([
        // email.send is the sole existing counterparty-facing action:
        // a denial faces the external recipient the worker was replying
        // to. Kernel-owned classification; shell cannot spoof (AD-151).
        id("email.send"),
    ])
    .with_token_requiring([(
        id("email.read_thread:selected_no_attachments"),
        SelectionTokenType::email_thread_selection(),
    )])
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
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_catalog_effect_paths_are_fully_enumerated_and_classified() {
        let catalog = canonical_catalog();
        let paths = catalog.effect_paths();
        assert_eq!(
            paths.len(),
            15,
            "Expected exactly 15 classified effect paths, got {:?}",
            paths
        );
        let path_names: Vec<&str> = paths.iter().map(|p| p.name.as_str()).collect();
        assert!(path_names.contains(&"notify_owner_best_effort"));
        assert!(path_names.contains(&"notify_owner_required"));
        // Characterization: notify_owner_required is a kernel-origin gated
        // effect, not a post-gate approved effect or shell dispatch.
        let required = paths
            .iter()
            .find(|p| p.name == "notify_owner_required")
            .expect("notify_owner_required must be in the catalog");
        assert_eq!(required.classification, EffectPathClass::KernelOriginGated);
        assert!(path_names.contains(&"create_approved_draft"));
        assert!(path_names.contains(&"activate_approved_artifact"));
        assert!(path_names.contains(&"dispatch_read_selected_thread"));
        assert!(path_names.contains(&"fire_due_workflow_timers"));
        assert!(path_names.contains(&"dispatch_lyra_preview/propose_draft_creation"));
        assert!(path_names.contains(&"dispatch_artifact_propose"));
        assert!(path_names.contains(&"run_model_swap_golden_set"));
        assert!(path_names.contains(&"apply_model_swap_activation"));
        assert!(path_names.contains(&"dispatch_plan_preview"));
        assert!(path_names.contains(&"resolve_approved_plan"));
        assert!(path_names.contains(&"secret_intake::capture"));
        assert!(path_names.contains(&"sweep_expired_grants"));
        assert!(path_names.contains(&"answer_callback_query"));
    }

    #[test]
    fn counterparty_classification_is_kernel_owned_and_fails_closed() {
        let catalog = canonical_catalog();
        assert!(catalog.is_counterparty_facing(&id("email.send")));
        assert!(!catalog.is_counterparty_facing(&id("telegram.reply:owner_channel")));
        assert!(!catalog.is_counterparty_facing(&id("unknown.future_action")));
    }
}

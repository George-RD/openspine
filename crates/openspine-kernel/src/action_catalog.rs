//! The canonical catalog of known action ids (D-053).
//!
//! This is a curated kernel const, not derived from the fixtures: deriving
//! from fixtures would make a fixture typo self-legitimizing. Every id
//! referenced anywhere in `artifacts/lyra/` (agents / workflows / packs /
//! policies) plus the dispatch/stub ids the kernel actually mediates belongs
//! here; it is the review surface for "what actions exist".

use openspine_schemas::action::{ActionCatalog, ActionId, EffectPath, EffectPathClass};
use openspine_schemas::selection::SelectionTokenType;

#[path = "action_catalog_data.rs"]
mod action_catalog_data;
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
    ];
    // Every catalog id receives a literal declaration. `None/None` is a
    // deliberate classification for non-egress actions, not an auto-default;
    // adding an id without adding its row is a review-visible completeness
    // failure and the gate fails closed on the missing entry.
    let decls = action_catalog_data::egress_declarations();
    ActionCatalog::new(ids.iter().map(|s| id(s)))
        .with_kernel_origin([id("owner.notify")])
        .with_counterparty_facing([id("email.send")])
        .with_token_requiring([(
            id("email.read_thread:selected_no_attachments"),
            SelectionTokenType::email_thread_selection(),
        )])
        .with_egress_declarations(decls)
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
        ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspine_schemas::egress::EgressClass;
    #[test]
    fn test_catalog_effect_paths_are_fully_enumerated_and_classified() {
        let catalog = canonical_catalog();
        let paths = catalog.effect_paths();
        assert_eq!(
            paths.len(),
            19,
            "Expected exactly 19 classified effect paths, got {:?}",
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
        assert!(path_names.contains(&"resolve_email_counterparty"));
        assert!(path_names.contains(&"briefcase.topup"));
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
        assert!(path_names.contains(&"dispatch_skill_context"));
    }

    #[test]
    fn counterparty_classification_is_kernel_owned_and_fails_closed() {
        let catalog = canonical_catalog();
        assert!(catalog.is_counterparty_facing(&id("email.send")));
        assert!(!catalog.is_counterparty_facing(&id("telegram.reply:owner_channel")));
        assert!(!catalog.is_counterparty_facing(&id("unknown.future_action")));
    }

    #[test]
    fn handler_registry_requires_explicit_classification() {
        // Enumerate the independent handler registry and require an explicit
        // catalog declaration plus exact axis values for every dispatchable
        // id. A side-set omission must fail this test, never default to
        // None/None silently.
        let catalog = canonical_catalog();
        let registry = crate::api::handler_registry::ActionHandlerRegistry::default_registrations();
        for action in registry.registered_action_ids() {
            let decl = catalog
                .egress_decl_for(&action)
                .unwrap_or_else(|| panic!("dispatchable action {action} lacks catalog entry"));
            let expected_channels: Option<Vec<&str>> = match action.as_str() {
                "telegram.reply:owner_channel"
                | "lyra.ui.preview"
                | "plan.propose"
                | "artifact.propose"
                | "artifact.nominate_upstream" => Some(vec!["telegram.owner.reply"]),
                _ => None,
            };
            let actual_channels = decl
                .output_channels
                .as_ref()
                .map(|channels| channels.iter().map(String::as_str).collect::<Vec<_>>());
            assert_eq!(
                actual_channels, expected_channels,
                "dispatchable action {action} has the wrong output-channel classification"
            );
            let expected_class = match action.as_str() {
                "web.search" => Some(EgressClass::Search),
                "web.forum_browse" => Some(EgressClass::ForumBrowse),
                "web.form_submit" => Some(EgressClass::WebFormPost),
                _ => None,
            };
            assert_eq!(
                decl.egress_class, expected_class,
                "dispatchable action {action} has the wrong egress classification"
            );
        }
    }

    #[test]
    fn worker_actions_declare_no_egress_and_no_output_channel() {
        // `worker.commission` / `worker.report_result` / `worker.failed` must
        // not be classified as egress endpoints or output-channel deliveries:
        // the worker can only ever report back via `worker.result` (AD-035
        // reply chokepoint), never egress directly.
        let catalog = canonical_catalog();
        for id in [
            ActionId::new("worker.commission"),
            ActionId::new("worker.report_result"),
            ActionId::new("worker.failed"),
        ] {
            let decl = catalog
                .egress_decl_for(&id)
                .expect("worker action declared");
            assert_eq!(
                decl.egress_class, None,
                "{id} must not be a rated egress endpoint"
            );
            assert_eq!(
                decl.output_channels, None,
                "{id} must not name an output channel"
            );
        }
    }
}

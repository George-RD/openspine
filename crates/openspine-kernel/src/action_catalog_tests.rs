//! Canonical action-catalog tests. Split from `action_catalog.rs` to keep
//! both files under the 500-line gate.

use super::*;
use openspine_schemas::action::{ActionImplementationId, DarkWindowPolicy, DelegationCatalogError};
use openspine_schemas::egress::EgressClass;
#[test]
fn test_catalog_effect_paths_are_fully_enumerated_and_classified() {
    let catalog = canonical_catalog();
    let paths = catalog.effect_paths();
    assert_eq!(
        paths.len(),
        21,
        "Expected exactly 21 classified effect paths, got {:?}",
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
    assert!(path_names.contains(&"dispatch_overlay_export"));
    assert!(path_names.contains(&"dispatch_overlay_restore"));
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
            "terminal.reply:owner_device" => Some(vec!["terminal.owner.reply"]),
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
fn every_dispatchable_action_has_a_tool_descriptor() {
    // Fail-closed completeness (spec #209 D3): a granted, dispatchable action
    // that lacks a tool descriptor is a capability gap a human must
    // consciously accept, so it fails here rather than silently omitting.
    // Mirrors `handler_registry_requires_explicit_classification`.
    let catalog = canonical_catalog();
    let registry = crate::api::handler_registry::ActionHandlerRegistry::default_registrations();
    for action in registry.registered_action_ids() {
        let descriptor = catalog
            .tool_descriptor_for(&action)
            .unwrap_or_else(|| panic!("dispatchable action {action} lacks a tool descriptor"));
        // The presentation flag must stay honest against the catalog's own
        // selection-token axis: a descriptor may not claim a different
        // token requirement than the action actually carries.
        assert_eq!(
            descriptor.selection_token_required,
            catalog.requires_selection_token(&action).is_some(),
            "tool descriptor for {action} has a selection_token_required flag \
             that disagrees with the catalog"
        );
    }
    // Cardinality, not just membership: pins the dispatchable descriptor count
    // so a deliberately-removed descriptor (or a stray extra one) fails, not
    // just a lookup miss. 15 is the current dispatchable set; changing it is a
    // conscious edit here.
    assert_eq!(
        catalog.tool_descriptor_count(),
        15,
        "expected exactly 15 dispatchable tool descriptors"
    );
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

#[test]
fn every_delegation_descriptor_names_a_catalogued_action() {
    let catalog = canonical_catalog();
    for descriptor in action_catalog_data::delegation_descriptors() {
        assert!(
            catalog.contains(&descriptor.action_id),
            "delegation descriptor names uncatalogued action {}",
            descriptor.action_id
        );
    }
}

#[test]
fn email_draft_has_a_reviewed_descriptor_and_a_registered_implementation() {
    let catalog = canonical_catalog();
    let descriptor = catalog
        .delegation_descriptor_for(&id("email.create_draft"))
        .expect("email draft delegation semantics must be catalog-owned");
    assert!(descriptor.reusable_delegation);
    assert!(matches!(
        descriptor
            .delegation_policy
            .as_ref()
            .expect("delegation policy")
            .dark_window_policy,
        DarkWindowPolicy::Prohibited
    ));

    let implementation_id = ActionImplementationId::new("gmail.draft.v1");
    let (_, implementation) = catalog
        .validated_delegation_contract(&id("email.create_draft"), &implementation_id)
        .expect("registered Gmail draft implementation must validate");
    assert_eq!(implementation.executor_id, "gmail.create_draft");
    assert_eq!(implementation.connector_kind, "gmail");
    assert_eq!(implementation.resolver_id, "gmail.thread_recipient");
    assert!(catalog
        .implementation_descriptor_for_action(&id("email.create_draft"))
        .is_some());
    assert!(catalog
        .implementation_descriptor_for_action(&id("email.send"))
        .is_none());

    let unknown_implementation_id = ActionImplementationId::new("gmail.unknown.v1");
    assert!(matches!(
        catalog
            .validated_delegation_contract(&id("email.create_draft"), &unknown_implementation_id),
        Err(DelegationCatalogError::MissingImplementationDescriptor { .. })
    ));
}

/// #135/D-162: the dark-window `Allow` eligibility allowlist is empty, and
/// emptiness is the whole security property — 28 test call sites take
/// their legacy-row branch because activation refuses, so if an action
/// ever became eligible those tests would silently switch to the `Ok(())`
/// branch and keep passing. Pinned here for the same reason, and in the
/// same shape, as the non-effect stub allowlist below.
#[test]
fn dark_window_allow_eligibility_allowlist_is_empty_and_fails_closed() {
    assert!(
        super::action_catalog_contracts::dark_window_allow_eligible_actions().is_empty(),
        "no action may be dark-window Allow eligible without an explicit catalog decision \
         and proposal-specific proof (#133)"
    );
    // An id the catalog has never heard of is ineligible, not unclassified.
    assert!(!super::dark_window_allow_eligible(&id(
        "unknown.future_action"
    )));
    // Connector writes and communication stay ineligible by construction.
    for action in [
        "email.send",
        "email.create_draft",
        "coolify.deploy",
        "filesystem.host_write",
        "secret.rotate",
        "network.raw_egress",
        "policy.modify_direct",
    ] {
        assert!(
            !super::dark_window_allow_eligible(&id(action)),
            "{action} must never be dark-window Allow eligible"
        );
    }
}

#[test]
fn non_effect_stub_allowlist_is_explicit_and_fails_closed() {
    let catalog = canonical_catalog();
    let allowlisted = [
        "memory.read:owner_preferences_limited",
        "memory.read:writing_preferences_scoped",
        "email.read_inbox",
        "email.read_thread:unselected",
        "email.read_attachment",
        "filesystem.host_read",
        "vault.secret_read",
    ];
    // Cardinality, not just membership: without this an eighth id could be
    // added to the allowlist and every positive/negative loop below would
    // still pass. The "exactly seven catalogued READ ids" guarantee is a
    // #127 boundary this change re-asserts, so pin the count.
    assert_eq!(
        catalog.non_effect_stub_count(),
        allowlisted.len(),
        "the non-effect stub allowlist must stay at exactly {} ids",
        allowlisted.len()
    );
    for action in allowlisted {
        let action = id(action);
        assert!(catalog.contains(&action), "{action} must be catalogued");
        assert!(
            catalog.is_non_effect_stub(&action),
            "{action} must be an explicit non-effect stub"
        );
    }

    for action in [
        "email.send",
        "coolify.deploy",
        "briefcase.topup",
        "artifact.write:task_scratch",
        "model.generate:approved_provider",
        "filesystem.host_write",
        "secret.rotate",
        "policy.modify_direct",
        "connector.enable",
        "web.form_submit",
        "unknown.future_action",
    ] {
        assert!(
            !catalog.is_non_effect_stub(&id(action)),
            "{action} must fail closed rather than return a stub"
        );
    }
}

#[test]
fn overlay_export_restore_are_non_delegable_with_no_egress() {
    let catalog = canonical_catalog();
    for id in [
        ActionId::new("openspine.overlay.export"),
        ActionId::new("openspine.overlay.restore"),
    ] {
        assert!(catalog.contains(&id), "{id} must be catalogued");
        assert!(
            catalog.is_non_delegable(&id),
            "{id} must be catalogued non-delegable"
        );
        let decl = catalog
            .egress_decl_for(&id)
            .expect("overlay action declared");
        assert_eq!(decl.egress_class, None, "{id} must not be rated egress");
        assert_eq!(
            decl.output_channels, None,
            "{id} must not name an output channel"
        );
    }
}
#[test]
fn non_delegable_catalog_is_exactly_the_root_only_set() {
    let catalog = canonical_catalog();
    let expected = [
        "openspine.overlay.export",
        "openspine.overlay.restore",
        "email.send",
    ];
    assert_eq!(catalog.non_delegable_count(), expected.len());
    for action in expected {
        assert!(catalog.is_non_delegable(&ActionId::new(action)));
    }
}

#[test]
fn approval_narrowing_allowlist_is_explicit_and_fails_closed() {
    let catalog = canonical_catalog();
    // The licence to carry a standing rule with NO reviewed scope, i.e. to hold
    // blanket reusable authority over an action. Mirrors the non-effect stub
    // guard in this file, for a strictly more dangerous set.
    let allowlisted = ["connector.enable", "openspine.status.read"];
    // Cardinality, not just membership: without this a third id could be added
    // and every loop below would still pass, so "entries are added on
    // demonstrated need" would be a policy nothing enforces.
    assert_eq!(
        catalog.approval_narrowing_count(),
        allowlisted.len(),
        "the approval-narrowing allowlist must stay at exactly {} ids",
        allowlisted.len()
    );
    for action in allowlisted {
        assert!(
            catalog.is_approval_narrowing(&id(action)),
            "{action} must remain eligible to narrow an approval"
        );
    }
    // Effectful actions: each is catalogued, reaches a connector, writes, or
    // communicates, and carries no reusable-delegation descriptor. An unscoped
    // standing rule for any of them is blanket authority over an external
    // effect.
    for action in [
        "email.send",
        "secret.rotate",
        "filesystem.host_write",
        "network.raw_egress",
        "coolify.deploy",
        "policy.modify_direct",
        "unknown.future_action",
    ] {
        assert!(
            !catalog.is_approval_narrowing(&id(action)),
            "{action} must never carry blanket reusable authority"
        );
    }
}

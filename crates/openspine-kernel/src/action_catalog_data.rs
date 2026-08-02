use std::collections::BTreeSet;

use openspine_schemas::action::{
    ActionDescriptor, ActionEgressDeclaration, ActionId, ActionSemantics, BudgetWindowBounds,
    DarkWindowPolicy, DataDestination, DelegationDefaults, DelegationPolicyBounds,
    DelegationProposalMode, EffectKind, EffectReversibility, ReviewedScopeDimension,
};
use openspine_schemas::egress::EgressClass;
use openspine_schemas::standing_rule::BudgetWindow;

fn id(s: &str) -> ActionId {
    ActionId::new(s)
}

/// Protocol-neutral semantics for actions that may eventually participate in
/// reusable delegation. A descriptor does not assert that a concrete
/// resolver/executor exists; that independent readiness axis lands in #127.
pub(crate) fn delegation_descriptors() -> Vec<ActionDescriptor> {
    vec![ActionDescriptor {
        schema_version: 1,
        descriptor_version: 1,
        action_id: id("email.create_draft"),
        semantics: ActionSemantics {
            owner_verb: "create".to_string(),
            owner_object: "email draft".to_string(),
            owner_target: "reviewed mailbox conversation".to_string(),
            effect_kind: EffectKind::OwnerAccountWrite,
            reversibility: EffectReversibility::Reversible,
            destination: DataDestination::OwnerCloudAccount,
        },
        reusable_delegation: true,
        required_scope_dimensions: BTreeSet::from([
            ReviewedScopeDimension::ConnectorImplementation,
            ReviewedScopeDimension::ConnectorInstance,
            ReviewedScopeDimension::AccountRole,
            ReviewedScopeDimension::AccountIdentity,
            ReviewedScopeDimension::Target,
            ReviewedScopeDimension::Counterparty,
            ReviewedScopeDimension::RelationshipTier,
            ReviewedScopeDimension::EffectDestination,
            ReviewedScopeDimension::Workflow,
            ReviewedScopeDimension::TaskShape,
        ]),
        delegation_policy: Some(DelegationPolicyBounds {
            schema_version: 1,
            policy_version: 1,
            quota: BudgetWindowBounds {
                minimum_max: 1,
                maximum_max: 20,
                minimum_window_secs: 60,
                maximum_window_secs: 30 * 24 * 3600,
            },
            rate: BudgetWindowBounds {
                minimum_max: 1,
                maximum_max: 5,
                minimum_window_secs: 60,
                maximum_window_secs: 24 * 3600,
            },
            maximum_lapse_secs: 90 * 24 * 3600,
            proposal_mode: DelegationProposalMode::DefaultsPermitted,
            defaults: Some(DelegationDefaults {
                quota: BudgetWindow {
                    max: 5,
                    window_secs: 7 * 24 * 3600,
                },
                rate: BudgetWindow {
                    max: 1,
                    window_secs: 3600,
                },
                expires_after_secs: 90 * 24 * 3600,
            }),
            dark_window_policy: DarkWindowPolicy::Prohibited,
            fresh_target_selection_required: true,
        }),
    }]
}

/// Explicit egress metadata for every canonical action. `None/None` is a
/// deliberate non-egress classification, never an implicit default.
pub(crate) fn egress_declarations() -> Vec<(ActionId, ActionEgressDeclaration)> {
    vec![
        (
            id("openspine.status.read"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("workflow.invoke:approved"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("artifact.propose"),
            ActionEgressDeclaration {
                output_channels: Some(vec!["telegram.owner.reply".to_string()]),
                egress_class: None,
            },
        ),
        (
            id("plan.propose"),
            ActionEgressDeclaration {
                output_channels: Some(vec!["telegram.owner.reply".to_string()]),
                egress_class: None,
            },
        ),
        (
            id("plan.execute"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("setup.workflow.start"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("secret.intake"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("secret.rotate"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("memory.read:owner_preferences_limited"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("model.generate:approved_provider"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("lyra.ui.preview"),
            ActionEgressDeclaration {
                output_channels: Some(vec!["telegram.owner.reply".to_string()]),
                egress_class: None,
            },
        ),
        (
            id("telegram.reply:owner_channel"),
            ActionEgressDeclaration {
                output_channels: Some(vec!["telegram.owner.reply".to_string()]),
                egress_class: None,
            },
        ),
        (
            id("terminal.reply:owner_device"),
            ActionEgressDeclaration {
                output_channels: Some(vec!["terminal.owner.reply".to_string()]),
                egress_class: None,
            },
        ),
        (
            id("connector.enable"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("route.activate"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("capability_pack.change"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("workflow.activate"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("policy.change_proposal"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("email.read_inbox"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("email.read_thread:unselected"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("email.send"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("email.read_attachment"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("network.raw_egress"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("vault.secret_read"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("policy.modify_direct"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("filesystem.host_read"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("filesystem.host_write"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("coolify.deploy"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("coolify.rollback"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("coolify.secret_modify"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("email.read_thread:selected_no_attachments"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("memory.read:writing_preferences_scoped"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("artifact.write:task_scratch"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("email.create_draft"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("artifact.activate"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("artifact.revoke"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("artifact.reconfirm"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("artifact.nominate_upstream"),
            ActionEgressDeclaration {
                output_channels: Some(vec!["telegram.owner.reply".to_string()]),
                egress_class: None,
            },
        ),
        (
            id("coolify.delete_resource"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("owner.notify"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("briefcase.topup"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("web.search"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: Some(EgressClass::Search),
            },
        ),
        (
            id("web.forum_browse"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: Some(EgressClass::ForumBrowse),
            },
        ),
        (
            id("web.form_submit"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: Some(EgressClass::WebFormPost),
            },
        ),
        (
            id("worker.commission"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("worker.report_result"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("worker.failed"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("skill.context"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("openspine.overlay.export"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
        (
            id("openspine.overlay.restore"),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: None,
            },
        ),
    ]
}

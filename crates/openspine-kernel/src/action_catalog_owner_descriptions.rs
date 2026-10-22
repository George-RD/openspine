//! Kernel-owned owner-facing descriptions for catalog actions that expose no
//! tool or delegation description (ANO-822).
//!
//! Package review must show the owner what each action a package names means.
//! Actions with a model tool schema or a delegation descriptor already carry
//! catalog-owned text; the rest get one fixed sentence here. This is a label for
//! review only: it adds no worker tool, delegation eligibility, executor or
//! permission interpretation, and an action absent from both sources still
//! blocks a review as missing metadata.
use openspine_schemas::action::ActionId;

pub(crate) fn owner_description_fallback(action: &ActionId) -> Option<&'static str> {
    Some(match action.as_str() {
        "artifact.activate" => "Activate an owner-approved artifact version.",
        "artifact.reconfirm" => {
            "Reconfirm a learned artifact after a base change; the owner's approval re-admits it."
        }
        "owner.notify" => "Send the owner a kernel-originated notification.",
        "secret.intake" => {
            "Capture a secret from the owner through the kernel's secret-intake flow."
        }
        "secret.rotate" => "Rotate a stored secret.",
        "web.form_submit" => "Submit a form on a website.",
        "web.forum_browse" => "Browse a web forum.",
        "web.search" => "Search the web.",
        "artifact.write:task_scratch" => "Write scratch output that belongs to the current task.",
        "briefcase.topup" => {
            "Top up a briefcase allowance; the kernel mutates the briefcase after the gate."
        }
        "capability_pack.change" => {
            "Change a capability pack; reserved, so it is denied unless granted."
        }
        "connector.enable" => "Enable a connector; reserved, so it is denied unless granted.",
        "coolify.delete_resource" => "Delete a resource on the deployment platform.",
        "coolify.deploy" => "Deploy to the deployment platform.",
        "coolify.rollback" => "Roll back a deployment on the deployment platform.",
        "coolify.secret_modify" => "Modify a secret on the deployment platform.",
        "email.read_attachment" => "Read an email attachment.",
        "email.read_inbox" => "Read the whole email inbox.",
        "email.read_thread:unselected" => "Read an email thread the owner has not selected.",
        "email.send" => "Send an email directly to a counterparty; never delegable.",
        "filesystem.host_read" => "Read files on the host filesystem.",
        "filesystem.host_write" => "Write files on the host filesystem.",
        "memory.read:owner_preferences_limited" => {
            "Read a limited set of owner preferences from memory."
        }
        "memory.read:writing_preferences_scoped" => "Read scoped writing preferences from memory.",
        "model.generate:approved_provider" => {
            "Generate text with an approved model provider; the kernel admits the spend."
        }
        "network.raw_egress" => "Open raw outbound network connections.",
        "plan.execute" => "Execute an owner-approved plan.",
        "policy.change_proposal" => {
            "Propose a policy change; reserved, so it is denied unless granted."
        }
        "policy.modify_direct" => "Modify a policy directly, outside the approval flow.",
        "route.activate" => "Activate a route; reserved, so it is denied unless granted.",
        "setup.workflow.start" => "Start the guided setup workflow.",
        "vault.secret_read" => "Read a secret from the vault.",
        "workflow.activate" => "Activate a workflow; reserved, so it is denied unless granted.",
        "workflow.invoke:approved" => "Invoke an owner-approved workflow.",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every catalog action must be describable to the owner by exactly one
    /// kernel-owned source, so a new action cannot silently weaken review.
    #[test]
    fn every_catalog_action_has_exactly_one_owner_description_source() {
        let catalog = crate::action_catalog::canonical_catalog();
        let mut ids: Vec<_> = catalog.action_ids().iter().cloned().collect();
        ids.sort();
        let mut violations = Vec::new();
        for action in ids {
            let catalog_text = catalog.tool_descriptor_for(&action).is_some()
                || catalog.delegation_descriptor_for(&action).is_some();
            if catalog_text == owner_description_fallback(&action).is_some() {
                violations.push(action.as_str().to_owned());
            }
        }
        assert!(
            violations.is_empty(),
            "each action needs catalog text xor a kernel fallback: {violations:?}"
        );
        assert!(owner_description_fallback(&ActionId::new("not.an.action")).is_none());
    }
}

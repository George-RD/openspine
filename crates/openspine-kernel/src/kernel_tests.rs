//! Kernel-level exhaustiveness test for the action catalog (D-053).
//!
//! Every action id named by a real `artifacts/lyra` fixture MUST be present in
//! the canonical catalog. This guards against the catalog drifting out of sync
//! with the fixtures: if a fixture introduces a new action id, this test fails
//! until `canonical_catalog` is updated to include it.

use openspine_schemas::action::ActionId;

use crate::action_catalog::canonical_catalog;
use crate::artifact_loader::load_registry;
use crate::test_support::fixtures::repo_lyra_dir;

#[test]
fn canonical_catalog_covers_all_fixture_action_ids() {
    let registry = load_registry(&repo_lyra_dir()).expect("the real Lyra fixtures must all parse");
    let catalog = canonical_catalog();

    // Collect every candidate action id named across the fixture registry.
    // Routes and PromptTemplates carry no action ids, so they are skipped.
    let mut all_ids: Vec<ActionId> = Vec::new();

    for agent in registry.agents.values() {
        all_ids.extend(agent.designed_tools.iter().cloned());
        all_ids.extend(agent.approval_required_tools.iter().cloned());
        all_ids.extend(agent.denied_tools.iter().cloned());
    }
    for workflow in registry.workflows.values() {
        all_ids.extend(workflow.candidate_allowed_actions.iter().cloned());
        all_ids.extend(workflow.approval_required.iter().cloned());
        all_ids.extend(workflow.denied_actions.iter().cloned());
    }
    for pack in registry.packs.values() {
        all_ids.extend(pack.candidate_allowed_actions.iter().cloned());
        all_ids.extend(pack.approval_required.iter().cloned());
        all_ids.extend(pack.denied_actions.iter().cloned());
    }
    for policy in registry.policies.values() {
        all_ids.extend(policy.candidate_allowed_actions.iter().cloned());
        all_ids.extend(policy.approval_required.iter().cloned());
        all_ids.extend(policy.denied_actions.iter().cloned());
    }

    let missing: Vec<ActionId> = all_ids
        .into_iter()
        .filter(|id| !catalog.contains(id))
        .collect();

    assert!(
        missing.is_empty(),
        "canonical_catalog is missing fixture action ids: {:?}",
        missing
    );
}

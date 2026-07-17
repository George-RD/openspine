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

#[test]
fn post_bind_clock_commit_persists_the_injected_clock_sample() {
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;

    let store = crate::store::Store::open_in_memory().unwrap();

    // A caller might otherwise capture a pre-setup sample and reuse it.
    let pre_setup = 1_000_000_i64;
    assert_eq!(
        store.validate_boot_clock(pre_setup).unwrap(),
        crate::store::BootClockCheck::Ok {
            high_water_ms: pre_setup
        }
    );

    // The exact post-bind sample `main` must commit.
    let expected = 2_000_000_i64;
    let calls = Arc::new(AtomicI64::new(0));
    let calls2 = calls.clone();
    let clock = move || {
        calls2.fetch_add(1, Ordering::SeqCst);
        expected
    };

    // Production path: `main` calls this only AFTER bind. The helper must
    // commit exactly the sample the closure returns, never `pre_setup`.
    crate::commit_post_bind_clock(&store, pre_setup, clock).unwrap();

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "clock closure must be sampled exactly once"
    );

    // Read the persisted high-water back without lowering it.
    let committed = match store.validate_boot_clock(i64::MIN).unwrap() {
        crate::store::BootClockCheck::Ok { high_water_ms }
        | crate::store::BootClockCheck::Regressed { high_water_ms, .. } => high_water_ms,
    };
    assert_eq!(
        committed, expected,
        "post-bind commit must persist the injected clock sample"
    );
    assert_ne!(
        committed, pre_setup,
        "post-bind commit must NOT reuse the pre-setup stale sample"
    );
}

#[test]
fn post_bind_clock_regression_refuses_without_persisting() {
    let store = crate::store::Store::open_in_memory().unwrap();
    let pre_setup = 1_000_000_i64;
    let regressed = pre_setup - 60_001;

    let error = crate::commit_post_bind_clock(&store, pre_setup, || regressed)
        .expect_err("post-bind regression must fail startup");
    assert!(
        error
            .to_string()
            .contains("wall clock regressed during startup"),
        "unexpected error: {error}"
    );
    assert_eq!(
        store.validate_boot_clock(regressed).unwrap(),
        crate::store::BootClockCheck::Ok {
            high_water_ms: regressed
        },
        "rejected post-bind sample must not be persisted"
    );
}

#[test]
fn tolerated_post_bind_regression_preserves_pre_setup_high_water() {
    let store = crate::store::Store::open_in_memory().unwrap();
    let pre_setup = 1_000_000_i64;
    let fresh = pre_setup - 60_000;

    crate::commit_post_bind_clock(&store, pre_setup, || fresh).unwrap();
    let check = store.validate_boot_clock(i64::MIN).unwrap();
    assert!(
        matches!(
            check,
            crate::store::BootClockCheck::Regressed {
                high_water_ms: 1_000_000,
                ..
            }
        ),
        "a tolerated post-bind regression must preserve the candidate high-water: {check:?}"
    );
}

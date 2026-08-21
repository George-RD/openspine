//! D-007 (the task grant is the final runtime authority) invariant tests.
//!
//! D-007: "the task grant is the only live authority object presented to a
//! running agent/workflow. Routes, agents, workflows, identity records, and
//! capability packs are *inputs* to authority." Those inputs never grant
//! authority on their own — a fresh grant is composed for each task, and once
//! any input is inactive no new grant mints from it. Grants are short-lived,
//! bounded by their `expires_at`.
//!
//! There is deliberately **no mid-flight revoke API** (spec #197 testing
//! decision D-007): a live grant is bounded only by (1) its expiry, (2) MAC
//! tamper — proven in `openspine-schemas` grant_chain tests — and (3) the
//! kernel's refusal to compose a new grant from a deactivated source, proven
//! here. Expiry denial at the gate itself is proven in `openspine-gate`
//! (`gate::tests::expired_grant_denies_even_an_allowed_action`); this file
//! stays at the pure composition + schema seam (no gate dependency, respecting
//! the schemas -> authority -> gate trust-boundary ordering).

#[allow(dead_code)]
mod common;

use common::*;

use openspine_authority::{compose_authority, AuthorityOutcome};
use openspine_schemas::artifact::Lifecycle;

/// D-007: no new grant mints from a deactivated backing source. Each of the
/// five composition inputs — route, agent, workflow, capability pack, global
/// policy — independently gates minting: dropping any one out of `Active`
/// yields a denial, never a grant. The inputs are not authority; only the
/// composed grant is, and it cannot be minted from a dead input.
#[test]
fn no_grant_mints_from_any_deactivated_source() {
    let now = jiff::Timestamp::now();

    // Baseline: with every source Active a grant composes.
    {
        let (event, identity, route, agent, workflow, pack, policy, session) = (
            owner_event(),
            owner_identity(),
            owner_route(),
            main_assistant_agent(),
            owner_control_conversation_workflow(),
            owner_control_basic_pack(),
            global_policy(),
            empty_session_policy(),
        );
        let input = owner_control_input(
            &event, &identity, &route, &agent, &workflow, &pack, &policy, &session,
        );
        assert!(
            matches!(
                compose_authority(&input, &test_catalog(), now),
                AuthorityOutcome::Granted(_)
            ),
            "all-active sources must compose a grant"
        );
    }

    // route deactivated.
    {
        let mut route = owner_route();
        route.lifecycle_state = Lifecycle::Quarantined;
        let (event, identity, agent, workflow, pack, policy, session) = (
            owner_event(),
            owner_identity(),
            main_assistant_agent(),
            owner_control_conversation_workflow(),
            owner_control_basic_pack(),
            global_policy(),
            empty_session_policy(),
        );
        let input = owner_control_input(
            &event, &identity, &route, &agent, &workflow, &pack, &policy, &session,
        );
        assert!(
            matches!(
                compose_authority(&input, &test_catalog(), now),
                AuthorityOutcome::Denied { .. }
            ),
            "a deactivated route must not mint a grant (D-007)"
        );
    }

    // agent deactivated.
    {
        let mut agent = main_assistant_agent();
        agent.lifecycle_state = Lifecycle::Quarantined;
        let (event, identity, route, workflow, pack, policy, session) = (
            owner_event(),
            owner_identity(),
            owner_route(),
            owner_control_conversation_workflow(),
            owner_control_basic_pack(),
            global_policy(),
            empty_session_policy(),
        );
        let input = owner_control_input(
            &event, &identity, &route, &agent, &workflow, &pack, &policy, &session,
        );
        assert!(
            matches!(
                compose_authority(&input, &test_catalog(), now),
                AuthorityOutcome::Denied { .. }
            ),
            "a deactivated agent must not mint a grant (D-007)"
        );
    }

    // workflow deactivated.
    {
        let mut workflow = owner_control_conversation_workflow();
        workflow.lifecycle_state = Lifecycle::Quarantined;
        let (event, identity, route, agent, pack, policy, session) = (
            owner_event(),
            owner_identity(),
            owner_route(),
            main_assistant_agent(),
            owner_control_basic_pack(),
            global_policy(),
            empty_session_policy(),
        );
        let input = owner_control_input(
            &event, &identity, &route, &agent, &workflow, &pack, &policy, &session,
        );
        assert!(
            matches!(
                compose_authority(&input, &test_catalog(), now),
                AuthorityOutcome::Denied { .. }
            ),
            "a deactivated workflow must not mint a grant (D-007)"
        );
    }

    // capability pack deactivated.
    {
        let mut pack = owner_control_basic_pack();
        pack.lifecycle_state = Lifecycle::Quarantined;
        let (event, identity, route, agent, workflow, policy, session) = (
            owner_event(),
            owner_identity(),
            owner_route(),
            main_assistant_agent(),
            owner_control_conversation_workflow(),
            global_policy(),
            empty_session_policy(),
        );
        let input = owner_control_input(
            &event, &identity, &route, &agent, &workflow, &pack, &policy, &session,
        );
        assert!(
            matches!(
                compose_authority(&input, &test_catalog(), now),
                AuthorityOutcome::Denied { .. }
            ),
            "a deactivated capability pack must not mint a grant (D-007)"
        );
    }

    // global policy deactivated.
    {
        let mut policy = global_policy();
        policy.lifecycle_state = Lifecycle::Quarantined;
        let (event, identity, route, agent, workflow, pack, session) = (
            owner_event(),
            owner_identity(),
            owner_route(),
            main_assistant_agent(),
            owner_control_conversation_workflow(),
            owner_control_basic_pack(),
            empty_session_policy(),
        );
        let input = owner_control_input(
            &event, &identity, &route, &agent, &workflow, &pack, &policy, &session,
        );
        assert!(
            matches!(
                compose_authority(&input, &test_catalog(), now),
                AuthorityOutcome::Denied { .. }
            ),
            "a deactivated global policy must not mint a grant (D-007)"
        );
    }
}

/// D-007: grants are short-lived. A composed grant's authority is bounded by
/// its `expires_at`; once past it the grant is expired and authorizes nothing
/// (the gate turns this into a `GrantExpired` denial — see the module note).
/// There is no separate live-revocation lever: expiry is the time bound.
#[test]
fn composed_grant_authority_is_bounded_by_expiry() {
    let now = jiff::Timestamp::now();
    let (event, identity, route, agent, workflow, pack, policy, session) = (
        owner_event(),
        owner_identity(),
        owner_route(),
        main_assistant_agent(),
        owner_control_conversation_workflow(),
        owner_control_basic_pack(),
        global_policy(),
        empty_session_policy(),
    );
    let input = owner_control_input(
        &event, &identity, &route, &agent, &workflow, &pack, &policy, &session,
    );
    let AuthorityOutcome::Granted(grant) = compose_authority(&input, &test_catalog(), now) else {
        panic!("expected a grant")
    };
    // The grant is time-boxed: expiry is issued_at + the composed runtime cap.
    assert_eq!(
        grant.expires_at,
        grant.issued_at + std::time::Duration::from_secs(grant.limits.max_runtime_seconds),
        "grant expiry must be its issue time plus the composed runtime cap (D-007: short-lived)"
    );
    assert!(
        !grant.is_expired(grant.issued_at),
        "a freshly issued grant is live"
    );
    assert!(
        grant.is_expired(grant.expires_at),
        "at expiry the grant authorizes nothing (D-007)"
    );
}

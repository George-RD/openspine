//! Integration tests for `compose_authority` — the eight named cases from
//! `openspec/changes/implement-authority-composition/tasks.md` plus the
//! D-034 regression. Lives here (not inline in `src/compose.rs`) per the
//! 500-line-per-file convention: these tests only exercise the crate's
//! public API, so they need no access to `compose.rs`'s private helpers.
//! Fixture builders live in `tests/common/mod.rs` (also split out for the
//! same size-gate reason).

mod common;

use common::*;
use openspine_authority::{compose_authority, AuthorityInput, AuthorityOutcome};
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::route::RouteEffect;

#[test]
fn owner_control_grant_matches_prd_12_1() {
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
    let outcome = compose_authority(&input, jiff::Timestamp::now());
    let AuthorityOutcome::Granted(grant) = outcome else {
        panic!("expected a grant")
    };
    assert!(grant
        .allowed_actions
        .contains(&ActionId::new("openspine.status.read")));
    assert!(grant
        .allowed_actions
        .contains(&ActionId::new("telegram.reply:owner_channel")));
    assert!(grant
        .approval_required_actions
        .contains(&ActionId::new("connector.enable")));
    // D-048: `artifact.activate` is the single canonical activation action
    // id (D-034 precedent) — added to `owner_control_basic_pack`'s
    // `approval_required` by `implement-artifact-lifecycle-slice`.
    assert!(grant
        .approval_required_actions
        .contains(&ActionId::new("artifact.activate")));
    assert!(grant
        .denied_actions
        .contains(&ActionId::new("email.read_inbox")));
    assert_eq!(grant.limits.max_runtime_seconds, 120);
    assert_eq!(grant.limits.max_model_calls, 8);
}

#[test]
fn no_candidate_allow_means_action_is_not_granted() {
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
    let AuthorityOutcome::Granted(grant) = compose_authority(&input, jiff::Timestamp::now()) else {
        panic!("expected a grant")
    };
    // email.read_inbox is not a candidate allow anywhere in this scenario.
    assert!(!grant
        .allowed_actions
        .contains(&ActionId::new("email.read_inbox")));
}

#[test]
fn explicit_deny_overrides_allow() {
    // Simulate a pack that (incorrectly) both allows and denies the same action.
    let mut pack = owner_control_basic_pack();
    pack.candidate_allowed_actions
        .push(ActionId::new("email.send"));
    pack.denied_actions.push(ActionId::new("email.send"));
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
    let AuthorityOutcome::Granted(grant) = compose_authority(&input, jiff::Timestamp::now()) else {
        panic!("expected a grant")
    };
    assert!(!grant.allowed_actions.contains(&ActionId::new("email.send")));
    assert!(grant.denied_actions.contains(&ActionId::new("email.send")));
}

#[test]
fn approval_required_overrides_plain_allow() {
    // openspine.status.read is a candidate allow on both the agent and the
    // pack; mark it approval-required on the pack too and prove
    // approval-required wins over the plain allow from elsewhere.
    let mut pack = owner_control_basic_pack();
    pack.approval_required
        .push(ActionId::new("openspine.status.read"));
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
    let AuthorityOutcome::Granted(grant) = compose_authority(&input, jiff::Timestamp::now()) else {
        panic!("expected a grant")
    };
    assert!(grant
        .approval_required_actions
        .contains(&ActionId::new("openspine.status.read")));
    assert!(!grant
        .allowed_actions
        .contains(&ActionId::new("openspine.status.read")));
}

#[test]
fn spoofed_owner_id_without_verified_source_is_denied() {
    let mut event = owner_event();
    event.verified_source = false;
    let identity = owner_identity();
    let (route, agent, workflow, pack, policy, session) = (
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
    let outcome = compose_authority(&input, jiff::Timestamp::now());
    assert!(
        matches!(outcome, AuthorityOutcome::Denied { .. }),
        "unverified source must never yield owner authority, got {outcome:?}"
    );
}

#[test]
fn main_assistant_grant_never_inherits_email_drafter_authority() {
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
    let AuthorityOutcome::Granted(grant) = compose_authority(&input, jiff::Timestamp::now()) else {
        panic!("expected a grant")
    };
    for email_only_action in [
        "email.read_thread:selected_no_attachments",
        "email.create_draft",
    ] {
        assert!(!grant
            .allowed_actions
            .iter()
            .any(|a| a.as_str() == email_only_action));
        assert!(!grant
            .approval_required_actions
            .iter()
            .any(|a| a.as_str() == email_only_action));
    }
}

#[test]
fn email_grant_excludes_inbox_wide_read_matching_prd_12_2() {
    let (event, identity, route, agent, workflow, pack, policy, session) = (
        email_event(),
        owner_identity(),
        email_route(),
        email_reply_drafter_agent(),
        selected_thread_email_reply_draft_workflow(),
        selected_thread_email_draft_pack(),
        global_policy(),
        empty_session_policy(),
    );
    let input = AuthorityInput {
        event: &event,
        identity: &identity,
        route: &route,
        global_policy: &policy,
        agent: &agent,
        workflow: &workflow,
        pack: &pack,
        session: &session,
        user: "owner",
        purpose: "draft_reply_for_selected_email_thread",
    };
    let AuthorityOutcome::Granted(grant) = compose_authority(&input, jiff::Timestamp::now()) else {
        panic!("expected a grant")
    };

    assert!(!grant
        .allowed_actions
        .iter()
        .any(|a| a.as_str() == "email.read_inbox"));
    assert!(!grant
        .allowed_actions
        .iter()
        .any(|a| a.as_str() == "email.read_thread:unselected"));
    assert!(grant
        .allowed_actions
        .iter()
        .any(|a| a.as_str() == "email.read_thread:selected_no_attachments"));

    // D-034 regression: PRD §12.2's ground truth — no create_draft variant
    // in allowed_actions, exactly `email.create_draft` (bare) in
    // approval_required_actions.
    assert!(!grant
        .allowed_actions
        .iter()
        .any(|a| a.as_str().starts_with("email.create_draft")));
    assert_eq!(
        grant
            .approval_required_actions
            .iter()
            .filter(|a| a.as_str().starts_with("email.create_draft"))
            .collect::<Vec<_>>(),
        vec![&ActionId::new("email.create_draft")]
    );
    assert_eq!(grant.limits.max_runtime_seconds, 180);
}

#[test]
fn widening_via_a_proposed_pack_requires_approval_first() {
    // "A proposed change adds inbox-wide read" (spec.md): a pack that
    // (hypothetically) allows email.read_inbox but is still `proposed`, not
    // `active`, must never be composed into a grant.
    let mut pack = owner_control_basic_pack();
    pack.lifecycle_state = Lifecycle::Proposed;
    pack.candidate_allowed_actions
        .push(ActionId::new("email.read_inbox"));
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
    let outcome = compose_authority(&input, jiff::Timestamp::now());
    assert!(
        matches!(outcome, AuthorityOutcome::Denied { .. }),
        "a proposed (not-yet-approved) pack must never be composed into a grant, got {outcome:?}"
    );
}

#[test]
fn quarantined_artifact_cannot_participate_in_a_grant() {
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
    assert!(matches!(
        compose_authority(&input, jiff::Timestamp::now()),
        AuthorityOutcome::Denied { .. }
    ));
}

#[test]
fn a_deny_route_is_never_composed() {
    let mut route = owner_route();
    route.effect = RouteEffect::Deny;
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
    assert!(matches!(
        compose_authority(&input, jiff::Timestamp::now()),
        AuthorityOutcome::Denied { .. }
    ));
}

#[test]
fn task_token_is_thirty_two_random_bytes_of_hex() {
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
    let AuthorityOutcome::Granted(grant) = compose_authority(&input, jiff::Timestamp::now()) else {
        panic!("expected a grant")
    };
    assert_eq!(grant.task_token.len(), 64);
    assert!(grant.task_token.bytes().all(|b| b.is_ascii_hexdigit()));

    let AuthorityOutcome::Granted(grant2) = compose_authority(&input, jiff::Timestamp::now())
    else {
        panic!("expected a grant")
    };
    assert_ne!(
        grant.task_token, grant2.task_token,
        "each grant must mint its own random token"
    );
}

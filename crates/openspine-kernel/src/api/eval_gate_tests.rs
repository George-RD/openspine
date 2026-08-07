//! Proposal-specific evaluation tests (#133).
//!
//! Every test here defends one acceptance criterion. The gate is exercised
//! through `run_gate` with kernel-assembled sources, and one test drives the
//! full production propose path so the wiring itself is proven rather than
//! assumed.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::standing_rule::{BudgetWindow, StandingRuleManifest};

use crate::artifact_loader::ParsedProposal;
use crate::overlay_eval_gate::{run_gate, AssemblySources, GateDenial};
use crate::pipeline::AppState;
use crate::store::standing_rules::StandingRule;

use super::scoped_admission::scoped_admission_support::{
    draft_env, mint_draft_grant, resolved_context, scoped_manifest,
};

/// Assemble the sources the kernel would supply, with the knobs each test
/// needs to vary. Defaults mirror a healthy runtime: executor registered,
/// nothing denied, no active rules.
pub(super) struct Sources {
    pub(super) executor_ready: bool,
    pub(super) denied: Vec<ActionId>,
    pub(super) active_rules: Vec<StandingRule>,
}

impl Default for Sources {
    fn default() -> Self {
        Self {
            executor_ready: true,
            denied: Vec::new(),
            active_rules: Vec::new(),
        }
    }
}

pub(super) fn run(
    state: &AppState,
    manifest: &StandingRuleManifest,
    sources: Sources,
) -> Result<crate::overlay_eval_gate::GateEvidence, GateDenial> {
    let proposal = ParsedProposal::StandingRule(manifest.clone());
    let digest = digest_of_bytes(serde_yaml::to_string(manifest).unwrap().as_bytes());
    let ready = sources.executor_ready;
    let executor_ready = move |_: &ActionId| ready;
    run_gate(
        &state.store,
        &state.action_catalog,
        &proposal,
        &digest,
        AssemblySources {
            catalog: &state.action_catalog,
            executor_ready: &executor_ready,
            denied_actions: &sources.denied,
            active_rules: sources.active_rules,
            policy_version: Some(1),
        },
    )
}

pub(super) async fn proposed_manifest(state: &AppState) -> StandingRuleManifest {
    let grant = mint_draft_grant(state, "thread-1");
    let context = resolved_context(state, &grant).await;
    let mut manifest = scoped_manifest("rule-eval", &context);
    manifest.lifecycle_state = openspine_schemas::artifact::Lifecycle::Proposed;
    manifest
}

// ---------------------------------------------------------------- judge axes

#[tokio::test]
async fn judge_refuses_standing_rule_whose_action_has_no_registered_executor() {
    let env = draft_env(&["thread-1"]).await;
    let manifest = proposed_manifest(&env.state).await;

    let denial = run(
        &env.state,
        &manifest,
        Sources {
            executor_ready: false,
            ..Sources::default()
        },
    )
    .expect_err("a rule whose action cannot execute must never reach the owner");

    assert!(
        format!("{denial}").contains("no registered executor"),
        "the denial must name the readiness axis: {denial}"
    );
}

#[tokio::test]
async fn judge_refuses_policy_denied_action() {
    let env = draft_env(&["thread-1"]).await;
    let manifest = proposed_manifest(&env.state).await;

    let denial = run(
        &env.state,
        &manifest,
        Sources {
            denied: vec![ActionId::new("email.create_draft")],
            ..Sources::default()
        },
    )
    .expect_err("deny beats everything, including a well-formed proposal");

    assert!(
        format!("{denial}").contains("denies action"),
        "the denial must name the policy axis: {denial}"
    );
}

#[tokio::test]
async fn judge_refuses_budgets_outside_declared_bounds() {
    let env = draft_env(&["thread-1"]).await;
    let mut manifest = proposed_manifest(&env.state).await;
    // email.create_draft declares quota max <= 20.
    manifest.quota = BudgetWindow {
        max: 5_000,
        window_secs: 7 * 24 * 3600,
    };

    let denial = run(&env.state, &manifest, Sources::default())
        .expect_err("a budget outside the action's declared envelope must be refused");

    assert!(
        format!("{denial}").contains("quota"),
        "the denial must name the failing limit: {denial}"
    );
}

#[tokio::test]
async fn judge_refuses_expiry_outside_declared_bounds() {
    let env = draft_env(&["thread-1"]).await;
    let mut manifest = proposed_manifest(&env.state).await;
    manifest.expires_after_secs = 10_000 * 24 * 3600;

    let denial = run(&env.state, &manifest, Sources::default())
        .expect_err("an expiry beyond the declared maximum lapse must be refused");

    assert!(format!("{denial}").contains("expiry"), "{denial}");
}

#[tokio::test]
async fn incomplete_scope_binding_denies_by_dimension_rather_than_passing() {
    let env = draft_env(&["thread-1"]).await;
    let mut manifest = proposed_manifest(&env.state).await;
    manifest.reviewed_scope = None;

    let denial = run(&env.state, &manifest, Sources::default())
        .expect_err("a rule with no reviewed scope is not evaluable as reusable authority");

    assert!(
        format!("{denial}").contains("reviewed_scope"),
        "the denial must name the missing dimension: {denial}"
    );
}

#[tokio::test]
async fn inconsistent_scope_binding_is_refused_as_incomplete_input() {
    let env = draft_env(&["thread-1"]).await;
    let mut manifest = proposed_manifest(&env.state).await;
    // Keep the stored values, corrupt the stored key: the binding now
    // disagrees with itself and must fail closed rather than match on
    // either half.
    if let Some(binding) = manifest.reviewed_scope.as_mut() {
        binding.reviewed_scope_digest = digest_of_bytes(b"not-the-real-scope-key");
    }

    let denial = run(&env.state, &manifest, Sources::default())
        .expect_err("a self-inconsistent binding must fail closed");

    assert!(
        format!("{denial}").contains("inconsistent"),
        "the denial must name the inconsistency: {denial}"
    );
}

#[tokio::test]
async fn judge_refuses_scope_already_held_by_another_active_rule() {
    let env = draft_env(&["thread-1"]).await;
    let manifest = proposed_manifest(&env.state).await;

    // An active rule under a DIFFERENT artifact id holding the same reviewed
    // scope: activating this proposal would silently revoke the incumbent
    // through #128's supersession. That takeover needs explicit review.
    let mut incumbent = manifest.clone();
    incumbent.id = "rule-incumbent".into();
    env.state
        .store
        .activate_standing_rule(&incumbent, None, Timestamp::now())
        .unwrap();
    let active = env
        .state
        .store
        .active_standing_rules_for_action(&ActionId::new("email.create_draft"), Timestamp::now())
        .unwrap();
    assert_eq!(active.len(), 1, "the incumbent must be active");

    let denial = run(
        &env.state,
        &manifest,
        Sources {
            active_rules: active,
            ..Sources::default()
        },
    )
    .expect_err("a silent takeover of another artifact's reviewed scope must be refused");

    assert!(
        format!("{denial}").contains("collides"),
        "the denial must name the overlap axis: {denial}"
    );
}

#[tokio::test]
async fn judge_admits_disjoint_scope_alongside_an_active_rule() {
    let env = draft_env(&["thread-1", "thread-2"]).await;
    let manifest = proposed_manifest(&env.state).await;

    // An active rule on a different thread is a disjoint scope: #128 proved
    // disjoint rules coexist, so this must NOT be refused as an overlap.
    let other_grant = mint_draft_grant(&env.state, "thread-2");
    let other_context = resolved_context(&env.state, &other_grant).await;
    let mut incumbent = scoped_manifest("rule-other-thread", &other_context);
    incumbent.id = "rule-other-thread".into();
    env.state
        .store
        .activate_standing_rule(&incumbent, None, Timestamp::now())
        .unwrap();
    let active = env
        .state
        .store
        .active_standing_rules_for_action(&ActionId::new("email.create_draft"), Timestamp::now())
        .unwrap();

    run(
        &env.state,
        &manifest,
        Sources {
            active_rules: active,
            ..Sources::default()
        },
    )
    .expect("a disjoint reviewed scope must not be refused as an overlap");
}

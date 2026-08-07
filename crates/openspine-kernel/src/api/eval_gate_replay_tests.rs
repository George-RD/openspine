//! Replay-ledger, summary and authority-boundary tests (#133). Split from
//! `eval_gate_tests.rs` to keep both files under the 500-line gate.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::standing_rule::{BudgetWindow, StandingRuleManifest};

use crate::overlay_eval_gate::{run_gate, AssemblySources};

use super::eval_gate_tests::{proposed_manifest, run, Sources};
use super::scoped_admission::scoped_admission_support::{draft_env, usage_count};

// ------------------------------------------------------------------- replay

#[tokio::test]
async fn replay_executes_matching_and_changed_context_cases() {
    let env = draft_env(&["thread-1"]).await;
    let manifest = proposed_manifest(&env.state).await;

    let evidence =
        run(&env.state, &manifest, Sources::default()).expect("a healthy proposal passes");
    let replay: serde_json::Value = serde_json::from_str(evidence.replay.evidence_json()).unwrap();

    assert_eq!(
        replay["evaluation"], "proposal-bound-executed-cases",
        "the evaluation must be named for what it did"
    );
    assert!(
        replay["cases_executed"].as_u64().unwrap() > 1,
        "replay must execute more than the baseline: {replay}"
    );
    assert!(
        replay["cases_matched"].as_u64().unwrap() >= 1,
        "the reviewed scope must admit itself"
    );
    assert!(
        replay["changed_context_cases_refused"].as_u64().unwrap() >= 1,
        "at least one changed-context case must be refused"
    );
    assert!(
        replay["ledger"].as_array().unwrap().len()
            == replay["cases_executed"].as_u64().unwrap() as usize,
        "the ledger must carry one entry per executed case"
    );
    assert!(
        evidence.replay.fitness().is_none(),
        "a required case class is pass/fail, never a score"
    );
}

#[tokio::test]
async fn mutated_dimension_cases_are_refused_and_name_the_dimension() {
    let env = draft_env(&["thread-1"]).await;
    let manifest = proposed_manifest(&env.state).await;

    let evidence = run(&env.state, &manifest, Sources::default()).unwrap();
    let replay: serde_json::Value = serde_json::from_str(evidence.replay.evidence_json()).unwrap();
    let ledger = replay["ledger"].as_array().unwrap();

    let mutations: Vec<&serde_json::Value> = ledger
        .iter()
        .filter(|case| case["kind"] == "dimension_mutation")
        .collect();
    assert!(
        !mutations.is_empty(),
        "changed-context cases must actually be generated"
    );
    for case in mutations {
        assert_eq!(
            case["observed"], "does_not_match",
            "a changed instance dimension must not match: {case}"
        );
        assert!(
            case["dimension"].as_str().is_some(),
            "every mutation case must name the dimension it changed: {case}"
        );
    }

    // The dimensions that distinguish accounts and targets must be among
    // those actually varied — otherwise the ledger proves less than it looks.
    let varied: Vec<&str> = replay["mutated_dimensions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    // Every bound INSTANCE dimension must be exercised. AccountRole and
    // RelationshipTier were silently uncovered by a `_` catch-all until the
    // reviewer caught it; assert them explicitly so a regression is loud.
    for required in [
        "Target",
        "TargetDigest",
        "AccountIdentity",
        "AccountRole",
        "RelationshipTier",
        "Counterparty",
        "ConnectorInstance",
        "Workflow",
        "TaskShape",
        "BoundParameters",
    ] {
        assert!(
            varied.contains(&required),
            "{required} must be exercised as a changed-context case; varied: {varied:?}"
        );
    }
}

// ------------------------------------------------------------------ summary

#[tokio::test]
async fn summary_reports_executed_case_counts_and_claims_no_more() {
    let env = draft_env(&["thread-1"]).await;
    let manifest = proposed_manifest(&env.state).await;

    let evidence = run(&env.state, &manifest, Sources::default()).unwrap();

    assert!(
        evidence
            .summary
            .contains("replayed this exact proposal against"),
        "the summary must state that cases ran: {}",
        evidence.summary
    );
    assert!(
        evidence
            .summary
            .contains("changed-context case(s) were refused"),
        "the summary must state the refusals: {}",
        evidence.summary
    );
    assert!(
        evidence.summary.contains("grants no authority"),
        "the summary must not imply approval: {}",
        evidence.summary
    );
}

#[tokio::test]
async fn availability_only_evaluation_makes_no_replay_claim() {
    // A route proposal has no reviewed scope: the owner-history check still
    // runs, but it is named for what it measures and claims no replay. This
    // drives the real production propose path end to end.
    use crate::pipeline::handle_owner_update;
    use crate::telegram::TelegramConnector;
    use crate::test_support::fixtures::{
        owner_update, seed_owner_history, test_state_with_telegram,
    };

    let telegram = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": {"message_id": 1, "chat": {"id": 42}, "date": 0}
            })),
        )
        .mount(&telegram)
        .await;
    let connector =
        TelegramConnector::with_api_url("t".to_string(), telegram.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    seed_owner_history(&state, &grant);

    let yaml = super::artifact_propose_tests::route_yaml("route-eval-copy", "proposed");
    let parsed = crate::artifact_loader::parse_proposal("route", &yaml).expect("valid route");
    let digest = digest_of_bytes(yaml.as_bytes());
    let executor_ready = |_: &ActionId| true;
    let evidence = run_gate(
        &state.store,
        &state.action_catalog,
        &parsed,
        &digest,
        AssemblySources {
            catalog: &state.action_catalog,
            executor_ready: &executor_ready,
            denied_actions: &[],
            active_rules: Vec::new(),
            policy_version: Some(1),
        },
    )
    .expect("a route proposal with owner history passes");

    assert!(
        !evidence.summary.to_lowercase().contains("replay"),
        "copy must not claim a replay when no cases ran: {}",
        evidence.summary
    );
    assert!(
        evidence
            .summary
            .contains("owner-control-history-availability"),
        "the check must be named for what it measures: {}",
        evidence.summary
    );
}

// -------------------------------------------------------- authority boundary

#[tokio::test]
async fn passing_evaluation_grants_no_authority() {
    let env = draft_env(&["thread-1"]).await;
    let manifest = proposed_manifest(&env.state).await;

    run(&env.state, &manifest, Sources::default()).expect("a healthy proposal passes");

    assert!(
        env.state
            .store
            .active_standing_rules_for_action(
                &ActionId::new("email.create_draft"),
                Timestamp::now()
            )
            .unwrap()
            .is_empty(),
        "evaluation must not activate a rule"
    );
    assert_eq!(
        usage_count(&env.state, "rule-eval", "reserved"),
        0,
        "evaluation must reserve no budget"
    );
    assert_eq!(
        env.state.store.count_pending_draft_writes().unwrap(),
        0,
        "evaluation must dispatch no connector effect"
    );
}

// ----------------------------------------------- non-scope-bound authority

/// B1 regression. Before this, a standing rule for any action without a
/// reusable-delegation descriptor — i.e. every catalogued action but one —
/// took the catalog-membership arm and was never checked for delegability,
/// executor readiness, or policy deny. The reviewer's probe proposed a rule
/// for a non-delegable, unexecutable, policy-denied action with absurd
/// budgets and it passed both evaluators.
#[tokio::test]
async fn non_delegable_action_cannot_carry_a_standing_rule() {
    let env = draft_env(&["thread-1"]).await;
    let manifest = probe_manifest("openspine.overlay.export");

    let denial = run(
        &env.state,
        &manifest,
        Sources {
            executor_ready: false,
            ..Sources::default()
        },
    )
    .expect_err("a non-delegable action must never carry reusable authority");

    assert!(
        format!("{denial}").contains("not declared reusable-delegatable"),
        "the denial must name delegability: {denial}"
    );
}

#[tokio::test]
async fn policy_denied_non_scope_bound_rule_is_refused() {
    let env = draft_env(&["thread-1"]).await;
    let manifest = probe_manifest("connector.enable");

    let denial = run(
        &env.state,
        &manifest,
        Sources {
            denied: vec![ActionId::new("connector.enable")],
            ..Sources::default()
        },
    )
    .expect_err("deny must beat a non-scope-bound rule too");

    assert!(format!("{denial}").contains("denies action"), "{denial}");
}

/// A standing rule with absurd limits for an action that declares no bounds.
pub(super) fn probe_manifest(action: &str) -> StandingRuleManifest {
    let mut manifest = crate::store::standing_rules_tests::manifest(
        "probe-rule",
        action,
        100 * 365 * 24 * 3600,
        BudgetWindow {
            max: 1_000_000,
            window_secs: 60,
        },
        BudgetWindow {
            max: 1_000_000,
            window_secs: 60,
        },
        None,
    );
    manifest.lifecycle_state = openspine_schemas::artifact::Lifecycle::Proposed;
    manifest
}

/// F1 regression (reviewer Probe B). Routing every standing rule to the
/// reusable-authority arm made `catalog_structural_arm`'s `UnknownAction`
/// raise unreachable, so a rule naming an id nobody declared passed: every
/// other axis reads catalog metadata, and an unknown id is absent from all of
/// it. This was a strict loss against `origin/main`.
#[tokio::test]
async fn uncatalogued_action_cannot_carry_a_standing_rule() {
    let env = draft_env(&["thread-1"]).await;
    let manifest = probe_manifest("totally.not.a.real.action");

    let denial = run(&env.state, &manifest, Sources::default())
        .expect_err("an action nobody declared must not carry reusable authority");

    assert!(
        format!("{denial}").contains("unknown action"),
        "the denial must name the unknown action: {denial}"
    );
}

/// F2 regression (reviewer Probe C). Treating "has no reviewed scope" as the
/// licence to skip the scope-bound axes was the same hole one layer down, and
/// it landed on exactly the effectful actions that matter: each of these is
/// catalogued, effectful, absent from the non-delegable set, and carries no
/// delegation descriptor. Unscoped and effectively unbounded, every one of
/// them passed.
#[tokio::test]
async fn effectful_actions_cannot_carry_an_unscoped_standing_rule() {
    let env = draft_env(&["thread-1"]).await;

    for action in [
        "email.send",
        "secret.rotate",
        "filesystem.host_write",
        "network.raw_egress",
        "coolify.deploy",
        "policy.modify_direct",
    ] {
        let manifest = probe_manifest(action);
        let denial = run(
            &env.state,
            &manifest,
            Sources {
                executor_ready: false,
                ..Sources::default()
            },
        )
        .expect_err(&format!(
            "{action} must not carry blanket reusable authority"
        ));
        let text = format!("{denial}");
        assert!(
            text.contains("MUST bind a reviewed scope")
                || text.contains("not declared reusable-delegatable"),
            "{action} must be refused for lacking a reviewed scope, got: {text}"
        );
    }
}

/// The counterpart: an approval-narrowing action legitimately carries an
/// unscoped rule. `connector.enable` narrows an approval requirement for an
/// action with no dispatchable executor at all, and breaking it would be the
/// over-correction this design deliberately avoids.
#[tokio::test]
async fn approval_narrowing_action_may_carry_an_unscoped_standing_rule() {
    use crate::pipeline::handle_owner_update;
    use crate::telegram::TelegramConnector;
    use crate::test_support::fixtures::{
        owner_update, seed_owner_history, test_state_with_telegram,
    };

    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": {"message_id": 1, "chat": {"id": 42}, "date": 0}
            })),
        )
        .mount(&server)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "t".to_string(),
        server.uri().parse().unwrap(),
    ));
    // The availability arm still applies to an unscoped rule: there is no
    // reviewed scope to vary, so replay measures owner-control history and is
    // reported under that name.
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    seed_owner_history(&state, &grant);

    let mut manifest = probe_manifest("connector.enable");
    manifest.quota = BudgetWindow {
        max: 3,
        window_secs: 3600,
    };
    manifest.rate = BudgetWindow {
        max: 2,
        window_secs: 3600,
    };
    manifest.expires_after_secs = 3600;

    let evidence = run(
        &state,
        &manifest,
        Sources {
            executor_ready: false,
            ..Sources::default()
        },
    )
    .expect("an approval-narrowing action may carry an unscoped rule");

    let judge: serde_json::Value = serde_json::from_str(evidence.judge.evidence_json()).unwrap();
    assert_eq!(judge["scope_bound"], false);
    assert!(
        judge["axes_passed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|axis| axis == "approval_narrowing_action"),
        "the pass must record which axis licensed it: {judge}"
    );
}

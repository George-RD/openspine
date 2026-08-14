//! Production-path evaluation tests (#133): these drive the real entry
//! points (`dispatch_artifact_propose`, `commit_artifact_activation`) rather
//! than calling the gate or the store directly. Split from
//! `eval_gate_tests.rs` to keep both files under the 500-line gate.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::standing_rule::StandingRuleManifest;

use super::scoped_admission::scoped_admission_support::{
    draft_env, mint_draft_grant, resolved_context, scoped_manifest,
};
use crate::pipeline::AppState;

use super::eval_gate_replay_tests::probe_manifest;

async fn proposed_manifest(state: &AppState) -> StandingRuleManifest {
    let grant = mint_draft_grant(state, "thread-1");
    let context = resolved_context(state, &grant).await;
    let mut manifest = scoped_manifest("rule-eval", &context);
    manifest.lifecycle_state = openspine_schemas::artifact::Lifecycle::Proposed;
    manifest
}

// ------------------------------------------- production propose path (M2)

/// Drives the REAL entry point `dispatch_artifact_propose`, so the
/// `AssemblySources` construction and the `eval.epochs` plumb-through inside
/// it are exercised rather than assumed. A unit test that calls `run_gate`
/// directly proves neither.
#[tokio::test]
async fn propose_path_refuses_a_non_delegable_standing_rule_before_review_required() {
    use super::artifact_propose::dispatch_artifact_propose;
    use super::dispatch_tests::OWNER_CHAT_ID;
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
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    seed_owner_history(&state, &grant);

    let rule = probe_manifest("openspine.overlay.export");
    let yaml = serde_yaml::to_string(&rule).unwrap();
    let result = dispatch_artifact_propose(
        &state,
        &grant,
        &ActionId::new("artifact.propose"),
        &crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID),
        Some(&serde_json::json!({"kind": "standing_rule", "yaml": yaml})),
    )
    .await;

    let err =
        result.expect_err("a non-delegable standing rule must not reach the approval surface");
    assert!(
        format!("{err:?}").contains("not declared reusable-delegatable"),
        "the production path must surface the delegability denial: {err:?}"
    );

    // Nothing reached review_required, and no verdict claims a pass.
    let states: Vec<String> = {
        let conn = state.store.conn.lock();
        let mut stmt = conn
            .prepare("SELECT state FROM proposed_artifacts WHERE artifact_id = ?1")
            .unwrap();
        let rows = stmt
            .query_map(rusqlite::params!["probe-rule"], |row| {
                row.get::<_, String>(0)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows
    };
    assert!(
        !states.iter().any(|s| s == "review_required"),
        "the proposal must never reach review_required: {states:?}"
    );
}

/// `email.send` is catalog-owned non-delegable data (#130). This drives the
/// same production propose path and proves the catalog-backed judge returns the
/// typed non-delegable refusal for it before any review surface exists. A
/// catalog-cardinality assertion proves the datum, not this reader.
#[tokio::test]
async fn catalog_email_send_is_non_delegable() {
    use super::artifact_propose::dispatch_artifact_propose;
    use super::dispatch_tests::OWNER_CHAT_ID;
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
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    seed_owner_history(&state, &grant);

    let rule = probe_manifest("email.send");
    let yaml = serde_yaml::to_string(&rule).unwrap();
    let result = dispatch_artifact_propose(
        &state,
        &grant,
        &ActionId::new("artifact.propose"),
        &crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID),
        Some(&serde_json::json!({"kind": "standing_rule", "yaml": yaml})),
    )
    .await;

    let err = result.expect_err("email.send must never reach the approval surface");
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("not declared reusable-delegatable") && rendered.contains("email.send"),
        "the production path must surface the catalog non-delegable denial: {rendered}"
    );

    let states: Vec<String> = {
        let conn = state.store.conn.lock();
        let mut stmt = conn
            .prepare("SELECT state FROM proposed_artifacts WHERE artifact_id = ?1")
            .unwrap();
        let rows = stmt
            .query_map(rusqlite::params!["probe-rule"], |row| {
                row.get::<_, String>(0)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows
    };
    assert!(
        !states.iter().any(|s| s == "review_required"),
        "the proposal must never reach review_required: {states:?}"
    );
}

/// Drives the REAL activation entry point `commit_artifact_activation`, so
/// the currency re-check is proven to be wired into activation rather than
/// merely callable on the store.
#[tokio::test]
async fn activation_path_refuses_a_stale_verdict() {
    use crate::store::activation::ActivationCommit;
    use crate::store::eval_verdict_store::{EvalVerdict, VerdictEpochs};
    use crate::store::learned_artifacts::{
        CompatibilityStatus, LearnedArtifact, NominationStatus, Provenance,
    };
    use openspine_schemas::artifact::ArtifactNamespace;

    let env = draft_env(&["thread-1"]).await;
    let manifest = proposed_manifest(&env.state).await;
    let binding = manifest.reviewed_scope.clone().unwrap();

    env.state
        .store
        .insert_eval_verdict(&EvalVerdict {
            id: ulid::Ulid::new(),
            artifact_kind: "standing_rule".to_string(),
            artifact_id: manifest.id.to_string(),
            artifact_version: manifest.version,
            verdict: "pass".to_string(),
            fitness: None,
            evidence: None,
            evaluator: Some("overlay-eval-gate/replay@v2".to_string()),
            artifact_digest: "sha256:".to_string() + &"0".repeat(64),
            recorded_at: Timestamp::now(),
            epochs: VerdictEpochs {
                compatibility_digest: Some(binding.compatibility_digest.as_str().to_string()),
                reviewed_scope_digest: Some(binding.reviewed_scope_digest.as_str().to_string()),
                descriptor_version: Some(1),
                ..VerdictEpochs::default()
            },
        })
        .unwrap();

    let commit = |descriptor_version: Option<u32>| ActivationCommit {
        learned: LearnedArtifact {
            kind: "standing_rule".to_string(),
            artifact_id: manifest.id.to_string(),
            version: manifest.version,
            namespace: ArtifactNamespace::Overlay,
            provenance: Provenance::ProducedBy {
                source_event_id: ulid::Ulid::new(),
                source_exchange: env.state.artifacts.put(b"eval-currency-probe").unwrap(),
                source_scope: crate::counterparty_keys::SYSTEM_SCOPE,
            },
            accepted_via: None,
            learned_at: Timestamp::now(),
            compatibility: CompatibilityStatus::Compatible,
            pending_yaml_digest: None,
            accepted_dependency_fingerprint: None,
            nomination: NominationStatus::None,
            pending_reconfirmation_id: None,
            source_path: None,
            accepted_base_epoch: None,
        },
        proposed_id: ulid::Ulid::new(),
        grant_id: None,
        payload_ref: None,
        dangling: false,
        superseded_old_version: None,
        standing_rule: Some((manifest.clone(), None)),
        owner_review_approval: None,
        live_descriptor_version: descriptor_version,
        live_implementation_version: None,
        live_policy_version: None,
    };

    // Descriptor revised since evaluation: activation must refuse.
    let refused = env
        .state
        .store
        .commit_artifact_activation(commit(Some(2)))
        .expect_err("activation must refuse a stale verdict");
    assert!(
        format!("{refused}").contains("descriptor_version"),
        "the refusal must name the stale axis: {refused}"
    );

    // No rule became active as a result of the refused activation.
    assert!(
        env.state
            .store
            .active_standing_rules_for_action(
                &ActionId::new("email.create_draft"),
                Timestamp::now()
            )
            .unwrap()
            .is_empty(),
        "a refused activation must leave no active rule"
    );
}

//! Activation-time verdict currency tests (#133). Split from
//! `eval_gate_tests.rs` to keep both files under the 500-line gate.

use jiff::Timestamp;

use super::scoped_admission::scoped_admission_support::{
    draft_env, mint_draft_grant, resolved_context, scoped_manifest,
};
use crate::pipeline::AppState;
use openspine_schemas::standing_rule::StandingRuleManifest;

async fn proposed_manifest(state: &AppState) -> StandingRuleManifest {
    let grant = mint_draft_grant(state, "thread-1");
    let context = resolved_context(state, &grant).await;
    let mut manifest = scoped_manifest("rule-eval", &context);
    manifest.lifecycle_state = openspine_schemas::artifact::Lifecycle::Proposed;
    manifest
}

// ------------------------------------------------- activation-time currency

#[tokio::test]
async fn stale_verdict_cannot_support_activation() {
    use crate::store::eval_verdict_store::{EvalVerdict, VerdictEpochs};

    let env = draft_env(&["thread-1"]).await;
    let manifest = proposed_manifest(&env.state).await;
    let binding = manifest.reviewed_scope.clone().unwrap();

    // A verdict recorded under the epochs the proposal was evaluated with.
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

    // Unchanged world: the verdict still binds, so activation is not refused
    // on currency grounds.
    let live = env
        .state
        .store
        .live_epochs_for_standing_rule(&manifest, Some(1), None, None);
    env.state
        .store
        .reject_stale_eval_verdicts(
            "standing_rule",
            manifest.id.as_str(),
            manifest.version,
            &live,
        )
        .expect("a current verdict must not block activation");

    // The descriptor is revised after evaluation: the verdict is stale and
    // activation must refuse rather than ride the old evidence through.
    let moved = env
        .state
        .store
        .live_epochs_for_standing_rule(&manifest, Some(2), None, None);
    let refused = env
        .state
        .store
        .reject_stale_eval_verdicts(
            "standing_rule",
            manifest.id.as_str(),
            manifest.version,
            &moved,
        )
        .expect_err("a stale verdict must refuse activation");
    assert!(
        format!("{refused}").contains("descriptor_version"),
        "the refusal must name the stale axis: {refused}"
    );
}

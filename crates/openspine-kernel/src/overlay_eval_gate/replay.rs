//! Replay evaluator (#133).
//!
//! For a reusable-authority proposal this executes concrete cases against the
//! exact proposed binding and records their outcomes; the ledger shape is
//! what makes the word *replay* defensible. A case-free or one-sided ledger
//! is a denial, not a lower score — there is deliberately no fitness number
//! for this evaluator, because `fitness: Some(1.0)` for counting conversation
//! turns is precisely the failure this change removes.
//!
//! For the other authority-bearing kinds there is no reviewed scope to vary.
//! The owner-control history check remains, but it is reported as what it
//! measures — `owner-control-history-availability` — and is never described
//! as a replay.

use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::digest::Digest;
use serde_json::json;

use crate::artifact_loader::ParsedProposal;
use crate::store::{Store, StoreError};

use super::eval_input::CanonicalEvaluationInput;
use super::replay_cases::{self, CaseBuildError};
use super::ReplayPassed;

#[derive(Debug, thiserror::Error)]
pub enum ReplayDenial {
    #[error("no captured owner-control history is available")]
    NoOwnerHistory,
    #[error("owner-control history query failed: {0}")]
    Store(#[from] StoreError),
    #[error("proposal is not in proposed lifecycle state")]
    InvalidLifecycle,
    #[error("model swap is missing kernel-verified golden-set results")]
    MissingGoldenSetResults,
    #[error("model swap has fewer than three passing standard golden-set cases")]
    StandardCoverageFailed,
    #[error("replay could not build its case set: {0}")]
    CaseSetUnbuildable(#[from] CaseBuildError),
    #[error("replay executed no cases, so nothing was replayed")]
    NoCasesExecuted,
    #[error("replay executed no case that matched the reviewed scope")]
    NoMatchingCase,
    #[error("replay executed no changed-context case that was refused")]
    NoRefusedChangedContextCase,
    #[error("replay case failed: {0}")]
    CaseFailed(String),
}

pub(super) fn evaluate(
    store: &Store,
    catalog: &openspine_schemas::action::ActionCatalog,
    input: &CanonicalEvaluationInput,
    proposal: &ParsedProposal,
    digest: &Digest,
) -> Result<ReplayPassed, ReplayDenial> {
    if proposal.lifecycle_state() != Lifecycle::Proposed {
        return Err(ReplayDenial::InvalidLifecycle);
    }
    if let ParsedProposal::ModelSwap(swap) = proposal {
        return model_swap_arm(swap, digest);
    }
    if let Some(reusable) = input.reusable() {
        // Executed-case replay needs a reviewed scope to vary. A standing
        // rule for an action with no reusable-delegation descriptor has
        // none, so it falls to the availability check — which is reported as
        // what it measures, never as a replay (D-163).
        if reusable.scoped().is_some() {
            return executed_case_arm(catalog, reusable, digest);
        }
    }
    availability_arm(store, digest)
}

/// The reusable-authority arm: build the case set, run it, and require a
/// two-sided ledger in which every case did what it was constructed to do.
fn executed_case_arm(
    catalog: &openspine_schemas::action::ActionCatalog,
    reusable: &super::eval_input::ReusableAuthorityInput,
    digest: &Digest,
) -> Result<ReplayPassed, ReplayDenial> {
    let scoped = reusable
        .scoped()
        .expect("executed-case replay runs only for a scope-bound rule");
    let ledger = replay_cases::execute(
        catalog,
        &reusable.manifest().action_id,
        &scoped.implementation().implementation_id,
        scoped.binding(),
    )?;

    if ledger.len() == 0 {
        return Err(ReplayDenial::NoCasesExecuted);
    }
    let failures = ledger.failures();
    if let Some(first) = failures.first() {
        return Err(ReplayDenial::CaseFailed(format!(
            "{:?}{} expected {:?} but observed {:?}",
            first.kind,
            first
                .dimension
                .as_ref()
                .map(|d| format!(" on {d}"))
                .unwrap_or_default(),
            first.expected,
            first.observed,
        )));
    }
    if ledger.matched_count() == 0 {
        return Err(ReplayDenial::NoMatchingCase);
    }
    if ledger.refused_changed_context_count() == 0 {
        return Err(ReplayDenial::NoRefusedChangedContextCase);
    }

    let evidence = json!({
        "evaluation": "proposal-bound-executed-cases",
        "cases_executed": ledger.len(),
        "cases_matched": ledger.matched_count(),
        "changed_context_cases_refused": ledger.refused_changed_context_count(),
        "mutated_dimensions": ledger.mutated_dimensions(),
        "ledger": ledger.cases,
        "reviewed_scope_digest": scoped.binding().reviewed_scope_digest.as_str(),
        "artifact_digest": digest.as_str(),
    });
    Ok(ReplayPassed {
        verdict: "pass",
        // No fitness: required case classes are pass/fail. A score is what
        // let "the owner has spoken once" record 1.0.
        fitness: None,
        evidence_json: evidence.to_string(),
        artifact_digest: digest.as_str().to_string(),
    })
}

/// Owner-control history availability, named for what it measures.
fn availability_arm(store: &Store, digest: &Digest) -> Result<ReplayPassed, ReplayDenial> {
    let owner_turns = store.count_owner_control_conversation_turns()?;
    if owner_turns == 0 {
        return Err(ReplayDenial::NoOwnerHistory);
    }
    let evidence = json!({
        "evaluation": "owner-control-history-availability",
        "cases_executed": 0,
        "captured_turns": owner_turns,
        "artifact_digest": digest.as_str(),
    });
    Ok(ReplayPassed {
        verdict: "pass",
        fitness: None,
        evidence_json: evidence.to_string(),
        artifact_digest: digest.as_str().to_string(),
    })
}

fn model_swap_arm(
    swap: &openspine_schemas::model_swap::ModelSwapManifest,
    digest: &Digest,
) -> Result<ReplayPassed, ReplayDenial> {
    let result = swap
        .golden_set_result
        .as_ref()
        .ok_or(ReplayDenial::MissingGoldenSetResults)?;
    let standard = result
        .cases
        .iter()
        .filter(|case| {
            matches!(
                case.kind,
                openspine_schemas::model_swap::GoldenSetCaseKind::Standard
            )
        })
        .collect::<Vec<_>>();
    let passed = standard.iter().filter(|case| case.passed).count();
    if passed < 3 {
        return Err(ReplayDenial::StandardCoverageFailed);
    }
    let evidence = json!({
        "evaluation": "golden-set-executed-cases",
        "golden_set_id": result.golden_set_id,
        "golden_set_digest": result.golden_set_digest,
        "cases_executed": standard.len(),
        "cases_matched": passed,
        "artifact_digest": digest.as_str(),
    });
    Ok(ReplayPassed {
        verdict: "pass",
        fitness: None,
        evidence_json: evidence.to_string(),
        artifact_digest: digest.as_str().to_string(),
    })
}

//! AD-142 overlay eval gate: every authority-bearing proposal (route /
//! agent / workflow / pack / policy — all five currently-proposable kinds
//! are authority-bearing per D-048's uniform-approval requirement; no
//! quiet-activating kind per AD-001 exists in this codebase yet, so there
//! is deliberately no exemption branch here — see "Scope" below) passes
//! an offline replay pass ([`replay`]) plus an adversarial risk-judge pass
//! ([`judge`], AD-110/111) BEFORE `dispatch_artifact_propose` may move the
//! proposal `validated -> review_required` and show the owner an approval
//! button. Both verdicts land in the eval-verdict store (D-056's landing
//! surface from `define-lineage-and-eval-store`) as evidence attached to
//! the proposal, so the one-loop confirmation (D-011) is informed rather
//! than decorative (AD-142).
//!
//! # Structural enforcement (not a runtime flag)
//!
//! [`ReplayPassed`] and [`JudgePassed`] have no public constructor — every
//! field is private to this module, so the only way to obtain one is
//! [`run_gate`] genuinely running the corresponding evaluator to a pass.
//! [`crate::store::proposed_artifacts::Store::promote_authority_bearing_proposal`]
//! is the *only* store operation that can perform the
//! `validated -> review_required` transition (the generic
//! `set_proposed_artifact_state` explicitly refuses that specific edge —
//! see its doc comment) and it consumes a `ReplayPassed` and a
//! `JudgePassed` by value, then re-derives the proposal's kind/id/version/
//! digest from the stored row itself and requires both tokens' embedded
//! digest to match it before promoting — a token computed for one
//! proposal can never promote a different one. A caller outside this
//! module cannot fabricate either type or reuse one across proposals, so
//! reaching the approval surface without the gate having genuinely run
//! against *that exact proposal* is a compile error / transactional
//! denial, not a runtime check a future call site could skip.
//!
//! # Evaluator policy scope (D-056)
//!
//! D-056 settled only the eval-verdict *landing surface* — an open
//! `verdict` string plus optional metadata — and explicitly deferred
//! judge-independence, evaluator identity, attack-trace evidence
//! semantics, and verdict vocabulary to a later, owner-ratified evaluation
//! change. AD-142 (settled) nonetheless requires this change to run *some*
//! replay and judge pass now, so [`replay`] and [`judge`] each implement a
//! minimal, first-cut, fully-deterministic evaluator built only from data
//! this kernel genuinely captures today (owner-control conversation turns,
//! the live artifact registry, the canonical action catalog). Their exact
//! pass/fail criteria are this change's own evaluator-policy proposal —
//! see `IMPLEMENTATION-NOTES.md`'s proposed `D-0XX` entries — not a claim
//! that they satisfy OQ-17's full "replay of past owner conversations
//! against a holdout set" or AD-111's prover-verifier attack-trace
//! formalism, both of which remain open for owner ratification in a later
//! change (mirroring how AD-152's model-swap golden-set format is
//! deferred to `implement-model-swap-ceremony`).

pub(crate) mod eval_input;
mod judge;
pub(crate) mod personality_probes;
mod replay;
mod replay_cases;
mod summary;

#[cfg(test)]
mod tests;

use openspine_schemas::action::ActionCatalog;
use openspine_schemas::digest::Digest;

use crate::artifact_loader::ParsedProposal;
use crate::store::Store;

pub(crate) use eval_input::{AssemblySources, IncompleteInput};
pub(crate) use judge::JudgeDenial;
pub(crate) use replay::ReplayDenial;

/// Why an authority-bearing proposal was denied before reaching the
/// approval surface. D-004 deny-by-default: an evaluator failing to reach
/// a pass verdict is itself a denial of the whole `artifact.propose` call
/// — the proposal never leaves `validated`, and the owner never sees an
/// approval button for it.
#[derive(Debug, thiserror::Error)]
pub enum GateDenial {
    #[error("model swaps require the verified golden-set runner")]
    ModelSwapRequiresVerifiedRunner,
    #[error("offline replay failed: {0}")]
    Replay(#[from] ReplayDenial),
    #[error("risk-judge pass failed: {0}")]
    Judge(#[from] JudgeDenial),
    #[error("approval summary exceeds the bounded safety limit")]
    ApprovalSummaryTooLong,
    #[error("evaluation input incomplete: {0}")]
    IncompleteInput(#[from] IncompleteInput),
}

/// Unforgeable proof the offline replay evaluator ran to completion and
/// concluded the proposal may proceed, bound to the exact artifact digest
/// it was run against (D-011). See the module doc for why fields are
/// private and why that binding matters.
#[derive(Debug)]
pub struct ReplayPassed {
    verdict: &'static str,
    fitness: Option<f64>,
    evidence_json: String,
    artifact_digest: String,
}

impl ReplayPassed {
    pub(crate) fn verdict(&self) -> &'static str {
        self.verdict
    }
    pub(crate) fn fitness(&self) -> Option<f64> {
        self.fitness
    }
    pub(crate) fn evidence_json(&self) -> &str {
        &self.evidence_json
    }
    pub(crate) fn artifact_digest(&self) -> &str {
        &self.artifact_digest
    }
}

/// Unforgeable proof the adversarial risk-judge evaluator ran to
/// completion and concluded the proposal may proceed, bound to the exact
/// artifact digest it was run against (D-011).
#[derive(Debug)]
pub struct JudgePassed {
    verdict: &'static str,
    fitness: Option<f64>,
    evidence_json: String,
    artifact_digest: String,
}

impl JudgePassed {
    pub(crate) fn verdict(&self) -> &'static str {
        self.verdict
    }
    pub(crate) fn fitness(&self) -> Option<f64> {
        self.fitness
    }
    pub(crate) fn evidence_json(&self) -> &str {
        &self.evidence_json
    }
    pub(crate) fn artifact_digest(&self) -> &str {
        &self.artifact_digest
    }
}

/// Both passing verdicts, plus a short human-readable summary meant for
/// the owner's approval message (AD-142: "informed, not decorative").
#[derive(Debug)]
pub struct GateEvidence {
    pub replay: ReplayPassed,
    pub judge: JudgePassed,
    pub summary: String,
    /// The epochs both verdicts were computed under, stored with them so
    /// staleness is decidable at read time (#133).
    pub epochs: crate::store::VerdictEpochs,
}

/// Run the AD-142 gate for one proposal against the exact bytes
/// (`artifact_digest`) the owner will be asked to approve. Every one of
/// the five currently proposable kinds is authority-bearing (D-048), so
/// this always runs both evaluators — see the module doc for why there is
/// no "exempt" branch to bypass.
pub fn run_gate(
    store: &Store,
    catalog: &ActionCatalog,
    proposal: &ParsedProposal,
    artifact_digest: &Digest,
    sources: AssemblySources<'_>,
) -> Result<GateEvidence, GateDenial> {
    if matches!(proposal, ParsedProposal::ModelSwap(_)) {
        return Err(GateDenial::ModelSwapRequiresVerifiedRunner);
    }
    // Assemble first: an unresolvable dimension is a denial naming it, never
    // a fallback to evaluating the proposal as a generic artifact (#133).
    let input = eval_input::assemble(proposal, artifact_digest, sources)?;
    // Judge before replay: both must pass, but a structural denial names the
    // axis that is actually wrong ("no registered executor") where an
    // availability denial would mask it behind "no owner history".
    let judge = judge::evaluate(catalog, &input, proposal, artifact_digest)?;
    let replay = replay::evaluate(store, catalog, &input, proposal, artifact_digest)?;
    // Copy is rendered from the stored evidence, never authored: there is no
    // free-text field in which a replay claim could outrun the ledger.
    let summary = summary::render(&replay, &judge);
    Ok(GateEvidence {
        replay,
        judge,
        summary,
        epochs: input.epochs().clone(),
    })
}
/// Run the model-swap branch after `model_swap::enrich` has executed the
/// trusted golden set. Keeping this separate from [`run_gate`] prevents a
/// caller from treating deserialized `passed` booleans as generic replay
/// proof; the dispatcher reaches this function only after enrichment.
pub(crate) fn run_model_swap_gate(
    store: &Store,
    catalog: &ActionCatalog,
    proposal: &ParsedProposal,
    artifact_digest: &Digest,
    sources: AssemblySources<'_>,
) -> Result<GateEvidence, GateDenial> {
    if !matches!(proposal, ParsedProposal::ModelSwap(_)) {
        return Err(GateDenial::ModelSwapRequiresVerifiedRunner);
    }
    let input = eval_input::assemble(proposal, artifact_digest, sources)?;
    let judge = judge::evaluate(catalog, &input, proposal, artifact_digest)?;
    let replay = replay::evaluate(store, catalog, &input, proposal, artifact_digest)?;
    let summary = if let ParsedProposal::ModelSwap(manifest) = proposal {
        let observed_cases: Vec<_> = manifest
            .golden_set_result
            .as_ref()
            .map(|result| {
                result
                    .cases
                    .iter()
                    .map(|case| {
                        serde_json::json!({
                            "case_id": case.case_id,
                            "kind": case.kind,
                            "passed": case.passed,
                            "observed_excerpt": case.observed_excerpt.chars().take(120).collect::<String>(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        format!(
            "AD-152 model-swap golden-set gate — role: {} target_provider_id: {}; observed_cases: {}; replay: {} ({}); risk judge: {} ({})",
            manifest.role.as_str(),
            manifest.target_provider_id,
            serde_json::to_string(&observed_cases).unwrap_or_default(),
            replay.verdict(),
            replay.evidence_json(),
            judge.verdict(),
            judge.evidence_json(),
        )
    } else {
        unreachable!("model-swap gate checked the proposal kind above")
    };
    if summary.encode_utf16().count() > 3_500 {
        return Err(GateDenial::ApprovalSummaryTooLong);
    }
    Ok(GateEvidence {
        replay,
        judge,
        summary,
        epochs: input.epochs().clone(),
    })
}

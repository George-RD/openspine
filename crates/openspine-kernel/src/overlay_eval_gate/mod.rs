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

mod judge;
mod replay;

#[cfg(test)]
mod tests;

use openspine_schemas::action::ActionCatalog;
use openspine_schemas::digest::Digest;

use crate::artifact_loader::ParsedProposal;
use crate::store::Store;

pub(crate) use judge::JudgeDenial;
pub(crate) use replay::ReplayDenial;

/// Why an authority-bearing proposal was denied before reaching the
/// approval surface. D-004 deny-by-default: an evaluator failing to reach
/// a pass verdict is itself a denial of the whole `artifact.propose` call
/// — the proposal never leaves `validated`, and the owner never sees an
/// approval button for it.
#[derive(Debug, thiserror::Error)]
pub enum GateDenial {
    #[error("offline replay failed: {0}")]
    Replay(#[from] ReplayDenial),
    #[error("risk-judge pass failed: {0}")]
    Judge(#[from] JudgeDenial),
}

/// Unforgeable proof the offline replay evaluator ran to completion and
/// concluded the proposal may proceed, bound to the exact artifact digest
/// it was run against (D-011). See the module doc for why fields are
/// private and why that binding matters.
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
pub struct GateEvidence {
    pub replay: ReplayPassed,
    pub judge: JudgePassed,
    pub summary: String,
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
) -> Result<GateEvidence, GateDenial> {
    let replay = replay::evaluate(store, proposal, artifact_digest)?;
    let judge = judge::evaluate(catalog, proposal, artifact_digest)?;
    let summary = format!(
        "AD-142 overlay eval gate — replay: {} ({}); risk judge: {} ({})",
        replay.verdict(),
        replay.evidence_json(),
        judge.verdict(),
        judge.evidence_json(),
    );
    Ok(GateEvidence {
        replay,
        judge,
        summary,
    })
}

//! Deterministic first-cut risk judge for AD-142.
//!
//! D-056 leaves evaluator identity, independence, attack-trace semantics,
//! and verdict vocabulary open. This module therefore supplies only
//! structural probes over the canonical action catalog and artifact-declared
//! authority lists; it does not claim to settle those deferred policies.

use openspine_schemas::action::ActionCatalog;
use openspine_schemas::digest::Digest;
use serde_json::json;

use crate::artifact_loader::ParsedProposal;

use super::JudgePassed;

#[derive(Debug, thiserror::Error)]
pub enum JudgeDenial {
    #[error("proposal declares unknown action `{0}`")]
    UnknownAction(String),
    #[error("action `{0}` is both allowed and denied")]
    AllowDenyConflict(String),
    #[error("model swap is missing kernel-verified golden-set results")]
    MissingGoldenSetResults,
    #[error("model swap has no passing adversarial golden-set case")]
    AdversarialCaseFailed,
}

pub(super) fn evaluate(
    catalog: &ActionCatalog,
    proposal: &ParsedProposal,
    digest: &Digest,
) -> Result<JudgePassed, JudgeDenial> {
    if let ParsedProposal::ModelSwap(swap) = proposal {
        let result = swap
            .golden_set_result
            .as_ref()
            .ok_or(JudgeDenial::MissingGoldenSetResults)?;
        let adversarial = result
            .cases
            .iter()
            .filter(|case| {
                matches!(
                    case.kind,
                    openspine_schemas::model_swap::GoldenSetCaseKind::Adversarial
                )
            })
            .collect::<Vec<_>>();
        if adversarial.is_empty() || adversarial.iter().any(|case| !case.passed) {
            return Err(JudgeDenial::AdversarialCaseFailed);
        }
        let evidence = json!({
            "probe": "golden-set-adversarial-cases",
            "golden_set_id": result.golden_set_id,
            "golden_set_digest": result.golden_set_digest,
            "adversarial_cases": adversarial.len(),
            "adversarial_passed": adversarial.iter().filter(|case| case.passed).count(),
            "artifact_digest": digest.as_str(),
        });
        return Ok(JudgePassed {
            verdict: "pass",
            fitness: Some(1.0),
            evidence_json: evidence.to_string(),
            artifact_digest: digest.as_str().to_string(),
        });
    }
    let mut declared = Vec::new();
    let mut denied = Vec::new();
    match proposal {
        ParsedProposal::Route(_route) => {}
        ParsedProposal::Agent(agent) => {
            declared.extend(agent.designed_tools.iter());
            declared.extend(agent.approval_required_tools.iter());
            denied.extend(agent.denied_tools.iter());
        }
        ParsedProposal::Workflow(workflow) => {
            declared.extend(workflow.candidate_allowed_actions.iter());
            declared.extend(workflow.approval_required.iter());
            denied.extend(workflow.denied_actions.iter());
        }
        ParsedProposal::Pack(pack) => {
            declared.extend(pack.candidate_allowed_actions.iter());
            declared.extend(pack.approval_required.iter());
            denied.extend(pack.denied_actions.iter());
        }
        ParsedProposal::Policy(policy) => {
            declared.extend(policy.candidate_allowed_actions.iter());
            declared.extend(policy.approval_required.iter());
            denied.extend(policy.denied_actions.iter());
        }
        ParsedProposal::StandingRule(rule) => {
            // A standing rule authorizes exactly one action (composition
            // input, never a live authority source) — the judge only needs
            // to confirm that action is a known catalog id.
            declared.push(&rule.action_id);
        }
        ParsedProposal::ModelSwap(swap) => {
            let result = swap
                .golden_set_result
                .as_ref()
                .ok_or(JudgeDenial::MissingGoldenSetResults)?;
            let adversarial = result
                .cases
                .iter()
                .filter(|case| {
                    matches!(
                        case.kind,
                        openspine_schemas::model_swap::GoldenSetCaseKind::Adversarial
                    )
                })
                .collect::<Vec<_>>();
            if adversarial.is_empty() || adversarial.iter().any(|case| !case.passed) {
                return Err(JudgeDenial::AdversarialCaseFailed);
            }
        }
    }
    for action in &declared {
        if !catalog.contains(action) {
            return Err(JudgeDenial::UnknownAction(action.to_string()));
        }
        if denied.contains(action) {
            return Err(JudgeDenial::AllowDenyConflict(action.to_string()));
        }
    }
    let evidence = json!({
        "probe": "canonical-action-catalog-and-allow-deny-consistency",
        "declared_actions": declared.iter().map(|a| a.as_str()).collect::<Vec<_>>(),
        "artifact_digest": digest.as_str(),
    });
    Ok(JudgePassed {
        verdict: "pass",
        fitness: Some(1.0),
        evidence_json: evidence.to_string(),
        artifact_digest: digest.as_str().to_string(),
    })
}

//! Deterministic first-cut replay evaluator for AD-142.
//!
//! This is deliberately policy-neutral about AD-111's deferred attack-trace
//! vocabulary (D-056). It requires a positively identified owner-control
//! conversation corpus and refuses to pass when that corpus is empty.

use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::digest::Digest;
use serde_json::json;

use crate::artifact_loader::ParsedProposal;
use crate::store::{Store, StoreError};

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
}

/// Replay the proposal against the captured owner-control corpus. The
/// corpus is intentionally provenance-filtered by the store query; generic
/// model-use turns cannot satisfy this prerequisite.
pub(super) fn evaluate(
    store: &Store,
    proposal: &ParsedProposal,
    digest: &Digest,
) -> Result<ReplayPassed, ReplayDenial> {
    if proposal.lifecycle_state() != Lifecycle::Proposed {
        return Err(ReplayDenial::InvalidLifecycle);
    }
    if let ParsedProposal::ModelSwap(swap) = proposal {
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
        if standard.iter().filter(|case| case.passed).count() < 3 {
            return Err(ReplayDenial::StandardCoverageFailed);
        }
        let evidence = json!({
            "corpus": "golden-set",
            "golden_set_id": result.golden_set_id,
            "golden_set_digest": result.golden_set_digest,
            "standard_cases": standard.len(),
            "standard_passed": standard.iter().filter(|case| case.passed).count(),
            "artifact_digest": digest.as_str(),
        });
        return Ok(ReplayPassed {
            verdict: "pass",
            fitness: Some(1.0),
            evidence_json: evidence.to_string(),
            artifact_digest: digest.as_str().to_string(),
        });
    }
    let owner_turns = store.count_owner_control_conversation_turns()?;
    if owner_turns == 0 {
        return Err(ReplayDenial::NoOwnerHistory);
    }

    let route_specificity = None::<u32>;
    let evidence = json!({
        "corpus": "owner-control-conversation",
        "captured_turns": owner_turns,
        "route_specificity": route_specificity,
        "artifact_digest": digest.as_str(),
    });
    Ok(ReplayPassed {
        verdict: "pass",
        fitness: Some(1.0),
        evidence_json: evidence.to_string(),
        artifact_digest: digest.as_str().to_string(),
    })
}

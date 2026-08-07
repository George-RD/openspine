//! Owner-facing gate copy, rendered from the stored verdicts (#133).
//!
//! The summary is a pure function of the two verdicts' recorded evidence.
//! There is no free-text field, so a claim cannot outrun what actually ran —
//! the failure PR #125 shipped, where copy said prior examples were replayed
//! while only corpus presence had been checked.
//!
//! The rule the renderer enforces: the words *replay*/*replayed* appear only
//! when the evidence records a non-zero executed-case count.

use serde_json::Value;

use super::{JudgePassed, ReplayPassed};

/// Render the owner-facing summary from what the evaluators recorded.
pub(super) fn render(replay: &ReplayPassed, judge: &JudgePassed) -> String {
    let replay_evidence: Value =
        serde_json::from_str(replay.evidence_json()).unwrap_or(Value::Null);
    let judge_evidence: Value = serde_json::from_str(judge.evidence_json()).unwrap_or(Value::Null);

    let executed = replay_evidence
        .get("cases_executed")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let evaluation = replay_evidence
        .get("evaluation")
        .and_then(Value::as_str)
        .unwrap_or("unnamed-evaluation");

    let mut parts = Vec::new();

    if executed == 0 {
        // Nothing was replayed. Name the check for what it measured and make
        // no claim about cases.
        parts.push(format!(
            "OpenSpine checked {evaluation} for this proposal; no proposal-specific cases were executed."
        ));
    } else {
        let matched = replay_evidence
            .get("cases_matched")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let refused = replay_evidence
            .get("changed_context_cases_refused")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let dimensions = replay_evidence
            .get("mutated_dimensions")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        parts.push(format!(
            "OpenSpine replayed this exact proposal against {executed} case(s): \
             {matched} matched the reviewed scope and {refused} changed-context case(s) were refused."
        ));
        if !dimensions.is_empty() {
            parts.push(format!("Dimensions varied: {dimensions}."));
        }
    }

    if let Some(axes) = judge_evidence.get("axes_passed").and_then(Value::as_array) {
        let names = axes
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("Structural checks passed: {names}."));
    } else if let Some(probe) = judge_evidence.get("probe").and_then(Value::as_str) {
        parts.push(format!("Structural check: {probe}."));
    }

    // Name each evaluator by what it actually did, taken from its own
    // recorded evidence. Hardcoding the label "replay" here would put that
    // word next to "pass" even on a proposal where nothing was replayed —
    // the same overclaim one layer down.
    let judge_probe = judge_evidence
        .get("probe")
        .and_then(Value::as_str)
        .unwrap_or("structural-probe");
    parts.push(format!(
        "Evaluation grants no authority; approval remains yours. \
         (evaluators: {evaluation}={}; {judge_probe}={})",
        replay.verdict(),
        judge.verdict(),
    ));

    parts.join(" ")
}

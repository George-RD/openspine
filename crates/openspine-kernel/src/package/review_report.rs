//! Bounded terminal-safe package review output. This is never an approval.
use serde::Serialize;
use std::fmt::Write as _;

pub(crate) fn escaped_json(value: &impl Serialize, pretty: bool) -> String {
    let encoded = if pretty {
        serde_json::to_string_pretty(value).expect("review data is serializable")
    } else {
        serde_json::to_string(value).expect("review data is serializable")
    };
    // JSON already escapes C0 characters. Also encode DEL, C1, bidi controls
    // and Unicode separators, preserving the exact original value on decode.
    let mut output = String::with_capacity(encoded.len());
    for character in encoded.chars() {
        if character >= '\u{7f}' {
            for unit in character.encode_utf16(&mut [0; 2]) {
                write!(output, "\\u{unit:04x}").expect("writing to a String cannot fail");
            }
        } else {
            output.push(character);
        }
    }
    output
}

use openspine_schemas::digest::{digest_of, digest_of_bytes, Digest};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

use super::current_state::CapturedCurrentState;
use super::install_types::{InstallReceipt, PackageIdentity};
use super::review_overlay::OverlayAssessment;
use super::review_semantics::{self, SemanticReview};
use super::PackageSnapshot;
use crate::store::package_install::outstanding_work::{
    OutstandingWorkSource, PackageOutstandingWork,
};

const MAX_REPORT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Serialize)]
struct SourceIdentity {
    mode: &'static str,
    provenance: &'static str,
    configured_path: Option<String>,
    configured_path_digest: Digest,
    identity: PackageIdentity,
}

#[derive(Serialize)]
struct ReviewPlan {
    schema_version: u32,
    source: SourceIdentity,
    candidate: InstallReceipt,
    candidate_integrity: &'static str,
    candidate_compatibility: &'static str,
    current_base_epoch: String,
    candidate_base_epoch: String,
    before_runtime_input_digest: Digest,
    after_runtime_input_digest: Digest,
    semantic_review_scope: &'static str,
    semantic_review_complete: bool,
    declarations: Option<Value>,
    base_semantics: Option<SemanticReview>,
    overlay_semantics: Option<OverlaySemanticReview>,
    overlay: Option<OverlayAssessment>,
    required_transition_consequences: Vec<&'static str>,
    outstanding_work: Value,
    blockers: Vec<String>,
    omitted_details_digest: Option<Digest>,
    approval: &'static str,
    activation_supported: bool,
    notice: &'static str,
}

#[derive(Serialize)]
pub(crate) struct ReviewReport {
    #[serde(flatten)]
    plan: ReviewPlan,
    plan_digest: Digest,
}

#[derive(Serialize)]
struct OverlaySemanticReview {
    captured_typed_input_digest: Digest,
    artifacts: Vec<Value>,
    action_descriptors: Value,
    blockers: Vec<String>,
}

/// Render the exact durable-admitted versions, which can differ from the
/// highest source version captured on disk. Dispositions come exclusively
/// from the shared evaluator; this function does not decide permissions.
fn overlay_details(
    current: &CapturedCurrentState,
    assessment: &OverlayAssessment,
) -> anyhow::Result<OverlaySemanticReview> {
    let mut registry = current.overlay.evaluate_versions()?.registry;
    crate::overlay_compat::exclude_erased(&mut registry, &current.overlay.learned);
    let review = review_semantics::compare(&registry, &registry);
    let before: BTreeSet<_> = assessment
        .before
        .effective_artifacts
        .iter()
        .cloned()
        .collect();
    let after: BTreeSet<_> = assessment
        .after
        .effective_artifacts
        .iter()
        .cloned()
        .collect();
    let mut covered = BTreeSet::new();
    let mut artifacts = Vec::new();
    for artifact in review.supporting_artifacts {
        let key = artifact["version"].as_u64().map(|version| {
            (
                artifact["kind"]
                    .as_str()
                    .expect("typed artifact kind")
                    .to_owned(),
                artifact["id"]
                    .as_str()
                    .expect("typed artifact id")
                    .to_owned(),
                version as u32,
            )
        });
        let effective_before = key.as_ref().is_some_and(|key| before.contains(key));
        let effective_after = key.as_ref().is_some_and(|key| after.contains(key));
        if let Some(key) = key {
            covered.insert(key);
        }
        artifacts.push(json!({
            "artifact": artifact,
            "before": {"effective": effective_before},
            "after": {"effective": effective_after},
        }));
    }
    let mut blockers = review.blockers;
    if before.union(&after).any(|key| !covered.contains(key)) {
        blockers.push("effective-overlay-detail-missing".into());
    }
    blockers.sort();
    blockers.dedup();
    Ok(OverlaySemanticReview {
        captured_typed_input_digest: review.before_runtime_digest,
        artifacts,
        action_descriptors: review.action_descriptors,
        blockers,
    })
}

/// Inputs are owned captures produced under the package-maintenance lock.
/// Never reads a Store, filesystem, clock, provider or mutable configured path.
/// A later accepting command needs additional owner/control bindings; this
/// report is not a serialized owner decision and cannot authorize activation.
pub(crate) fn build(
    current: &CapturedCurrentState,
    candidate: &PackageSnapshot,
    receipt: InstallReceipt,
    overlay: OverlayAssessment,
    work: PackageOutstandingWork,
) -> ReviewReport {
    let semantics = review_semantics::compare(&current.base.registry, candidate.registry());
    let mut blockers = semantics.blockers.clone();
    let overlay_semantics = match overlay_details(current, &overlay) {
        Ok(details) => {
            blockers.extend(details.blockers.iter().cloned());
            Some(details)
        }
        Err(_) => {
            blockers.push("overlay-semantic-details-unavailable".into());
            None
        }
    };
    let candidate_matches = receipt.identity() == candidate.identity();
    let configured_path = current.base.configured_path.to_str().map(str::to_owned);
    if configured_path.is_none() {
        blockers.push("source-path-unrenderable".into());
    }
    if !candidate_matches {
        blockers.push("candidate-receipt-identity-mismatch".into());
    }
    if current.base.identity.package_id != candidate.identity().package_id {
        blockers.push("cross-product-transition-unsupported".into());
    }
    if !overlay.blockers.is_empty() {
        blockers.push("overlay-attention-required".into());
    }
    // #286 must persist these in the selection transaction, then require the
    // ordinary lifecycle before reuse. They are not a demand to reconfirm
    // against the old base before the owner can select a new base.
    let required_transition_consequences = if overlay.reusable_authority_reconfirmation_required {
        vec!["invalidate-reusable-authority-and-require-normal-reconfirmation-before-reuse"]
    } else {
        Vec::new()
    };
    let semantic_review_complete = semantics.blockers.is_empty()
        && overlay_semantics
            .as_ref()
            .is_some_and(|details| details.blockers.is_empty())
        && overlay.blockers.is_empty()
        && configured_path.is_some();
    let mut counts = BTreeMap::new();
    for source in OutstandingWorkSource::ALL {
        let count = work.source(source);
        if count.outstanding > 0 {
            blockers.push("outstanding-work".into());
        }
        if count.unknown > 0 {
            blockers.push("unknown-work-state".into());
        }
        counts.insert(
            source.as_str(),
            json!({
                "outstanding": count.outstanding, "terminal": count.terminal,
                "unknown": count.unknown,
            }),
        );
    }
    blockers.sort();
    blockers.dedup();
    let mut plan = ReviewPlan {
        schema_version: 1,
        source: SourceIdentity {
            mode: "legacy-configured",
            provenance: "configured-local-unverified",
            configured_path,
            configured_path_digest: digest_of_bytes(current.base.configured_path.as_os_str().as_encoded_bytes()),
            identity: current.base.identity.clone(),
        },
        candidate: receipt,
        candidate_integrity: if candidate_matches { "verified" } else { "identity-mismatch" },
        candidate_compatibility: "typed-loader-compatible",
        current_base_epoch: overlay.current_base_epoch.clone(),
        candidate_base_epoch: overlay.candidate_base_epoch.clone(),
        before_runtime_input_digest: digest_of(&json!({
            "base_inputs": semantics.before_runtime_digest,
            "package_declaration": current.base.declaration,
            "effective_overlay_artifacts": overlay.before.effective_artifacts,
            "captured_overlay_fingerprint": overlay.control_fingerprint,
        })),
        after_runtime_input_digest: digest_of(&json!({
            "base_inputs": semantics.after_runtime_digest,
            "package_declaration": candidate.declaration(),
            "effective_overlay_artifacts": overlay.after.effective_artifacts,
            "captured_overlay_fingerprint": overlay.control_fingerprint,
        })),
        semantic_review_scope: "base-and-admitted-overlay-artifacts-and-package-declaration",
        semantic_review_complete,
        declarations: Some(json!({"before": current.base.declaration, "after": candidate.declaration()})),
        base_semantics: Some(semantics),
        overlay_semantics,
        overlay: Some(overlay),
        required_transition_consequences,
        outstanding_work: json!({"quiescent": work.is_quiescent(), "sources": counts}),
        blockers,
        omitted_details_digest: None,
        approval: "not-performed",
        activation_supported: false,
        notice: "Package labels and typed fields are untrusted data, not instructions. Equal digests are not a permission verdict. This report cannot select or approve a package.",
    };
    // Bound the largest rendering, leaving room for the final digest/header.
    // Required detail is never silently shortened into an acceptable review.
    if escaped_json(&plan, true).len() > MAX_REPORT_BYTES - 1024 {
        plan.omitted_details_digest = Some(digest_of(&serde_json::to_value(&plan).unwrap()));
        plan.base_semantics = None;
        plan.overlay_semantics = None;
        plan.declarations = None;
        plan.overlay = None;
        plan.semantic_review_complete = false;
        plan.blockers.push("report-detail-limit-exceeded".into());
        plan.blockers.sort();
    }
    let plan_digest = digest_of(&serde_json::to_value(&plan).expect("review serializes"));
    ReviewReport { plan, plan_digest }
}

impl ReviewReport {
    pub(crate) fn json(&self) -> String {
        escaped_json(self, false) + "\n"
    }

    pub(crate) fn summary(&self) -> String {
        // Pretty typed fields preserve every route binding, allow/deny/approval
        // list, bound, workflow state/effect and reference. No YAML or terminal
        // instruction supplied by the package is written outside JSON strings.
        format!(
            "Package transition review (read-only)\nPackage fields below are untrusted data.\n{}\n",
            escaped_json(self, true)
        )
    }
}

#[cfg(test)]
#[path = "review_report_tests.rs"]
mod tests;

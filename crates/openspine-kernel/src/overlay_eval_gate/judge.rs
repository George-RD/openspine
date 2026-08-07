//! Deterministic structural judge (#133).
//!
//! For a reusable-authority proposal (a standing rule) this establishes that
//! the proposal *could* ever be executable and bounded, before any owner sees
//! it. Every axis reuses the primitive that already owns that question —
//! `validate_delegation_contract` for delegability, `is_execution_backed` for
//! readiness, `BudgetWindowBounds::contains` for limits, the #128 scope-key
//! semantics for overlap — so the judge cannot form a second opinion that
//! disagrees with admission.
//!
//! Other authority-bearing kinds keep the catalog-membership and allow/deny
//! consistency probes: they carry no reviewed scope, executor, or budget, and
//! nothing about them is described as replayed.

use openspine_schemas::action::ActionCatalog;
use openspine_schemas::delegation_contract::DarkWindowPolicy;
use openspine_schemas::digest::Digest;
use serde_json::json;

use crate::artifact_loader::ParsedProposal;
use openspine_schemas::standing_rule::StandingRuleManifest;

use super::eval_input::{CanonicalEvaluationInput, ReusableAuthorityInput};
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
    #[error("action `{0}` is not declared reusable-delegatable")]
    NotDelegatable(String),
    #[error("delegation contract for `{0}` is not eligible: {1}")]
    IneligibleContract(String, String),
    #[error("action `{0}` has no registered executor or handler, so a reusable rule could never execute")]
    ExecutorNotReady(String),
    #[error("an active policy denies action `{0}`")]
    PolicyDenied(String),
    #[error("dark-window configuration is not admissible for `{0}`: {1}")]
    DarkWindowNotAdmissible(String, String),
    #[error("{0} is outside the bounds declared for action `{1}`")]
    LimitOutOfBounds(String, String),
    #[error("standing rule manifest is invalid: {0}")]
    InvalidManifest(String),
    #[error(
        "reviewed scope collides with active rule `{0}`, which a different artifact already holds"
    )]
    ScopeCollidesWithActiveRule(String),
    #[error("reviewed scope widens active rule `{0}` without a new owner review")]
    ScopeWidensActiveRule(String),
    #[error(
        "action `{0}` is not an approval-narrowing action, so a standing rule for it MUST bind a reviewed scope"
    )]
    UnscopedRuleForEffectfulAction(String),
}

pub(super) fn evaluate(
    catalog: &ActionCatalog,
    input: &CanonicalEvaluationInput,
    proposal: &ParsedProposal,
    digest: &Digest,
) -> Result<JudgePassed, JudgeDenial> {
    if let ParsedProposal::ModelSwap(swap) = proposal {
        return model_swap_arm(swap, digest);
    }
    if let Some(reusable) = input.reusable() {
        return reusable_authority_arm(reusable, digest);
    }
    catalog_structural_arm(catalog, proposal, digest)
}

/// The reusable-authority axes. Each failure names the axis so a denial is
/// actionable; none of them is a score. The authority axes run for EVERY
/// standing rule — a rule admits its action without per-instance approval
/// whatever its action's catalog shape. The scope axes run only when the
/// action carries a reusable-delegation descriptor.
fn reusable_authority_arm(
    input: &ReusableAuthorityInput,
    digest: &Digest,
) -> Result<JudgePassed, JudgeDenial> {
    let manifest = input.manifest();
    let action = manifest.action_id.to_string();

    // 1. Positive-value manifest invariants (budgets, expiry, binding
    //    integrity) — the schema's own check, reused rather than restated.
    manifest.validate().map_err(JudgeDenial::InvalidManifest)?;

    // 2. The action must exist in the canonical catalog. Every other axis
    //    reads catalog metadata, and an unknown id is absent from all of it —
    //    `is_non_delegable`, the deny set and the descriptor map all answer
    //    "no" for an id nobody declared — so without this the rule would pass
    //    by simply naming something that does not exist.
    if !input.catalogued() {
        return Err(JudgeDenial::UnknownAction(action));
    }

    // 3. An action the catalog marks non-delegable may never be handed to a
    //    standing rule: a standing rule IS delegation.
    if input.non_delegable() {
        return Err(JudgeDenial::NotDelegatable(action));
    }

    // 4. Deny beats everything: a denied action can never become reusable
    //    authority, however well-formed the rest of the proposal is.
    if input.policy_denies_action() {
        return Err(JudgeDenial::PolicyDenied(action));
    }

    let mut axes = vec![
        "manifest_invariants",
        "catalogued",
        "delegability",
        "policy_deny",
    ];

    let Some(scoped) = input.scoped() else {
        // 5. No reusable-delegation descriptor, so there is no reviewed scope
        //    to bind and no executed-case replay to run. Such a rule grants
        //    BLANKET authority for its action, so the action must be one the
        //    catalog declares safe to admit that way: an approval-narrowing
        //    action that cannot itself reach a connector, write, or
        //    communicate.
        //
        //    Using "has no reviewed scope" as the licence instead would be the
        //    same hole one layer down, and it lands on exactly the actions
        //    where it matters most — `email.send`, `network.raw_egress`,
        //    `filesystem.host_write`, `secret.rotate`, `coolify.deploy` and
        //    `policy.modify_direct` all carry no delegation descriptor.
        if !input.approval_narrowing() {
            return Err(JudgeDenial::UnscopedRuleForEffectfulAction(action));
        }
        axes.push("approval_narrowing_action");
        let evidence = json!({
            "probe": "reusable-authority-structural-axes",
            "action_id": manifest.action_id.as_str(),
            "axes_passed": axes,
            "scope_bound": false,
            "artifact_digest": digest.as_str(),
        });
        return Ok(JudgePassed {
            verdict: "pass",
            fitness: None,
            evidence_json: evidence.to_string(),
            artifact_digest: digest.as_str().to_string(),
        });
    };

    let descriptor = scoped.descriptor();

    // 5. Executor readiness. This is a scope-bound axis: it asks whether the
    //    *named* implementation has a registered executor, so it applies only
    //    where a reusable-delegation descriptor names one. An action with no
    //    such descriptor has no implementation to check readiness of, and
    //    asserting otherwise would refuse rules the runtime handles fine.
    if !input.executor_ready() {
        return Err(JudgeDenial::ExecutorNotReady(action));
    }
    axes.push("executor_readiness");

    // 6. Contract eligibility across both catalog axes.
    if !descriptor.reusable_delegation {
        return Err(JudgeDenial::NotDelegatable(action));
    }
    openspine_schemas::delegation_contract::validate_delegation_contract(
        descriptor,
        scoped.implementation(),
    )
    .map_err(|err| JudgeDenial::IneligibleContract(action.clone(), err.to_string()))?;
    axes.push("contract_eligibility");

    // 6. Dark-window admissibility for the action's effect class. D-146
    //    forbids a communication/connector-write Allow default outright;
    //    `Prohibited` forbids any configuration at all.
    if let Some(dark_window) = manifest.dark_window {
        let policy_dark_window = descriptor
            .delegation_policy
            .as_ref()
            .map(|policy| policy.dark_window_policy)
            .unwrap_or(DarkWindowPolicy::Prohibited);
        match policy_dark_window {
            DarkWindowPolicy::Prohibited => {
                return Err(JudgeDenial::DarkWindowNotAdmissible(
                    action,
                    "the action prohibits dark windows".to_string(),
                ));
            }
            DarkWindowPolicy::DenyOnly {
                maximum_timeout_secs,
                maximum_outstanding: _,
            } => {
                if dark_window.timeout_secs > maximum_timeout_secs {
                    return Err(JudgeDenial::DarkWindowNotAdmissible(
                        action,
                        format!(
                            "timeout {} exceeds the declared maximum {}",
                            dark_window.timeout_secs, maximum_timeout_secs
                        ),
                    ));
                }
                if matches!(
                    dark_window.default,
                    openspine_schemas::standing_rule::DarkWindowDefault::Allow
                ) {
                    return Err(JudgeDenial::DarkWindowNotAdmissible(
                        action,
                        "the action permits deny-only dark windows".to_string(),
                    ));
                }
            }
            DarkWindowPolicy::BoundedAllow { .. } => {}
        }
    }
    axes.push("dark_window_admissibility");

    // 7. Budgets and expiry within the action's own declared bounds. There
    //    is no product-wide maxima table; the descriptor owns its envelope.
    if let Some(policy) = descriptor.delegation_policy.as_ref() {
        if !policy.quota.contains(manifest.quota) {
            return Err(JudgeDenial::LimitOutOfBounds("quota".to_string(), action));
        }
        if !policy.rate.contains(manifest.rate) {
            return Err(JudgeDenial::LimitOutOfBounds("rate".to_string(), action));
        }
        if manifest.expires_after_secs > policy.maximum_lapse_secs {
            return Err(JudgeDenial::LimitOutOfBounds("expiry".to_string(), action));
        }
    }
    axes.push("budget_and_expiry_bounds");

    // 8. Overlap and widening. `ReviewedActionScope::compare` is the single
    //    comparison implementation (#128); a proposed scope that an active
    //    rule's scope still matches is either the same scope under a
    //    different artifact — a silent supersession takeover — or a widening
    //    of it. Either needs an explicit new owner review.
    let binding = scoped.binding();
    for active in scoped.active_rules() {
        if active.artifact_id == manifest.id {
            continue;
        }
        let Some(active_key) = active.reviewed_scope_digest.as_ref() else {
            // An unbound active rule covers every scope for this action, so
            // any scoped proposal collides with it.
            return Err(JudgeDenial::ScopeCollidesWithActiveRule(
                active.rule_id.clone(),
            ));
        };
        if active_key.as_str() == binding.reviewed_scope_digest.as_str() {
            return Err(JudgeDenial::ScopeCollidesWithActiveRule(
                active.rule_id.clone(),
            ));
        }
        if let Some(active_scope) = active_reviewed_scope(active) {
            if widens(binding.scope.dimensions(), active_scope.dimensions()) {
                return Err(JudgeDenial::ScopeWidensActiveRule(active.rule_id.clone()));
            }
        }
    }
    axes.push("active_rule_overlap");

    let evidence = json!({
        "probe": "reusable-authority-structural-axes",
        "action_id": manifest.action_id.as_str(),
        "axes_passed": axes,
        "scope_bound": true,
        "reviewed_scope_digest": binding.reviewed_scope_digest.as_str(),
        "compatibility_digest": binding.compatibility_digest.as_str(),
        "active_rules_considered": scoped.active_rules().len(),
        "artifact_digest": digest.as_str(),
    });
    Ok(JudgePassed {
        verdict: "pass",
        fitness: None,
        evidence_json: evidence.to_string(),
        artifact_digest: digest.as_str().to_string(),
    })
}

/// The reviewed scope an active rule carries, recovered from its stored
/// manifest JSON. `None` when the row predates scope binding.
fn active_reviewed_scope(
    active: &crate::store::standing_rules::StandingRule,
) -> Option<openspine_schemas::reviewed_scope::ReviewedActionScope> {
    serde_json::from_str::<StandingRuleManifest>(&active.rule_json)
        .ok()?
        .reviewed_scope
        .map(|binding| binding.scope)
}

fn model_swap_arm(
    swap: &openspine_schemas::model_swap::ModelSwapManifest,
    digest: &Digest,
) -> Result<JudgePassed, JudgeDenial> {
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
    Ok(JudgePassed {
        verdict: "pass",
        fitness: None,
        evidence_json: evidence.to_string(),
        artifact_digest: digest.as_str().to_string(),
    })
}

/// Catalog membership and allow/deny consistency for the authority-bearing
/// kinds that carry declared action lists.
fn catalog_structural_arm(
    catalog: &ActionCatalog,
    proposal: &ParsedProposal,
    digest: &Digest,
) -> Result<JudgePassed, JudgeDenial> {
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
        ParsedProposal::StandingRule(_) => {
            unreachable!("every standing rule takes the reusable-authority arm")
        }
        ParsedProposal::Persona(_) => {}
        ParsedProposal::ModelSwap(_) => unreachable!("model swaps take the golden-set arm"),
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
        fitness: None,
        evidence_json: evidence.to_string(),
        artifact_digest: digest.as_str().to_string(),
    })
}

/// Whether `proposed` is a strictly broader grant than `incumbent`: it
/// constrains a subset of the incumbent's dimensions and agrees on every
/// shared one, so every context the incumbent admits the proposal admits too,
/// plus more.
///
/// Defence in depth, and deliberately so. `assemble` requires every dimension
/// the descriptor declares, and an incumbent's stored scope was derived
/// against the same descriptor, so today both maps hold exactly
/// Action + Descriptor + the required set and the length test cannot fire.
/// It becomes reachable if a descriptor revision ever *removes* a required
/// dimension, or if a rule row is migrated or hand-authored with a wider
/// scope than the current descriptor would derive. Refusing then is the
/// fail-closed answer, and the predicate is unit-tested directly rather than
/// through a runtime state that cannot currently be constructed.
fn widens(
    proposed: &std::collections::BTreeMap<
        openspine_schemas::action::ReviewedScopeDimension,
        openspine_schemas::reviewed_scope::ReviewedScopeValue,
    >,
    incumbent: &std::collections::BTreeMap<
        openspine_schemas::action::ReviewedScopeDimension,
        openspine_schemas::reviewed_scope::ReviewedScopeValue,
    >,
) -> bool {
    let agrees_on_shared = proposed
        .iter()
        .all(|(dimension, value)| incumbent.get(dimension) == Some(value));
    agrees_on_shared && proposed.len() < incumbent.len()
}

#[cfg(test)]
#[path = "judge_widening_tests.rs"]
mod judge_widening_tests;

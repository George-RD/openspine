//! The kernel-assembled evaluation input (#133).
//!
//! Every value the evaluators reason about is derived here, by the kernel,
//! from the canonical catalog, the executor registry, the active policy set,
//! and the live standing-rule table. The proposal contributes its own
//! declared content and nothing else: it cannot assert its own executor
//! readiness, its own policy standing, or its own evidence count. This is
//! D-146's "the kernel resolves every scope value" applied to evaluation.
//!
//! Like [`super::ReplayPassed`], the input has private fields and no public
//! constructor, so the only way to obtain one is [`assemble`] genuinely
//! resolving every dimension. A dimension that cannot be resolved is a typed
//! [`IncompleteInput`] naming it — never a pass, and never a fallback to
//! "treat it as a generic artifact".

use openspine_schemas::action::{ActionCatalog, ActionImplementationDescriptor};
use openspine_schemas::delegation_contract::ActionDescriptor;
use openspine_schemas::digest::Digest;
use openspine_schemas::standing_rule::{ReviewedScopeBinding, StandingRuleManifest};

use crate::artifact_loader::ParsedProposal;
use crate::store::standing_rules::StandingRule;
use crate::store::VerdictEpochs;

/// Why an evaluation input could not be assembled. Each variant names the
/// dimension the kernel could not resolve, so a denial is actionable rather
/// than a generic refusal.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IncompleteInput {
    #[error("evaluation input is missing dimension `{0}`")]
    MissingDimension(&'static str),
    #[error("action `{0}` names no concrete implementation descriptor")]
    NoImplementationDescriptor(String),
    #[error("reviewed scope binding is internally inconsistent: stored digest disagrees with stored values")]
    InconsistentScopeBinding,
    #[error("reviewed scope omits required dimension(s): {0}")]
    ScopeMissingRequiredDimensions(String),
}

/// The reusable-authority slice of an evaluation input: everything the
/// structural judge and the replay case set need about a proposed standing
/// rule, all of it kernel-derived.
pub(crate) struct ReusableAuthorityInput {
    manifest: StandingRuleManifest,
    /// Present only when the action carries a reusable-delegation descriptor.
    /// Its absence does NOT weaken the authority axes below — a standing rule
    /// admits an action without per-instance approval whatever its action's
    /// catalog shape, so executor readiness, policy deny, delegability and
    /// limits are always checked. Only the scope-bound axes (reviewed scope,
    /// overlap, executed-case replay) need the descriptor.
    scoped: Option<ScopeBoundInput>,
    catalogued: bool,
    non_delegable: bool,
    approval_narrowing: bool,
    executor_ready: bool,
    policy_denies_action: bool,
}

/// The scope-bound slice, present only for an action with a reusable-delegation
/// descriptor.
pub(crate) struct ScopeBoundInput {
    descriptor: ActionDescriptor,
    implementation: ActionImplementationDescriptor,
    binding: ReviewedScopeBinding,
    active_rules: Vec<StandingRule>,
}

impl ReusableAuthorityInput {
    pub(super) fn manifest(&self) -> &StandingRuleManifest {
        &self.manifest
    }
    pub(super) fn catalogued(&self) -> bool {
        self.catalogued
    }
    pub(super) fn non_delegable(&self) -> bool {
        self.non_delegable
    }
    /// Whether the action may carry a standing rule with no reviewed scope.
    pub(super) fn approval_narrowing(&self) -> bool {
        self.approval_narrowing
    }
    pub(super) fn executor_ready(&self) -> bool {
        self.executor_ready
    }
    pub(super) fn policy_denies_action(&self) -> bool {
        self.policy_denies_action
    }
    pub(super) fn scoped(&self) -> Option<&ScopeBoundInput> {
        self.scoped.as_ref()
    }
}

impl ScopeBoundInput {
    pub(super) fn descriptor(&self) -> &ActionDescriptor {
        &self.descriptor
    }
    pub(super) fn implementation(&self) -> &ActionImplementationDescriptor {
        &self.implementation
    }
    pub(super) fn binding(&self) -> &ReviewedScopeBinding {
        &self.binding
    }
    pub(super) fn active_rules(&self) -> &[StandingRule] {
        &self.active_rules
    }
}

/// What kind of evaluation this proposal gets. Only a reusable-authority
/// proposal — one that admits an action without per-instance owner approval
/// — carries the structural axes and the executed-case replay; the other
/// authority-bearing kinds keep the catalog-structural probes, and are
/// deliberately *not* described as replayed.
pub(crate) enum EvaluationSubject {
    ReusableAuthority(Box<ReusableAuthorityInput>),
    OtherAuthorityBearing,
}

/// A kernel-assembled, digest-bound evaluation input.
pub(crate) struct CanonicalEvaluationInput {
    subject: EvaluationSubject,
    epochs: VerdictEpochs,
}

impl CanonicalEvaluationInput {
    pub(super) fn epochs(&self) -> &VerdictEpochs {
        &self.epochs
    }
    /// The reusable-authority slice, when this proposal is one.
    pub(super) fn reusable(&self) -> Option<&ReusableAuthorityInput> {
        match &self.subject {
            EvaluationSubject::ReusableAuthority(input) => Some(input),
            EvaluationSubject::OtherAuthorityBearing => None,
        }
    }
}

/// Everything the kernel must supply to assemble an input. Passing this as
/// an explicit struct keeps [`assemble`] callable from tests without an
/// `AppState`, while still forbidding the proposal from contributing any of
/// these values.
pub(crate) struct AssemblySources<'a> {
    pub catalog: &'a ActionCatalog,
    /// Whether the action's catalogued implementation has a *registered*
    /// executor — the second, independent readiness axis (D-146).
    pub executor_ready: &'a dyn Fn(&openspine_schemas::action::ActionId) -> bool,
    /// The composed deny set in force. Deny beats everything, so a denied
    /// action can never become reusable authority.
    pub denied_actions: &'a [openspine_schemas::action::ActionId],
    /// Active standing rules for the proposed action, for overlap/widening.
    pub active_rules: Vec<StandingRule>,
    /// The active policy artifact version, recorded as an epoch.
    pub policy_version: Option<u32>,
}

/// Fold the whole active policy set into one epoch value.
///
/// A single `u32` must stand for "the policy set in force". Max-of-versions
/// would miss a NEW deny arriving at version 1 alongside an existing version 3,
/// so a stored verdict would not stale. Folding the sorted `(policy_id,
/// version)` set means any addition, removal or bump moves it. Propose time and
/// activation time MUST derive it the same way or every verdict reads stale, so
/// this is the single implementation both call.
pub(crate) fn policy_epoch<I, S>(policies: I) -> Option<u32>
where
    I: Iterator<Item = (S, u32)>,
    S: std::fmt::Display,
{
    let mut identity: Vec<(String, u32)> = policies
        .map(|(id, version)| (id.to_string(), version))
        .collect();
    if identity.is_empty() {
        return None;
    }
    identity.sort();
    let digest = openspine_schemas::digest::digest_of(
        &serde_json::to_value(&identity).unwrap_or(serde_json::Value::Null),
    );
    let bytes = digest.as_str().as_bytes();
    Some(u32::from_be_bytes([
        bytes[7], bytes[8], bytes[9], bytes[10],
    ]))
}

/// Assemble the evaluation input, or refuse naming the missing dimension.
pub(crate) fn assemble(
    proposal: &ParsedProposal,
    proposal_digest: &Digest,
    sources: AssemblySources<'_>,
) -> Result<CanonicalEvaluationInput, IncompleteInput> {
    let ParsedProposal::StandingRule(manifest) = proposal else {
        // Not a reusable-authority proposal: the structural catalog probes
        // still run, but there is no reviewed scope, executor, or budget to
        // resolve, and nothing will be called a replay.
        return Ok(CanonicalEvaluationInput {
            subject: EvaluationSubject::OtherAuthorityBearing,
            epochs: VerdictEpochs {
                proposal_digest: Some(proposal_digest.as_str().to_string()),
                policy_version: sources.policy_version,
                ..VerdictEpochs::default()
            },
        });
    };

    // A standing rule admits its action without per-instance owner approval,
    // whatever shape its action has in the catalog. The authority axes below
    // therefore always apply. Only the scope-bound axes need a
    // reusable-delegation descriptor, so its absence narrows what can be
    // checked — it never routes the rule to the weaker non-rule arm, which
    // would leave a non-delegable, unexecutable, policy-denied rule
    // unexamined (#133 done-when).
    let action_id = &manifest.action_id;
    let catalogued = sources.catalog.contains(action_id);
    let non_delegable = sources.catalog.is_non_delegable(action_id);
    let approval_narrowing = sources.catalog.is_approval_narrowing(action_id);
    let policy_denies_action = sources.denied_actions.contains(action_id);
    let executor_ready = (sources.executor_ready)(action_id);

    let scoped = match sources.catalog.delegation_descriptor_for(action_id) {
        None => None,
        Some(descriptor) => {
            let descriptor = descriptor.clone();
            let implementation = sources
                .catalog
                .implementation_descriptor_for_action(action_id)
                .cloned()
                .ok_or_else(|| {
                    IncompleteInput::NoImplementationDescriptor(action_id.to_string())
                })?;
            // The descriptor declares required dimensions, so a binding is
            // mandatory — the same rule #128 enforces at activation.
            let binding = manifest
                .reviewed_scope
                .as_ref()
                .ok_or(IncompleteInput::MissingDimension("reviewed_scope"))?
                .clone();
            if !binding.binding_is_valid() {
                return Err(IncompleteInput::InconsistentScopeBinding);
            }
            let bound = binding.scope.dimensions();
            let missing: Vec<String> = descriptor
                .required_scope_dimensions
                .iter()
                .filter(|dimension| !bound.contains_key(dimension))
                .map(|dimension| format!("{dimension:?}"))
                .collect();
            if !missing.is_empty() {
                return Err(IncompleteInput::ScopeMissingRequiredDimensions(
                    missing.join(", "),
                ));
            }
            Some(ScopeBoundInput {
                descriptor,
                implementation,
                binding,
                active_rules: sources.active_rules,
            })
        }
    };

    let epochs = VerdictEpochs {
        proposal_digest: Some(proposal_digest.as_str().to_string()),
        compatibility_digest: scoped
            .as_ref()
            .map(|s| s.binding.compatibility_digest.as_str().to_string()),
        reviewed_scope_digest: scoped
            .as_ref()
            .map(|s| s.binding.reviewed_scope_digest.as_str().to_string()),
        // Evidence lives on the owner-review object rather than the standing
        // rule manifest today, so a rule proposed without one records no
        // evidence epoch rather than a fabricated one.
        evidence_set_digest: None,
        descriptor_version: scoped.as_ref().map(|s| s.descriptor.descriptor_version),
        implementation_version: scoped
            .as_ref()
            .map(|s| s.implementation.implementation_version),
        policy_version: sources.policy_version,
    };

    Ok(CanonicalEvaluationInput {
        subject: EvaluationSubject::ReusableAuthority(Box::new(ReusableAuthorityInput {
            manifest: manifest.clone(),
            scoped,
            catalogued,
            non_delegable,
            approval_narrowing,
            executor_ready,
            policy_denies_action,
        })),
        epochs,
    })
}

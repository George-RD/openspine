//! Proposal-specific replay: concrete cases, actually executed (#133).
//!
//! There is no stored corpus of prior `ResolvedActionContext` rows, so cases
//! are *derived* from what the proposal already binds rather than retrieved:
//!
//! - the **baseline** case reconstructs the resolved context the reviewed
//!   scope describes, which the rule must admit — a rule that does not match
//!   the scope it was reviewed against is incoherent with its own
//!   justification;
//! - **changed-context** cases perturb exactly one bound instance dimension
//!   and must not match, and the observed mismatch must name that dimension.
//!
//! One dimension per case is what makes a failure attributable: a dimension
//! that is silently not compared shows up as a case that matched when it
//! should not have, rather than disappearing into an aggregate score.
//!
//! Every case is decided by [`ReviewedActionScope::compare`] against a real
//! [`ResolvedActionContext`] — the exact predicate `scoped_rule_matches`
//! applies at admission — so a matching bug fails replay and admission
//! together. Replay constructs contexts in memory, writes nothing, and
//! dispatches no connector effect.

use std::collections::BTreeMap;

use openspine_schemas::action::{ActionCatalog, ActionId, ReviewedScopeDimension};
use openspine_schemas::briefcase::{CounterpartyRef, RelationshipTier};
use openspine_schemas::digest::{digest_of_bytes, Digest};
use openspine_schemas::event::AccountRole;
use openspine_schemas::identity::RelationshipKind;
use openspine_schemas::resolved_context::{ResolvedActionContext, ResolvedActionContextInput};
use openspine_schemas::reviewed_scope::{ReviewedScopeValue, ScopeComparison};
use openspine_schemas::standing_rule::ReviewedScopeBinding;
use serde::Serialize;

/// What a case was constructed to test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CaseKind {
    /// The resolved context the reviewed scope describes: must match.
    ReviewedScopeBaseline,
    /// Exactly one bound instance dimension changed: must not match, and
    /// the mismatch must name that dimension.
    DimensionMutation,
}

/// What a case is required to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Expected {
    Matches,
    DoesNotMatch,
}

/// One executed case and its outcome.
#[derive(Debug, Clone, Serialize)]
pub(super) struct ExecutedCase {
    pub kind: CaseKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimension: Option<String>,
    pub expected: Expected,
    pub observed: Expected,
    /// For a mutation case, whether the observed mismatch named the very
    /// dimension the case changed. A mismatch attributed to some other
    /// dimension would mean the scope key is not distinguishing what the
    /// ledger claims it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributed: Option<bool>,
}

impl ExecutedCase {
    fn passed(&self) -> bool {
        self.expected == self.observed && self.attributed.unwrap_or(true)
    }
}

/// Why a case set could not be executed at all. A replay that cannot build
/// its baseline is a denial, never an empty pass.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CaseBuildError {
    #[error("reviewed scope carries no instance dimension to vary")]
    NoInstanceDimensions,
    #[error("could not reconstruct the reviewed context: {0}")]
    ContextUnbuildable(String),
}

/// The full executed-case ledger for one replay.
#[derive(Debug, Clone, Serialize)]
pub(super) struct CaseLedger {
    pub cases: Vec<ExecutedCase>,
}

impl CaseLedger {
    pub(super) fn matched_count(&self) -> usize {
        self.cases
            .iter()
            .filter(|case| case.observed == Expected::Matches)
            .count()
    }
    pub(super) fn refused_changed_context_count(&self) -> usize {
        self.cases
            .iter()
            .filter(|case| {
                case.kind == CaseKind::DimensionMutation && case.observed == Expected::DoesNotMatch
            })
            .count()
    }
    pub(super) fn failures(&self) -> Vec<&ExecutedCase> {
        self.cases.iter().filter(|case| !case.passed()).collect()
    }
    pub(super) fn mutated_dimensions(&self) -> Vec<&str> {
        self.cases
            .iter()
            .filter_map(|case| case.dimension.as_deref())
            .collect()
    }
    pub(super) fn len(&self) -> usize {
        self.cases.len()
    }
}

/// Build and execute the case set for a proposed binding.
pub(super) fn execute(
    catalog: &ActionCatalog,
    action_id: &ActionId,
    implementation_id: &openspine_schemas::action::ActionImplementationId,
    binding: &ReviewedScopeBinding,
) -> Result<CaseLedger, CaseBuildError> {
    let baseline_input = input_from_scope(binding);
    let baseline = ResolvedActionContext::try_new(
        catalog,
        action_id,
        implementation_id,
        baseline_input.clone(),
    )
    .map_err(|err| CaseBuildError::ContextUnbuildable(err.to_string()))?;

    let mut cases = vec![ExecutedCase {
        kind: CaseKind::ReviewedScopeBaseline,
        dimension: None,
        expected: Expected::Matches,
        observed: match binding.scope.compare(&baseline) {
            ScopeComparison::Matches => Expected::Matches,
            _ => Expected::DoesNotMatch,
        },
        attributed: None,
    }];

    let mut varied = 0usize;
    for dimension in binding.scope.dimensions().keys().copied() {
        let Some(mutated_input) = perturb_input(&baseline_input, dimension) else {
            continue;
        };
        varied += 1;
        let observed = match ResolvedActionContext::try_new(
            catalog,
            action_id,
            implementation_id,
            mutated_input,
        ) {
            // A context that no longer constructs at all is also a refusal:
            // the changed instance could not even become a candidate.
            Err(_) => (Expected::DoesNotMatch, Some(true)),
            Ok(context) => match binding.scope.compare(&context) {
                ScopeComparison::Matches => (Expected::Matches, Some(false)),
                ScopeComparison::InvalidReviewedScope => (Expected::DoesNotMatch, Some(true)),
                ScopeComparison::Mismatch { dimensions } => (
                    Expected::DoesNotMatch,
                    Some(dimensions.contains(&dimension)),
                ),
            },
        };
        cases.push(ExecutedCase {
            kind: CaseKind::DimensionMutation,
            dimension: Some(format!("{dimension:?}")),
            expected: Expected::DoesNotMatch,
            observed: observed.0,
            attributed: observed.1,
        });
    }

    if varied == 0 {
        return Err(CaseBuildError::NoInstanceDimensions);
    }
    Ok(CaseLedger { cases })
}

/// Reconstruct the instance inputs the reviewed scope describes. Declaration
/// axes (action, descriptor, implementation, egress) are not inputs — the
/// catalog supplies them inside `try_new`.
fn input_from_scope(binding: &ReviewedScopeBinding) -> ResolvedActionContextInput {
    let mut input = ResolvedActionContextInput {
        connector_instance_id: String::new(),
        account_role: None,
        account_identity_digest: None,
        target_refs: Vec::new(),
        counterparty: None,
        bound_parameters: BTreeMap::new(),
        target_digest: None,
        payload_digest: None,
        workflow_id: None,
        task_shape_digest: None,
    };
    for (dimension, value) in binding.scope.dimensions() {
        match (dimension, value) {
            (
                ReviewedScopeDimension::ConnectorInstance,
                ReviewedScopeValue::ConnectorInstance(v),
            ) => {
                input.connector_instance_id = v.clone();
            }
            (ReviewedScopeDimension::AccountRole, ReviewedScopeValue::AccountRole(v)) => {
                input.account_role = Some(*v);
            }
            (ReviewedScopeDimension::AccountIdentity, ReviewedScopeValue::AccountIdentity(v)) => {
                input.account_identity_digest = Some(v.clone());
            }
            (ReviewedScopeDimension::Target, ReviewedScopeValue::Target(v)) => {
                input.target_refs = v.refs.clone();
            }
            (ReviewedScopeDimension::Counterparty, ReviewedScopeValue::Counterparty(v)) => {
                input.counterparty = Some(CounterpartyRef::Bound {
                    identity_id: *v,
                    relationship: relationship_from(binding),
                });
            }
            (ReviewedScopeDimension::BoundParameters, ReviewedScopeValue::BoundParameters(v)) => {
                input.bound_parameters = v.clone();
            }
            (ReviewedScopeDimension::TargetDigest, ReviewedScopeValue::TargetDigest(v)) => {
                input.target_digest = Some(v.clone());
            }
            (ReviewedScopeDimension::PayloadDigest, ReviewedScopeValue::PayloadDigest(v)) => {
                input.payload_digest = Some(v.clone());
            }
            (ReviewedScopeDimension::Workflow, ReviewedScopeValue::Workflow(v)) => {
                input.workflow_id = Some(v.clone());
            }
            (ReviewedScopeDimension::TaskShape, ReviewedScopeValue::TaskShape(v)) => {
                input.task_shape_digest = Some(v.clone());
            }
            _ => {}
        }
    }
    input
}

/// The relationship kind implied by the reviewed relationship tier. The tier
/// is a reviewed dimension; the kind is how a bound counterparty carries it.
fn relationship_from(
    binding: &ReviewedScopeBinding,
) -> openspine_schemas::identity::RelationshipKind {
    use openspine_schemas::briefcase::RelationshipTier;
    use openspine_schemas::identity::RelationshipKind;
    // Pick a kind whose derived tier equals the reviewed tier, so the
    // reconstructed context carries the tier the owner actually reviewed.
    for kind in [
        RelationshipKind::Owner,
        RelationshipKind::Spouse,
        RelationshipKind::Family,
        RelationshipKind::Colleague,
        RelationshipKind::Client,
        RelationshipKind::Vendor,
        RelationshipKind::Unknown,
    ] {
        let tier: RelationshipTier = kind.into();
        if let Some(ReviewedScopeValue::RelationshipTier(reviewed)) = binding
            .scope
            .dimensions()
            .get(&ReviewedScopeDimension::RelationshipTier)
        {
            if &tier == reviewed {
                return kind;
            }
        }
    }
    RelationshipKind::Unknown
}

/// Change exactly one instance dimension in the input, or `None` when the
/// dimension is a declaration axis the catalog owns.
fn perturb_input(
    base: &ResolvedActionContextInput,
    dimension: ReviewedScopeDimension,
) -> Option<ResolvedActionContextInput> {
    let mut input = base.clone();
    match dimension {
        ReviewedScopeDimension::ConnectorInstance => {
            input.connector_instance_id = format!("{}-varied", base.connector_instance_id);
        }
        ReviewedScopeDimension::AccountIdentity => {
            input.account_identity_digest =
                Some(other_digest(base.account_identity_digest.as_ref()));
        }
        ReviewedScopeDimension::Target => {
            let mut refs = base.target_refs.clone();
            let first = refs.first_mut()?;
            first.id = Some(format!("{}-varied", first.id.clone().unwrap_or_default()));
            input.target_refs = refs;
        }
        ReviewedScopeDimension::TargetDigest => {
            input.target_digest = Some(other_digest(base.target_digest.as_ref()));
        }
        ReviewedScopeDimension::PayloadDigest => {
            input.payload_digest = Some(other_digest(base.payload_digest.as_ref()));
        }
        ReviewedScopeDimension::BoundParameters => {
            let mut params = base.bound_parameters.clone();
            let key = params.keys().next().cloned()?;
            let value = params.get(&key).cloned().unwrap_or_default();
            params.insert(key, format!("{value}-varied"));
            input.bound_parameters = params;
        }
        ReviewedScopeDimension::Workflow => {
            input.workflow_id = Some(format!(
                "{}-varied",
                base.workflow_id.clone().unwrap_or_default()
            ));
        }
        ReviewedScopeDimension::TaskShape => {
            input.task_shape_digest = Some(other_digest(base.task_shape_digest.as_ref()));
        }
        ReviewedScopeDimension::Counterparty => {
            let CounterpartyRef::Bound { relationship, .. } = base.counterparty.clone()? else {
                return None;
            };
            input.counterparty = Some(CounterpartyRef::Bound {
                identity_id: ulid::Ulid::new(),
                relationship,
            });
        }
        ReviewedScopeDimension::AccountRole => {
            input.account_role = Some(match base.account_role? {
                AccountRole::OwnerMailbox => AccountRole::SharedWorkspaceMailbox,
                _ => AccountRole::OwnerMailbox,
            });
        }
        ReviewedScopeDimension::RelationshipTier => {
            // The tier is carried by the bound counterparty's relationship
            // kind, so vary the kind to a one whose derived tier differs.
            let CounterpartyRef::Bound {
                identity_id,
                relationship,
            } = base.counterparty.clone()?
            else {
                return None;
            };
            let current: RelationshipTier = relationship.into();
            let varied = if current == RelationshipTier::Owner {
                RelationshipKind::Vendor
            } else {
                RelationshipKind::Owner
            };
            input.counterparty = Some(CounterpartyRef::Bound {
                identity_id,
                relationship: varied,
            });
        }
        // Declaration axes only: the catalog owns these, and the
        // compatibility epoch — not the scope key — detects their movement.
        // Listed explicitly rather than caught by `_` so a newly added
        // INSTANCE dimension fails to compile here instead of silently
        // going uncovered by any changed-context case (D-163).
        ReviewedScopeDimension::Action
        | ReviewedScopeDimension::Descriptor
        | ReviewedScopeDimension::ConnectorImplementation
        | ReviewedScopeDimension::EffectDestination
        | ReviewedScopeDimension::EgressClass
        | ReviewedScopeDimension::DisclosureClass
        | ReviewedScopeDimension::OutputChannel => return None,
    }
    Some(input)
}

/// A digest that is definitely different from the input, without inventing a
/// plausible-looking real identity.
fn other_digest(current: Option<&Digest>) -> Digest {
    let seed = current.map(|d| d.as_str().to_string()).unwrap_or_default();
    digest_of_bytes(format!("varied-for-evaluation:{seed}").as_bytes())
}

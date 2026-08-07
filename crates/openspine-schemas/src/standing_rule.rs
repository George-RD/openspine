//! Standing-rule artifact class (AD-010, AD-106, AD-012 leaning, AD-013).
//!
//! A standing rule is a versioned, revocable, expiring authority-composition
//! INPUT (never a second live authority object — D-007; the task grant
//! remains the only live authority object). It targets exactly one
//! [`ActionId`] and carries two independent sliding-window budgets — quota
//! (volume) and rate (velocity), per AD-106 — plus an optional dark-window
//! (AD-012 leaning) time-boxed conditional-default configuration.
//!
//! Shape mirrors [`crate::policy::Policy`]/[`crate::model_swap::ModelSwapManifest`]
//! (id/schema_version/version/lifecycle_state) so it slots into the existing
//! `artifact.propose` -> AD-142 eval-gate -> `artifact.activate` ceremony as
//! a seventh proposable kind.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::action::{ActionId, ReviewedScopeDimension};
use crate::artifact::Lifecycle;
use crate::digest::Digest;
use crate::ids::ArtifactId;
use crate::reviewed_scope::{reviewed_scope_digest_of, ReviewedActionScope, ReviewedScopeValue};

/// One sliding-window budget: at most `max` uses within the trailing
/// `window_secs`. Quota (volume, e.g. 5/week) and rate (velocity, e.g.
/// 1/hour) are each one of these (AD-106).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetWindow {
    pub max: u32,
    pub window_secs: i64,
}

/// AD-012 (leaning) dark-window conditional grant: "if you don't respond in
/// `timeout_secs`, I take pre-agreed default `default`." Highest-scrutiny
/// audit case — every fire is recorded as `standing_rule.dark_window_fired`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DarkWindowConfig {
    pub timeout_secs: i64,
    pub default: DarkWindowDefault,
    /// How many pending exceptions this rule version may hold outstanding at
    /// once (#135). The safe default is one: without a bound, a caller whose
    /// quota is exhausted can vary the payload and mint a fresh pending
    /// exception per variation, turning owner silence into an unbounded
    /// queue of waivers. Serialized so the owner review object states the
    /// bound they are approving.
    #[serde(default = "default_max_pending_exceptions")]
    pub max_pending_exceptions: u32,
}

/// One outstanding exception. Silence is the least-reviewed admission path,
/// so its default bound is the smallest one that still works.
pub fn default_max_pending_exceptions() -> u32 {
    1
}

/// The largest outstanding-exception allowance a reviewed rule may declare.
/// A dark window is an exception mechanism; a large allowance would make it a
/// second budget.
pub const MAX_PENDING_EXCEPTIONS_CEILING: u32 = 3;

/// The pre-agreed default a dark-window timer applies when the owner does
/// not respond in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DarkWindowDefault {
    Allow,
    Deny,
}

/// The reviewed scope a standing rule binds (design.md §"Reviewed values are
/// stored, not only their digest"). Matching delegates to the canonical
/// [`ReviewedActionScope::compare`] — the single comparison implementation in
/// `responsibility-contract` — and this binding never re-defines scope or
/// drift semantics. It stores the full [`ReviewedActionScope`] (dimensions +
/// reviewed values + derived digest) so comparison can name the exact changed
/// dimensions and a narrow need not re-review the rest, plus the standing-rule
/// scope key over exactly the required dimensions and the bound compatibility
/// epoch. A persisted binding whose stored values disagree with its derived
/// digest surfaces as the existing `ScopeComparison::InvalidReviewedScope`
/// outcome, never a bespoke error, so `responsibility-contract` and
/// `standing-rules` cannot drift apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedScopeBinding {
    /// The persisted reviewed scope. This is the authoritative comparison
    /// object; [`ReviewedActionScope::compare`] returns matches / exact
    /// mismatches / invalid-scope directly from it.
    pub scope: ReviewedActionScope,
    /// The standing-rule scope key: a sealed digest over exactly the required
    /// scope dimensions (the `ReviewedActionScope`'s dimension values minus
    /// the always-injected `Action`/`Descriptor` declaration keys). This is
    /// the fast-path SQL match key and is deliberately distinct from the
    /// compatibility epoch, so two different accounts or targets never
    /// collide into one pattern.
    pub reviewed_scope_digest: Digest,
    /// The compatibility epoch (drift epoch) of the context this rule was
    /// reviewed against — computed over declaration axes only.
    pub compatibility_digest: Digest,
}

impl ReviewedScopeBinding {
    /// Derive a binding from a sealed [`ReviewedActionScope`] plus the
    /// compatibility epoch of the reviewed context. The scope-key digest is
    /// derived from the scope's required-dimension values here so the two
    /// cannot drift apart.
    pub fn derive_from(scope: ReviewedActionScope, compatibility_digest: Digest) -> Self {
        let reviewed_scope_digest = required_scope_digest_of(&scope);
        Self {
            scope,
            reviewed_scope_digest,
            compatibility_digest,
        }
    }

    /// Whether the stored values agree with the stored scope digest. A
    /// persisted disagreement is an invalid scope that MUST fail closed as
    /// `ScopeComparison::InvalidReviewedScope` (via [`ReviewedActionScope::compare`]).
    pub fn binding_is_valid(&self) -> bool {
        self.scope.binding_is_valid()
            && self.reviewed_scope_digest == required_scope_digest_of(&self.scope)
    }
}

/// Sealed digest over exactly the required scope dimensions carried by a
/// [`ReviewedActionScope`] — its dimension values minus the always-injected
/// `Action`/`Descriptor` declaration keys, which the compatibility epoch owns.
fn required_scope_digest_of(scope: &ReviewedActionScope) -> Digest {
    let mut required: BTreeMap<ReviewedScopeDimension, ReviewedScopeValue> = BTreeMap::new();
    for (dimension, value) in scope.dimensions() {
        if matches!(
            dimension,
            ReviewedScopeDimension::Action | ReviewedScopeDimension::Descriptor
        ) {
            continue;
        }
        required.insert(*dimension, value.clone());
    }
    reviewed_scope_digest_of(&required)
}

/// The standing-rule artifact proposed via `artifact.propose { kind:
/// "standing_rule" }` and activated via `artifact.activate` after passing
/// the AD-142 offline-replay + risk-judge gate (`overlay_eval_gate`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandingRuleManifest {
    pub id: ArtifactId,
    pub schema_version: u32,
    #[serde(default = "crate::artifact::default_version")]
    pub version: u32,
    pub lifecycle_state: Lifecycle,
    /// The single action this rule authorizes without per-instance owner
    /// approval, subject to the budgets below (AD-010's composition-input
    /// invariant: this never becomes a second live authority source).
    pub action_id: ActionId,
    /// Plain-language rule statement shown to the owner at proposal time
    /// (AD-010: "agent-proposed, plain-language rules confirmed once").
    pub description: String,
    pub quota: BudgetWindow,
    pub rate: BudgetWindow,
    /// Lapse-after-unused expiry (AD-010: "e.g. lapse after 90 days
    /// unused"). Refreshed to `now + expires_after_secs` on every
    /// successful consumption; a rule that is never used lapses on its own.
    pub expires_after_secs: i64,
    #[serde(default)]
    pub dark_window: Option<DarkWindowConfig>,
    /// The reviewed scope binding that makes this rule eligible for scoped
    /// admission. `None` means the rule carries no scope binding and is not
    /// eligible for scoped matching (falls back to ordinary owner approval).
    #[serde(default)]
    pub reviewed_scope: Option<ReviewedScopeBinding>,
}

impl StandingRuleManifest {
    /// Positive-value invariants a manifest MUST satisfy before it may ever
    /// reach owner approval or activation (P1 finding: a non-positive
    /// `window_secs` makes every trailing-window count exclude all prior
    /// usage — `now - window_secs*1e9` lands at or after `now` — so both
    /// hard caps silently admit every request; a non-positive
    /// `dark_window.timeout_secs` collapses the conditional owner-response
    /// window into an immediately-due authority path instead of failing
    /// closed). Called at proposal-parse time (`artifact_loader`) and again
    /// at activation (`Store::activate_standing_rule`) as defense in depth.
    pub fn validate(&self) -> Result<(), String> {
        if self.quota.window_secs <= 0 {
            return Err("quota.window_secs must be positive".to_string());
        }
        if self.rate.window_secs <= 0 {
            return Err("rate.window_secs must be positive".to_string());
        }
        if self.expires_after_secs <= 0 {
            return Err("expires_after_secs must be positive".to_string());
        }
        if let Some(dark_window) = self.dark_window {
            if dark_window.timeout_secs <= 0 {
                return Err("dark_window.timeout_secs must be positive".to_string());
            }
            // A zero allowance would be a dark window that can never schedule
            // (silently dead policy); an unbounded one is the amplification
            // this bound exists to stop.
            if dark_window.max_pending_exceptions == 0 {
                return Err("dark_window.max_pending_exceptions must be at least 1".to_string());
            }
            if dark_window.max_pending_exceptions > MAX_PENDING_EXCEPTIONS_CEILING {
                return Err(format!(
                    "dark_window.max_pending_exceptions must not exceed {MAX_PENDING_EXCEPTIONS_CEILING}"
                ));
            }
        }
        if let Some(binding) = &self.reviewed_scope {
            if !binding.binding_is_valid() {
                return Err("reviewed_scope digest disagrees with its stored values".to_string());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn standing_rule_manifest_round_trips_through_serde() {
        let manifest = StandingRuleManifest {
            id: "appointment_booking".to_string(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Proposed,
            action_id: ActionId::new("calendar.book_appointment"),
            description: "Always approve appointment bookings, up to 5/week".to_string(),
            quota: BudgetWindow {
                max: 5,
                window_secs: 7 * 24 * 3600,
            },
            rate: BudgetWindow {
                max: 1,
                window_secs: 3600,
            },
            expires_after_secs: 90 * 24 * 3600,
            dark_window: Some(DarkWindowConfig {
                timeout_secs: 1800,
                default: DarkWindowDefault::Deny,
                max_pending_exceptions: 1,
            }),
            reviewed_scope: None,
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let back: StandingRuleManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest, back);
    }

    #[test]
    fn dark_window_is_optional_and_absent_by_default() {
        let yaml = "id: no_dark_window\nschema_version: 1\nversion: 1\nlifecycle_state: proposed\naction_id: telegram.reply:owner_channel\ndescription: test\nquota: {max: 1, window_secs: 60}\nrate: {max: 1, window_secs: 60}\nexpires_after_secs: 60\n";
        let manifest: StandingRuleManifest = serde_yaml::from_str(yaml).unwrap();
        assert!(manifest.dark_window.is_none());
        assert!(manifest.reviewed_scope.is_none());
    }

    #[test]
    fn reviewed_scope_binding_derives_digest_from_values_and_validates() {
        // Build a `ReviewedActionScope` from a resolved context, then a
        // binding; the scope-key digest derives from the required-dimension
        // values, and the corrupt case fails `binding_is_valid`.
        use crate::action::{
            ActionCatalog, ActionDescriptor, ActionImplementationDescriptor,
            ActionImplementationId, ActionSemantics, BudgetWindowBounds, DarkWindowPolicy,
            DataDestination, DelegationDefaults, DelegationPolicyBounds, DelegationProposalMode,
            EffectKind, EffectReversibility,
        };
        use crate::briefcase::CounterpartyRef;
        use crate::egress::EgressClass;
        use crate::event::{AccountRole, TargetRef, TargetRefKind};
        use crate::resolved_context::{ResolvedActionContext, ResolvedActionContextInput};
        use ulid::Ulid;

        fn digest(c: char) -> crate::digest::Digest {
            crate::digest::Digest::parse(format!("sha256:{}", c.to_string().repeat(64))).unwrap()
        }

        let action_id = ActionId::new("message.create_draft");
        let descriptor = ActionDescriptor {
            schema_version: 1,
            descriptor_version: 2,
            action_id: action_id.clone(),
            semantics: ActionSemantics {
                owner_verb: "create".into(),
                owner_object: "draft".into(),
                owner_target: "selected conversation".into(),
                effect_kind: EffectKind::OwnerAccountWrite,
                reversibility: EffectReversibility::Reversible,
                destination: DataDestination::OwnerCloudAccount,
            },
            reusable_delegation: true,
            required_scope_dimensions: BTreeSet::from([
                ReviewedScopeDimension::ConnectorImplementation,
                ReviewedScopeDimension::ConnectorInstance,
                ReviewedScopeDimension::AccountRole,
                ReviewedScopeDimension::AccountIdentity,
                ReviewedScopeDimension::Target,
                ReviewedScopeDimension::Counterparty,
                ReviewedScopeDimension::RelationshipTier,
                ReviewedScopeDimension::EffectDestination,
                ReviewedScopeDimension::Workflow,
                ReviewedScopeDimension::TaskShape,
            ]),
            delegation_policy: Some(DelegationPolicyBounds {
                schema_version: 1,
                policy_version: 3,
                quota: BudgetWindowBounds {
                    minimum_max: 1,
                    maximum_max: 20,
                    minimum_window_secs: 60,
                    maximum_window_secs: 30 * 24 * 3600,
                },
                rate: BudgetWindowBounds {
                    minimum_max: 1,
                    maximum_max: 5,
                    minimum_window_secs: 60,
                    maximum_window_secs: 24 * 3600,
                },
                maximum_lapse_secs: 90 * 24 * 3600,
                proposal_mode: DelegationProposalMode::DefaultsPermitted,
                defaults: Some(DelegationDefaults {
                    quota: BudgetWindow {
                        max: 5,
                        window_secs: 7 * 24 * 3600,
                    },
                    rate: BudgetWindow {
                        max: 1,
                        window_secs: 3600,
                    },
                    expires_after_secs: 90 * 24 * 3600,
                }),
                dark_window_policy: DarkWindowPolicy::Prohibited,
                fresh_target_selection_required: true,
            }),
        };
        let implementation = ActionImplementationDescriptor {
            schema_version: 1,
            implementation_version: 4,
            action_id: action_id.clone(),
            implementation_id: ActionImplementationId::new("matrix.message.create_draft"),
            connector_kind: "matrix".into(),
            executor_id: "matrix.draft.executor".into(),
            executor_version: 2,
            resolver_id: "matrix.draft.resolver".into(),
            resolver_version: 3,
        };
        let egress = crate::action::ActionEgressDeclaration {
            output_channels: Some(vec!["matrix.owner.draft".into()]),
            egress_class: Some(EgressClass::WebFormPost),
        };
        let implementation_id = implementation.implementation_id.clone();
        let catalog = ActionCatalog::new([action_id.clone()])
            .with_egress_declarations([(action_id.clone(), egress)])
            .with_delegation_descriptors([descriptor])
            .with_implementation_descriptors([implementation]);
        let input = ResolvedActionContextInput {
            connector_instance_id: "matrix-primary".into(),
            account_role: Some(AccountRole::OwnerMailbox),
            account_identity_digest: Some(digest('a')),
            target_refs: vec![TargetRef {
                kind: TargetRefKind::Conversation,
                id: Some("conversation-42".into()),
            }],
            counterparty: Some(CounterpartyRef::Bound {
                identity_id: Ulid::from(11_u128),
                relationship: crate::identity::RelationshipKind::Client,
            }),
            bound_parameters: BTreeMap::new(),
            target_digest: Some(digest('b')),
            payload_digest: Some(digest('c')),
            workflow_id: Some("reply_workflow".into()),
            task_shape_digest: Some(digest('d')),
        };
        let context =
            ResolvedActionContext::try_new(&catalog, &action_id, &implementation_id, input)
                .unwrap();
        let scope = ReviewedActionScope::derive(&context).unwrap();

        let compat = context.compatibility_digest().clone();
        let binding = ReviewedScopeBinding::derive_from(scope.clone(), compat);
        assert!(
            binding.binding_is_valid(),
            "derived binding is internally consistent"
        );

        // An inconsistent digest fails closed.
        let mut corrupt = binding.clone();
        corrupt.reviewed_scope_digest = crate::digest::Digest::parse(
            "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        )
        .unwrap();
        assert!(!corrupt.binding_is_valid());
    }
}

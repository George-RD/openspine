//! Action requests and the typed gate decision vocabulary (PRD design.md
//! "Execution boundary" group; precedence semantics live in `openspine-gate`).
//!
//! `DenialReason` and `GateDecision` are defined here (not in
//! `openspine-gate`) so both the wire/audit representation (this crate) and
//! the in-process `gate()` result type (`openspine-gate::Decision`) share
//! one vocabulary instead of two parallel enums.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use ulid::Ulid;

use crate::selection::SelectionTokenType;

use crate::artifact::ArtifactRef;
use crate::egress::EgressClass;
use crate::event::TargetRef;

/// Dotted action identifier, exact-match only (D-033) — e.g. `email.send`,
/// `telegram.reply:owner_channel`, `email.read_thread:selected_no_attachments`.
/// The `:qualifier` suffix is part of the identifier; there is no
/// wildcard/prefix matching in phases 0–3.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActionId(pub String);

impl ActionId {
    pub fn new(s: impl Into<String>) -> Self {
        ActionId(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ActionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for ActionId {
    fn from(s: &str) -> Self {
        ActionId(s.to_string())
    }
}

/// The canonical, immutable set of action ids the kernel recognizes
/// (D-053). An id absent from the catalog is outside the product's
/// action universe: authority composition rejects it and `gate()` denies it.
/// Known but unimplemented ids (e.g. `route.activate`) are still members —
/// the catalog governs *existence*, not *dispatching*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EffectPathClass {
    /// A shell-initiated read performed after the authority `gate()` has
    /// authorized it (e.g. `email.read_thread:selected_no_attachments`).
    GatedShell,
    /// An effect the kernel performs only after a `gate()` decision allowed
    /// it (e.g. creating an approved draft).
    PostGateApprovedEffect,
    /// A kernel-originated effect mediated by the authority `gate()` itself
    /// (e.g. `notify_owner_best_effort` routes through `gate()`).
    KernelOriginGated,
    /// Internal maintenance with no external effect (e.g. sweeping expired
    /// grants, answering a Telegram callback query).
    InternalMaintenanceNonEffect,
    /// A read the kernel performs during the pre-grant Verify stage,
    /// authorized by the verified-owner lane selection and the containment
    /// guard — NOT by the authority `gate()`. Captured so D-055's
    /// effect-path inventory is complete for every external read the kernel
    /// makes (e.g. resolving an email thread's recipient before packing).
    PreGateOwnerSelectedRead,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct EffectPath {
    pub name: String,
    pub classification: EffectPathClass,
}

/// The catalog-owned egress metadata for one registered action (blocker 1).
///
/// Both axes are *declared* per action and owned by the kernel catalog, never
/// derived from optional connector metadata. `None` on an axis means the
/// action carries no requirement on that axis (e.g. a non-egress action has
/// `egress_class: None`; a non-output action has `output_channels: None`).
/// An empty `Some(vec![])` on `output_channels` is a deliberate,
/// fail-closed declaration (the action is classified as delivering to a
/// channel but names none — the gate must deny).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActionEgressDeclaration {
    pub output_channels: Option<Vec<String>>,
    pub egress_class: Option<EgressClass>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActionCatalog {
    ids: HashSet<ActionId>,
    /// Actions the kernel itself may emit without a granting decision
    /// (D-055.3). A kernel-origin request for an action OUTSIDE this set is
    /// denied outright — the kernel may not reach for arbitrary actions
    /// without being explicitly enumerated as trusted.
    kernel_origin_actions: HashSet<ActionId>,
    /// Actions that may only be exercised by a root grant and therefore
    /// must never appear in a commissioned worker's attenuation.
    non_delegable_actions: HashSet<ActionId>,
    /// Actions that require a valid, grant-bound, unexpired selection token
    /// of the correct type before `gate()` will grant them (D-055.1).
    token_requiring_actions: HashMap<ActionId, SelectionTokenType>,
    /// Every effectful path that reaches around `gate()` (D-055.1).
    effect_paths: Vec<EffectPath>,
    /// Actions whose effects face an external counterparty (AD-151). A gate
    /// denial for these surfaces the canonical deferral + owner escalation;
    /// all other denials return ordinary enum outcomes only. Kernel-owned
    /// catalog metadata — never shell-spoofable, never on TaskGrant.
    counterparty_facing_actions: HashSet<ActionId>,
    /// Catalog-owned egress metadata for every registered action (blocker 1):
    /// mandatory output-channel + egress-class declaration. Enforcement reads
    /// ONLY this map; connector metadata is never consulted.
    egress_declarations: HashMap<ActionId, ActionEgressDeclaration>,
}

impl ActionCatalog {
    pub fn new(ids: impl IntoIterator<Item = ActionId>) -> Self {
        ActionCatalog {
            ids: ids.into_iter().collect(),
            kernel_origin_actions: HashSet::new(),
            non_delegable_actions: HashSet::new(),
            token_requiring_actions: HashMap::new(),
            effect_paths: Vec::new(),
            counterparty_facing_actions: HashSet::new(),
            egress_declarations: HashMap::new(),
        }
    }

    pub fn contains(&self, id: &ActionId) -> bool {
        self.ids.contains(id)
    }

    /// Mark the given actions as kernel-origin (D-055.3): trusted to run
    /// without a granting decision when `gate()` is called with
    /// [`ActionOrigin::Kernel`]. Returns `self` for chaining.
    pub fn with_kernel_origin(mut self, actions: impl IntoIterator<Item = ActionId>) -> Self {
        self.kernel_origin_actions = actions.into_iter().collect();
        self
    }

    /// Every action id the catalog recognizes. Used by the completeness test
    /// to assert each carries a mandatory egress declaration (blocker 1).
    pub fn action_ids(&self) -> &HashSet<ActionId> {
        &self.ids
    }

    /// Mark actions that worker commission may never delegate. Returns
    /// `self` for chaining.
    pub fn with_non_delegable(mut self, actions: impl IntoIterator<Item = ActionId>) -> Self {
        self.non_delegable_actions = actions.into_iter().collect();
        self
    }

    /// True when `id` is catalogued as root-only and non-delegable.
    /// Unknown actions return false; catalog membership is validated
    /// independently by authority composition and the gate.
    pub fn is_non_delegable(&self, id: &ActionId) -> bool {
        self.non_delegable_actions.contains(id)
    }

    /// Mark the given actions as requiring a selection token of the expected type (D-055.1):
    /// `gate()` validates possession of a matching, unexpired, grant-bound
    /// selection token before granting them. Returns `self` for chaining.
    pub fn with_token_requiring(
        mut self,
        actions: impl IntoIterator<Item = (ActionId, SelectionTokenType)>,
    ) -> Self {
        self.token_requiring_actions = actions.into_iter().collect();
        self
    }

    /// Enumerate effect paths (D-055.1). Returns `self` for chaining.
    pub fn with_effect_paths(mut self, paths: impl IntoIterator<Item = EffectPath>) -> Self {
        self.effect_paths = paths.into_iter().collect();
        self
    }

    /// Mark actions whose effects face an external counterparty (AD-151).
    /// Unknown/unclassified actions fail closed to non-counterparty.
    /// Returns `self` for chaining.
    pub fn with_counterparty_facing(mut self, actions: impl IntoIterator<Item = ActionId>) -> Self {
        self.counterparty_facing_actions = actions.into_iter().collect();
        self
    }

    /// Mark the given actions as delivering to a specific named output
    /// channel. `gate()` denies the action unless the grant's caveat chain
    /// effectively allows that channel. Stored as the catalog-owned egress
    /// declaration (blocker 1); connector metadata is never consulted.
    pub fn with_output_channels(
        mut self,
        actions: impl IntoIterator<Item = (ActionId, Vec<String>)>,
    ) -> Self {
        for (id, channels) in actions {
            self.egress_declarations.insert(
                id,
                ActionEgressDeclaration {
                    output_channels: Some(channels),
                    egress_class: None,
                },
            );
        }
        self
    }

    /// Declare the catalog-owned egress metadata for a set of actions
    /// (blocker 1). Every registered action MUST have an explicit declaration
    /// (use `None` on an axis when the action carries no requirement there);
    /// the gate fails closed on any registered action missing its declaration.
    pub fn with_egress_declarations(
        mut self,
        declarations: impl IntoIterator<Item = (ActionId, ActionEgressDeclaration)>,
    ) -> Self {
        self.egress_declarations = declarations.into_iter().collect();
        self
    }

    /// True if a denial of `id` faces an external counterparty and must
    /// surface the canonical deferral + owner escalation (AD-151).
    /// Unknown/unclassified actions return false (fail closed).
    pub fn is_counterparty_facing(&self, id: &ActionId) -> bool {
        self.counterparty_facing_actions.contains(id)
    }

    /// True if `id` is a kernel-origin action trusted to bypass the granting
    /// decision (D-055.3).
    pub fn is_kernel_origin(&self, id: &ActionId) -> bool {
        self.kernel_origin_actions.contains(id)
    }

    /// If `id` requires a selection token, returns the expected token type (D-055.1).
    pub fn requires_selection_token(&self, id: &ActionId) -> Option<&SelectionTokenType> {
        self.token_requiring_actions.get(id)
    }

    /// If `id` carries a catalog egress declaration, returns it. A registered
    /// action WITHOUT a declaration is a catalog-integrity violation (blocker 1);
    /// the gate fails closed on its absence rather than treating it as unknown.
    pub fn egress_decl_for(&self, id: &ActionId) -> Option<&ActionEgressDeclaration> {
        self.egress_declarations.get(id)
    }

    /// If `id` is declared as a rated egress endpoint, returns its class.
    pub fn egress_class_for(&self, id: &ActionId) -> Option<EgressClass> {
        self.egress_decl_for(id).and_then(|d| d.egress_class)
    }

    /// If `id` delivers to one or more named output channels, returns them.
    pub fn output_channel_for(&self, id: &ActionId) -> Option<&[String]> {
        self.egress_decl_for(id)
            .and_then(|d| d.output_channels.as_deref())
    }

    pub fn effect_paths(&self) -> &[EffectPath] {
        &self.effect_paths
    }
}

/// Why `gate()` denied an action (Step 3 of the build plan; exhaustive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DenialReason {
    NotGranted,
    ExplicitDeny,
    GrantExpired,
    ApprovalMissing,
    ApprovalDigestMismatch,
    ApprovalExpired,
    SelectionTokenInvalid,
    KernelOriginNotTrusted,
    ChannelBindingViolation,
    LimitExceeded,
    UnknownAction,
    CaveatWidening,
    /// AD-060: the action targets a rated egress endpoint whose class is not
    /// covered by the grant's allowed egress classes.
    EgressClassNotGranted,
    /// The action delivers to a named output channel not effectively
    /// allowed by the grant's caveat chain (AD-035 reply chokepoint).
    OutputChannelNotGranted,
}

/// The typed outcome of mediating one action request.
/// `EffectSuppressed` is deliberately non-executable: dispatch paths match
/// only `Allow` and therefore cannot accidentally run shadow effects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum GateDecision {
    Allow,
    Deny { reason: DenialReason },
    ApprovalRequired { approval_type: String },
    EffectSuppressed,
}

/// A typed request to perform one effectful action, submitted to `gate()`.
///
/// `target_digest` is not part of any literal PRD example — it exists
/// because a digest-bound approval (D-011) binds a target digest whose
/// composition is action-specific (e.g. Step 6's email draft target digest
/// hashes `{thread_id, connector, account_role, to}`, not a generic
/// `TargetRef`). `ActionRequest` stays domain-agnostic by letting the
/// caller compute and attach whatever target digest that action's approval
/// flow requires; `payload_digest` needs no separate field since it is
/// always `payload_ref.digest` when a payload ref is present.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum SkillAttributionKind {
    #[default]
    Causal,
    Contextual {
        skills: Vec<String>,
        omitted: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillAttribution {
    pub id: String,
    pub version: u32,
    #[serde(default)]
    pub kind: SkillAttributionKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionRequest {
    pub id: Ulid,
    pub task_grant_id: Ulid,
    pub action: ActionId,
    pub target_ref: Option<TargetRef>,
    pub payload_ref: Option<ArtifactRef>,
    pub target_digest: Option<crate::digest::Digest>,
    #[serde(default)]
    pub selection_token_id: Option<Ulid>,
    /// Actual request parameters submitted with the action (blocker 2). The
    /// gate enforces any `BoundParameter` caveats on the grant against these
    /// exact values; a bound name missing here or carrying a different value
    /// is a caveat violation, not a silent pass.
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    #[serde(default)]
    pub skill_attribution: Option<SkillAttribution>,
    pub requested_at: jiff::Timestamp,
    pub schema_version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_id_qualifier_is_part_of_identity() {
        let unqualified = ActionId::new("email.read_thread");
        let qualified = ActionId::new("email.read_thread:selected_no_attachments");
        assert_ne!(unqualified, qualified);
    }

    #[test]
    fn action_id_serializes_as_bare_string() {
        let id = ActionId::new("telegram.reply:owner_channel");
        assert_eq!(
            serde_json::to_value(&id).unwrap(),
            serde_json::json!("telegram.reply:owner_channel")
        );
    }

    #[test]
    fn gate_decision_round_trips_with_tag() {
        let decision = GateDecision::Deny {
            reason: DenialReason::ExplicitDeny,
        };
        let value = serde_json::to_value(&decision).unwrap();
        assert_eq!(value["outcome"], "deny");
        assert_eq!(value["reason"], "explicit_deny");
        let back: GateDecision = serde_json::from_value(value).unwrap();
        assert_eq!(decision, back);
    }

    #[test]
    fn approval_required_never_serializes_as_allow() {
        let decision = GateDecision::ApprovalRequired {
            approval_type: "email.create_draft".to_string(),
        };
        let value = serde_json::to_value(&decision).unwrap();
        assert_eq!(value["outcome"], "approval_required");
        assert_ne!(value["outcome"], "allow");
    }
}

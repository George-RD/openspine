//! The kernel-owned briefcase (AD-121): deterministic per-task context
//! packing with typed visibility classes.
//!
//! Canon: AD-021 (pack deterministically from task shape: route × workflow ×
//! counterparty — grant + relevant preferences + relevant skills +
//! counterparty slice), AD-031 (depth = f(relationship tier × task class);
//! worker top-ups are gate-visible), AD-032 (the kernel packs — the master
//! agent only proposes/requests, avoiding confused-deputy over-packing from
//! poisoned upstream context), AD-121 (kernel-owned blackboard; keys typed
//! kernel-bound / worker-scratch / returned-output; a fog-of-war visibility
//! schema records what each worker can see).
//!
//! Everything here is pure data + pure functions, matching this crate's
//! no-I/O contract. The kernel (which owns the mutable `Briefcase` and
//! selects real sources from the store/registry) lives in
//! `openspine-kernel::briefcase`.
//!
//! ## Determinism is over (shape, source snapshot), not shape alone
//!
//! "Identical task shape yields an identical pack" is only an honest claim
//! if every input the pack draws from is either part of the shape or
//! explicitly encoded on the pack's own surface. [`PackSources`] is
//! content-addressed (`snapshot_id`) and that digest becomes
//! [`Briefcase::source_snapshot_id`] — so two packs are byte-identical iff
//! both the shape AND the source snapshot match, and that fact is visible
//! on the pack itself rather than a hidden extra input. Callers (the
//! kernel) MUST build [`PackSources::grant_view`] from a *stable semantic*
//! projection of the grant (allowed/denied/approval-required actions,
//! limits, provenance ids) — never from instance-unique fields
//! (`id`, `task_token`, `issued_at`, `expires_at`, `event_id`,
//! `selection_tokens`, `chain`, `caveat_mac`, lineage). Two independently
//! minted grants for the same task shape differ in exactly those instance
//! fields; if any leaked into `grant_view`, same-shape determinism would be
//! false for every real dispatch, not just a contrived same-object test.
//!
//! ## Structural confused-deputy defense (AD-032)
//!
//! [`Briefcase::apply_top_up`] and every other mutator take `&mut self`.
//! Workers never receive `&Briefcase`/`&mut Briefcase` — only a
//! [`BriefcaseView`] (an owned, filtered, mutator-free projection) minted by
//! [`Briefcase::view_for`]. There is no reachable API for a worker to add,
//! widen, or reclassify a section; only the kernel (which alone holds the
//! `Briefcase`) can call the mutators.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ulid::Ulid;

use crate::digest::{canonical_json, digest_of, Digest};
use crate::identity::RelationshipKind;
use crate::ids::ArtifactId;

/// Relationship tier (AD-031's first depth axis): a coarsening of
/// [`RelationshipKind`] onto the ordered scale the depth function and the
/// top-up policy key off of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipTier {
    Stranger,
    Known,
    Intimate,
    Owner,
}

impl From<RelationshipKind> for RelationshipTier {
    fn from(kind: RelationshipKind) -> Self {
        match kind {
            RelationshipKind::Owner => RelationshipTier::Owner,
            RelationshipKind::Spouse | RelationshipKind::Family => RelationshipTier::Intimate,
            RelationshipKind::Colleague | RelationshipKind::Client | RelationshipKind::Vendor => {
                RelationshipTier::Known
            }
            RelationshipKind::Unknown => RelationshipTier::Stranger,
        }
    }
}

/// Task class (AD-031's second depth axis). Concretizes AD-031's "task
/// class" — no such field exists yet on [`crate::workflow::WorkflowManifest`];
/// see the implementing change's IMPLEMENTATION-NOTES for the proposed
/// D-0XX canon entry naming this derivation. *leaning*.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum TaskClass {
    #[default]
    Conversation,
    DraftApproval,
    Effectful,
}

/// Kernel-owned blackboard visibility classes (AD-121).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisibilityClass {
    /// Stays in the kernel; never reaches a worker's view.
    KernelBound,
    /// Packed into the worker's working context.
    WorkerScratch,
    /// The worker's structured, schema-checked outbound result (AD-033).
    ReturnedOutput,
}

/// What a packed section actually is (AD-021's four pack ingredients, minus
/// the counterparty slice's own kind marker).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    Grant,
    Preference,
    Skill,
    CounterpartySlice,
}

/// The counterparty half of a task shape. A bound counterparty carries an
/// identity id and its relationship to the owner; an unresolved counterparty
/// (no identity-store binding yet) carries only a channel + identifier hint
/// and is treated as a stranger for depth/visibility purposes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CounterpartyRef {
    Bound {
        identity_id: Ulid,
        relationship: RelationshipKind,
    },
    /// No identity-store binding exists for this counterparty yet. The kernel
    /// MUST NOT masquerade another Ulid (e.g. a grant/event id) as an
    /// identity id here; production deployments SHOULD bind counterparties
    /// before effectful use.
    Unresolved { channel: String, identifier: String },
}

impl CounterpartyRef {
    /// The relationship tier this counterparty maps to.
    pub fn tier(&self) -> RelationshipTier {
        match self {
            CounterpartyRef::Bound { relationship, .. } => (*relationship).into(),
            CounterpartyRef::Unresolved { .. } => RelationshipTier::Stranger,
        }
    }
}

/// The determinism key (AD-021): route × workflow × counterparty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskShape {
    pub route_id: ArtifactId,
    pub workflow_id: ArtifactId,
    pub counterparty: CounterpartyRef,
}

/// One content-addressable source slice a pack draws a section from.
/// Deliberately generic over payload shape: no first-class
/// preference/skill artifact kind exists yet (see IMPLEMENTATION-NOTES).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SourceSlice {
    pub key: String,
    pub payload: Value,
    /// Minimum requested depth for this deterministic source rank.
    #[serde(default = "default_source_depth")]
    pub minimum_depth: u8,
}

fn default_source_depth() -> u8 {
    1
}

/// A candidate preference/skill artifact eligible for packing, tagged with
/// the shape dimensions that gate its relevance. AD-021: "1000 learned
/// things, ~5 per task" — most candidates in a real pool are irrelevant to
/// any given shape and must be filtered out, not packed wholesale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearnedSource {
    pub key: String,
    pub kind: SectionKind,
    pub payload: Value,
    /// Empty ⇒ applies at every tier.
    #[serde(default)]
    pub applicable_tiers: Vec<RelationshipTier>,
    /// Empty ⇒ applies to every workflow.
    #[serde(default)]
    pub applicable_workflows: Vec<ArtifactId>,
}

impl LearnedSource {
    fn is_relevant(&self, workflow_id: &str, tier: RelationshipTier) -> bool {
        (self.applicable_tiers.is_empty() || self.applicable_tiers.contains(&tier))
            && (self.applicable_workflows.is_empty()
                || self.applicable_workflows.iter().any(|w| w == workflow_id))
    }
}

/// Filter a candidate pool down to the sources relevant to this task shape,
/// split by kind, deterministically sorted by key. Pure — the same pool +
/// shape always yields the same selection, which is what makes packing's
/// determinism claim meaningful beyond "the code ran the same way twice."
pub fn select_relevant_sources(
    pool: &[LearnedSource],
    workflow_id: &str,
    tier: RelationshipTier,
) -> (Vec<SourceSlice>, Vec<SourceSlice>) {
    let mut all: Vec<(SectionKind, SourceSlice)> = pool
        .iter()
        .filter(|s| matches!(s.kind, SectionKind::Preference | SectionKind::Skill))
        .filter(|s| s.is_relevant(workflow_id, tier))
        .map(|s| {
            (
                s.kind,
                SourceSlice {
                    key: s.key.clone(),
                    payload: s.payload.clone(),
                    minimum_depth: 1,
                },
            )
        })
        .collect();
    all.sort_by(|(kind_a, a), (kind_b, b)| a.key.cmp(&b.key).then_with(|| kind_a.cmp(kind_b)));
    for (rank, (_, source)) in all.iter_mut().enumerate() {
        source.minimum_depth = (rank + 1).min(u8::MAX as usize) as u8;
    }
    let preferences = all
        .iter()
        .filter(|(kind, _)| *kind == SectionKind::Preference)
        .map(|(_, source)| source.clone())
        .collect();
    let skills = all
        .into_iter()
        .filter(|(kind, _)| *kind == SectionKind::Skill)
        .map(|(_, source)| source)
        .collect();
    (preferences, skills)
}

/// The exact, content-addressed bundle of inputs a pack is built from.
/// Digesting this (not just the shape) is what makes "identical shape ⇒
/// identical pack" an honest, testable claim — see the module-level note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackSources {
    /// A stable *semantic* projection of the task grant — never its
    /// instance-unique fields. See the module-level determinism note.
    pub grant_view: Value,
    #[serde(default)]
    pub preferences: Vec<SourceSlice>,
    #[serde(default)]
    pub skills: Vec<SourceSlice>,
    pub counterparty_slice: Value,
}

impl PackSources {
    pub fn snapshot_id(&self) -> Digest {
        digest_of(&serde_json::to_value(self).expect("PackSources always serializes"))
    }
}

/// One packed key, typed by kind and visibility class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BriefcaseSection {
    pub key: String,
    pub kind: SectionKind,
    pub visibility: VisibilityClass,
    pub depth: u8,
    /// Kernel-classified provenance for deterministic disclosure checks.
    /// `None` is legacy/unknown and must fail closed for rated egress.
    #[serde(default)]
    pub disclosure_class: Option<crate::disclosure_policy::DisclosureClass>,
    pub payload: Value,
}

/// The packed, kernel-owned blackboard for one task (AD-121). Only the
/// kernel ever holds `&mut Briefcase` (see module-level confused-deputy
/// note) — everything a worker sees comes from [`Briefcase::view_for`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Briefcase {
    pub schema_version: u32,
    pub task_shape: TaskShape,
    pub source_snapshot_id: Digest,
    pub depth: u8,
    pub tier: RelationshipTier,
    pub class: TaskClass,
    /// Sorted by `key` — the determinism invariant `pack`/`apply_top_up`
    /// uphold on every mutation.
    pub sections: Vec<BriefcaseSection>,
    #[serde(default)]
    pub top_up_log: Vec<TopUpDecision>,
}

/// AD-031: briefcase depth = f(relationship tier × task class). Pure table;
/// n=1 (a static weighting, not yet learned/mined — mirrors AD-122's
/// analogous n=1 static-tier-map precedent for the effort router).
pub fn depth(tier: RelationshipTier, class: TaskClass) -> u8 {
    let tier_weight: u8 = match tier {
        RelationshipTier::Stranger => 1,
        RelationshipTier::Known => 2,
        RelationshipTier::Intimate => 3,
        RelationshipTier::Owner => 4,
    };
    let class_weight: u8 = match class {
        TaskClass::Conversation => 1,
        TaskClass::DraftApproval => 2,
        TaskClass::Effectful => 2,
    };
    tier_weight * class_weight
}

/// AD-021/AD-032: pack this task's briefcase. Pure and deterministic over
/// `(shape, sources, tier, class)` — see the module-level determinism note
/// for what that claim depends on.
pub fn pack(
    shape: TaskShape,
    sources: &PackSources,
    tier: RelationshipTier,
    class: TaskClass,
) -> Briefcase {
    let depth_val = depth(tier, class);
    let snapshot_id = sources.snapshot_id();
    let mut sections = vec![BriefcaseSection {
        key: "grant".to_string(),
        kind: SectionKind::Grant,
        visibility: VisibilityClass::KernelBound,
        depth: depth_val,
        disclosure_class: Some(crate::disclosure_policy::DisclosureClass::Private),
        payload: sources.grant_view.clone(),
    }];
    let mut eligible: Vec<(SectionKind, &SourceSlice)> = sources
        .preferences
        .iter()
        .map(|source| (SectionKind::Preference, source))
        .chain(
            sources
                .skills
                .iter()
                .map(|source| (SectionKind::Skill, source)),
        )
        .collect();
    eligible.sort_by(|(kind_a, a), (kind_b, b)| a.key.cmp(&b.key).then_with(|| kind_a.cmp(kind_b)));
    for (kind, source) in eligible.into_iter().take(depth_val as usize) {
        sections.push(BriefcaseSection {
            key: format!("{:?}:{}", kind, source.key).to_lowercase(),
            kind,
            visibility: VisibilityClass::WorkerScratch,
            depth: depth_val,
            disclosure_class: Some(crate::disclosure_policy::DisclosureClass::Private),
            payload: source.payload.clone(),
        });
    }
    sections.push(BriefcaseSection {
        key: "counterparty_slice".to_string(),
        kind: SectionKind::CounterpartySlice,
        visibility: VisibilityClass::WorkerScratch,
        depth: depth_val,
        disclosure_class: Some(crate::disclosure_policy::DisclosureClass::Internal),
        payload: sources.counterparty_slice.clone(),
    });
    sections.sort_by(|a, b| a.key.cmp(&b.key));
    Briefcase {
        schema_version: 1,
        task_shape: shape,
        source_snapshot_id: snapshot_id,
        depth: depth_val,
        tier,
        class,
        sections,
        top_up_log: Vec::new(),
    }
}

/// Errors from briefcase visibility/top-up operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BriefcaseError {
    #[error("section {0:?} not found in briefcase")]
    SectionNotFound(String),
    #[error("section {key:?} is {actual:?}, not exportable as returned-output")]
    VisibilityViolation {
        key: String,
        actual: VisibilityClass,
    },
    #[error("top-up for {0:?} was not allowed; refusing to apply it")]
    TopUpNotAllowed(String),
    #[error("top-up request {0} has already been decided")]
    TopUpReplay(Ulid),
    #[error("top-up source binding did not match the requested source")]
    TopUpSourceMismatch,
    #[error("top-up would exceed the briefcase aggregate depth budget")]
    TopUpDepthExceeded,
}

/// A per-worker visibility record (AD-121's fog-of-war schema): which
/// classes this worker's view is filtered to. `KernelBound` is excluded
/// structurally by [`Briefcase::view_for`] regardless of this set's
/// contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerVisibility {
    pub worker_id: Ulid,
    pub allowed: BTreeSet<VisibilityClass>,
}

impl WorkerVisibility {
    /// The default visibility for a commissioned worker (AD-030 intern
    /// principle / AD-035 master-never-worker): sees only its packed
    /// working context, never the kernel-bound grant internals.
    pub fn worker_default(worker_id: Ulid) -> Self {
        let mut allowed = BTreeSet::new();
        allowed.insert(VisibilityClass::WorkerScratch);
        Self { worker_id, allowed }
    }
}

mod ops;
mod topup;
pub use topup::*;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod topup_tests;

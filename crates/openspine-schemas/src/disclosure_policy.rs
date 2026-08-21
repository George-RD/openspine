//! Relationship-scoped disclosure policy and deterministic egress checks.
//!
//! An outbound query assembled from a private briefcase is an effect even when
//! query text is generalized.  The gate therefore checks immutable classified
//! provenance, never the post-generalization text and never an LLM judgment.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::action::ActionId;
use crate::artifact::{ArtifactRef, Lifecycle};
use crate::digest::{digest_of_bytes, Digest};
use crate::egress::EgressClass;
use crate::event::DataClassification;
use crate::identity::RelationshipKind;
use crate::ids::ArtifactId;
use crate::provenance::ProvenanceOrigin;

/// Disclosure sensitivity carried by a classified briefcase item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisclosureClass {
    Public,
    Internal,
    Private,
    Sensitive,
}

impl DisclosureClass {
    /// Public context does not disclose private information and needs no
    /// relationship-scoped policy. Every other class is policy-covered.
    pub const fn requires_policy(self) -> bool {
        !matches!(self, Self::Public)
    }
}

/// The single authoritative mapping from the [`DataClassification`] event
/// vocabulary into the authoritative [`DisclosureClass`] label vocabulary.
/// `Unknown` maps to `Sensitive` — the most-restrictive, fail-closed class —
/// so an unclassified datum is never treated as safe to disclose.
impl From<DataClassification> for DisclosureClass {
    fn from(class: DataClassification) -> Self {
        match class {
            DataClassification::Public => Self::Public,
            DataClassification::Internal => Self::Internal,
            DataClassification::Private => Self::Private,
            DataClassification::Unknown => Self::Sensitive,
        }
    }
}

/// Immutable provenance for one item packed into a briefcase. The payload is
/// referenced by digest; only its deterministic disclosure class crosses into
/// the egress check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassifiedBriefcaseItem {
    pub item_ref: ArtifactRef,
    pub disclosure_class: DisclosureClass,
    /// The kernel-derived typed-identity origin this datum was produced under
    /// (spec #220 / D-174). `None` is an unresolved/legacy origin and, like an
    /// unclassified `disclosure_class`, must be treated as most-restrictive by
    /// any egress-time origin check (the deterministic origin-vs-recipient
    /// closure lands in the egress ticket, #225–#227). Kernel-derived, never
    /// worker-set: the whole item is minted only inside `provenance_from_sections`.
    #[serde(default)]
    pub origin: Option<ProvenanceOrigin>,
}

/// Provenance set carried by an outbound query.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisclosureProvenance {
    #[serde(default)]
    pub items: Vec<ClassifiedBriefcaseItem>,
}

impl DisclosureProvenance {
    /// Classes are derived from immutable item metadata, not from query text.
    pub fn classes(&self) -> BTreeSet<DisclosureClass> {
        self.items
            .iter()
            .map(|item| item.disclosure_class)
            .collect()
    }

    pub fn contains_private_context(&self) -> bool {
        self.items
            .iter()
            .any(|item| item.disclosure_class.requires_policy())
    }
}

/// The stable identity of a relationship-scoped disclosure policy. Keyed by the
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisclosurePolicyKey {
    pub relationship: RelationshipKind,
    pub disclosure_class: DisclosureClass,
}

/// A reviewed policy keyed by relationship and disclosure class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisclosurePolicy {
    pub id: ArtifactId,
    pub schema_version: u32,
    #[serde(default = "crate::artifact::default_version")]
    pub version: u32,
    pub lifecycle_state: Lifecycle,
    pub key: DisclosurePolicyKey,
    #[serde(default)]
    pub allowed_egress_classes: Vec<EgressClass>,
    /// Per-egress standing-rule envelopes this policy relies on. Each egress
    /// class owns a distinct D-107 composition envelope scoped to this exact
    /// (relationship, disclosure_class, egress_class) triple (never the real
    /// rated egress action's slot, and never shared with any other scope's
    /// envelope for the same egress class); a policy may bind several. The
    /// rules remain composition inputs and never replace the task grant.
    #[serde(default)]
    pub standing_rule_bindings: std::collections::BTreeMap<EgressClass, ArtifactId>,
    #[serde(default)]
    pub carve_outs: Vec<DisclosureCarveOut>,
}

/// A kernel-prepared, digest-bound outbound query. Minted before egress so the
/// connector never sees raw private text; dispatch verifies the digest and the
/// action/relationship/egress/grant/provenance binding, and consumes the
/// token once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedQuery {
    pub id: ArtifactId,
    /// The task grant this token was minted for. Consume MUST verify the
    /// requesting grant matches — a token minted under one grant must never
    /// be replayable under another (kernel-issued, never caller-supplied).
    pub grant_id: Ulid,
    pub action_id: ActionId,
    pub relationship: RelationshipKind,
    pub egress_class: EgressClass,
    /// The kernel-derived provenance set the token was minted against.
    /// Consume re-derives provenance from the current request and MUST match
    /// this exactly, so a caller cannot mint against one classified section
    /// set and consume against a different one.
    pub provenance: DisclosureProvenance,
    /// The only text eligible for transport: provenance-generalized.
    pub generalized_query: String,
    /// Digest of `grant|action|relationship|egress|generalized_query` — the
    /// tamper boundary dispatch checks before any connector call.
    pub digest: Digest,
    pub created_at: jiff::Timestamp,
}

impl PreparedQuery {
    pub fn binding_digest(&self) -> Digest {
        digest_of_bytes(
            format!(
                "{}|{}|{:?}|{:?}|{}",
                self.grant_id,
                self.action_id,
                self.relationship,
                self.egress_class,
                self.generalized_query
            )
            .as_bytes(),
        )
    }

    /// Verify every binding a rated-egress consume must hold: action,
    /// relationship, egress class, the requesting grant, and the
    /// kernel-derived provenance set the token was minted against. A
    /// mismatch on any field means this token cannot be consumed for the
    /// current request — dispatch fails closed rather than trusting a
    /// caller-declared binding.
    pub fn binding_matches(
        &self,
        action: &ActionId,
        relationship: RelationshipKind,
        egress_class: EgressClass,
        grant_id: Ulid,
        provenance: &DisclosureProvenance,
    ) -> bool {
        &self.action_id == action
            && self.relationship == relationship
            && self.egress_class == egress_class
            && self.grant_id == grant_id
            && &self.provenance == provenance
    }
}

/// One-use reference handed to dispatch after [`PreparedQuery`] is minted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedQueryRef {
    pub id: ArtifactId,
    pub digest: Digest,
}

impl DisclosurePolicy {
    pub fn covers(
        &self,
        relationship: RelationshipKind,
        class: DisclosureClass,
        egress: EgressClass,
        generalized_query: &str,
    ) -> bool {
        self.lifecycle_state == Lifecycle::Active
            && self.key.relationship == relationship
            && self.key.disclosure_class == class
            && (self.allowed_egress_classes.contains(&egress)
                || self.carve_outs.iter().any(|carve_out| {
                    carve_out.egress_class == egress
                        && carve_out.query_shape == digest_of_bytes(generalized_query.as_bytes())
                }))
    }
}

/// A narrow exception attached to an owner-confirmed policy. The query shape is
/// stored only as a one-way digest; raw private context never enters policy JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisclosureCarveOut {
    pub egress_class: EgressClass,
    pub query_shape: Digest,
}

/// One outbound query. `generalized_query` is the only text eligible for
/// transport; `raw_query` remains local to the caller and is never serialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundQuery {
    pub generalized_query: String,
    pub egress_class: EgressClass,
    pub provenance: DisclosureProvenance,
}

impl OutboundQuery {
    /// Generalize before policy evaluation. The caller supplies deterministic
    /// terms identified while building the query; policy coverage still comes
    /// exclusively from `provenance`.
    pub fn from_private_context(
        raw_query: &str,
        sensitive_terms: &BTreeSet<String>,
        egress_class: EgressClass,
        provenance: DisclosureProvenance,
    ) -> Self {
        Self {
            generalized_query: generalize_query(raw_query, sensitive_terms),
            egress_class,
            provenance,
        }
    }

    pub fn is_effect(&self) -> bool {
        self.provenance.contains_private_context()
    }
}

/// Replace longer sensitive terms first so overlapping terms cannot leak suffixes.
pub fn generalize_query(raw_query: &str, sensitive_terms: &BTreeSet<String>) -> String {
    let mut terms: Vec<&str> = sensitive_terms
        .iter()
        .filter(|term| !term.is_empty())
        .map(String::as_str)
        .collect();
    terms.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
    terms.iter().fold(raw_query.to_string(), |query, term| {
        query.replace(term, "[redacted]")
    })
}

/// Owner-only question produced when no policy covers a provenance class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerQuestionEscalation {
    pub key: DisclosurePolicyKey,
    pub egress_class: EgressClass,
    pub question: String,
}

/// The deterministic disclosure gate result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisclosureGateDecision {
    Allow { query: OutboundQuery },
    Block { escalation: OwnerQuestionEscalation },
}

/// Check every classified provenance class against the relationship-scoped
/// policy set. Generalized text is used only as a digest lookup for a scoped
/// carve-out; sensitivity still comes exclusively from provenance.
pub fn check_egress(
    relationship: RelationshipKind,
    query: OutboundQuery,
    policies: &[DisclosurePolicy],
) -> DisclosureGateDecision {
    for class in query.provenance.classes() {
        if class.requires_policy()
            && !policies.iter().any(|policy| {
                policy.covers(
                    relationship,
                    class,
                    query.egress_class,
                    &query.generalized_query,
                )
            })
        {
            return DisclosureGateDecision::Block {
                escalation: OwnerQuestionEscalation {
                    key: DisclosurePolicyKey {
                        relationship,
                        disclosure_class: class,
                    },
                    egress_class: query.egress_class,
                    question:
                        "Can I share this kind of information with this relationship through this channel?".to_string(),
                },
            };
        }
    }
    DisclosureGateDecision::Allow { query }
}

#[cfg(test)]
#[path = "disclosure_policy_tests.rs"]
mod tests;

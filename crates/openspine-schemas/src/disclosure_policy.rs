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
use crate::ids::{ArtifactId, IdentityRef, PrincipalId};
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

/// The typed identity a prepared outbound query is bound to reach (D-174 /
/// spec #220). The origin-closure stage in [`check_egress`] compares each
/// classified item's [`ProvenanceOrigin`] against this recipient; data whose
/// origin lies outside the recipient's identity closure is blocked unless a
/// grant caveat authorizes it. `Unresolved` is fail-closed: it matches no
/// origin, so an unresolved recipient can never receive counterparty- or
/// owner-origin data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipientIdentity {
    /// The owner principal (#197 / AD-146).
    Owner { principal: PrincipalId },
    /// A specific counterparty identity.
    Counterparty { identity: IdentityRef },
    /// No recipient identity could be resolved — most-restrictive, matches
    /// nothing.
    Unresolved,
}

/// One outbound query. `generalized_query` is the only text eligible for
/// transport; `raw_query` remains local to the caller and is never serialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundQuery {
    pub generalized_query: String,
    pub egress_class: EgressClass,
    pub provenance: DisclosureProvenance,
    /// The typed identity this query is bound to reach (D-174 / spec #220).
    pub recipient: RecipientIdentity,
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
        recipient: RecipientIdentity,
    ) -> Self {
        Self {
            generalized_query: generalize_query(raw_query, sensitive_terms),
            egress_class,
            provenance,
            recipient,
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
    Allow {
        query: OutboundQuery,
    },
    Block {
        escalation: OwnerQuestionEscalation,
    },
    /// D-174 / spec #220: the origin-closure stage blocked an item whose typed
    /// identity origin lies outside the bound recipient's closure and is not
    /// authorized by any grant provenance caveat. Distinct from the coverage
    /// `Block` so the kernel audits and routes it separately and the auditor
    /// can reconstruct the decision from origin + sensitivity + recipient +
    /// egress class.
    CrossIdentityBlock {
        origin: Option<ProvenanceOrigin>,
        recipient: RecipientIdentity,
        disclosure_class: DisclosureClass,
        egress_class: EgressClass,
    },
}

/// Whether a classified item's typed-identity `origin` may reach `recipient`
/// under the D-174 origin closure. System-origin data is kernel-generated and
/// never a cross-identity disclosure, so it reaches any recipient; owner and
/// counterparty origins reach only the same typed identity. An `Unresolved`
/// recipient matches nothing (fail closed).
fn origin_reaches_recipient(origin: &ProvenanceOrigin, recipient: &RecipientIdentity) -> bool {
    match origin {
        ProvenanceOrigin::System {} => true,
        ProvenanceOrigin::Owner { principal } => {
            matches!(recipient, RecipientIdentity::Owner { principal: to } if principal == to)
        }
        ProvenanceOrigin::Counterparty { identity } => {
            matches!(recipient, RecipientIdentity::Counterparty { identity: to } if identity == to)
        }
    }
}

/// Check every classified provenance class against the relationship-scoped
/// policy set. Generalized text is used only as a digest lookup for a scoped
/// carve-out; sensitivity still comes exclusively from provenance.
pub fn check_egress(
    relationship: RelationshipKind,
    query: OutboundQuery,
    policies: &[DisclosurePolicy],
    authorized_origins: &[ProvenanceOrigin],
) -> DisclosureGateDecision {
    // Stage 1 — relationship-scoped disclosure coverage. Sensitivity comes
    // exclusively from immutable provenance, never the generalized text.
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
    // Stage 2 (D-174 / spec #220) — origin-vs-recipient closure. Runs ONLY
    // after coverage passes. An item egresses to the bound recipient only when
    // its typed-identity origin is within the recipient's closure (system, or
    // the same identity) or a grant caveat authorizes the origin. An
    // unresolved origin is most-restrictive and fails closed. Comparison is
    // typed-field only — never query text, never an LLM judgment.
    for item in &query.provenance.items {
        if !item.disclosure_class.requires_policy() {
            continue;
        }
        let reaches = match &item.origin {
            Some(origin) => {
                origin_reaches_recipient(origin, &query.recipient)
                    || authorized_origins.contains(origin)
            }
            None => false,
        };
        if !reaches {
            return DisclosureGateDecision::CrossIdentityBlock {
                origin: item.origin.clone(),
                recipient: query.recipient.clone(),
                disclosure_class: item.disclosure_class,
                egress_class: query.egress_class,
            };
        }
    }
    DisclosureGateDecision::Allow { query }
}

#[cfg(test)]
#[path = "disclosure_policy_tests.rs"]
mod tests;

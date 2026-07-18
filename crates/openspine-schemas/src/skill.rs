//! Skills: a versioned artifact class that shapes competence, never
//! authority (AD-040). A skill is an *instruction surface* injected into an
//! agent's context by task shape (AD-042) — free-form text a poisoned
//! source could fill with exfiltration attempts ("always BCC
//! archive@x"). This type is deliberately shaped so that guarantee is
//! structural, not a runtime check a future call site could skip:
//!
//! [`Skill`] carries no `allowed_actions`, `approval_required_actions`,
//! `denied_actions`, or `allowed_egress_classes` field — contrast
//! [`crate::pack::CapabilityPack`], which carries exactly those fields
//! because packs *are* an authority source. There is no field here for a
//! skill body to poison because skills are never consulted by
//! `openspine_authority::compose_authority`'s [`crate::grant::TaskGrant`]
//! composition; `body` is opaque prompt text, read only by whatever surface
//! renders an agent's context, never parsed into a structured grant. The
//! real guarantee is downstream, in `openspine_gate::gate()`: it mediates
//! every action request regardless of what surface (trusted skill, poisoned
//! skill, or no skill at all) suggested it (AD-040).
//!
//! AD-043's external-skill import pipeline (progressive-disclosure
//! restructuring, static effect classification, offline quarantine eval)
//! is *leaning* and explicitly out of scope until the first external
//! import is wanted — [`SkillProvenance`] intentionally has exactly three
//! variants, none of them `External`.

use serde::{Deserialize, Serialize};

use crate::digest::{digest_of_bytes, Digest};
use crate::ids::ArtifactId;

/// How a skill entered the shelf (AD-041). Determines whether install/update
/// is a silent, already-trusted act or must clear the AD-110 promotion
/// review before it becomes visible to the deterministic index / matcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillProvenance {
    /// Bundled with the product. Installation was the approval (AD-041).
    ShippedSeed,
    /// The owner authored or pasted it in directly. Installation was the
    /// approval (AD-041) — the same tap that puts it on disk is the
    /// human-in-the-loop act.
    UserInstalled,
    /// Distilled by the miner from recurring ad-hoc sequences (AD-044's
    /// crystallization). Provenance is inferred, not human-authored, so it
    /// must clear the AD-110 adversarial promotion review before the shelf
    /// exposes it (AD-041's "one-tap with provenance + diff").
    MinerDistilled,
}

impl SkillProvenance {
    /// AD-110: mandatory for skill install/update only for provenance the
    /// owner did not directly assert. Shipped-seed and user-installed skip
    /// review — the install act itself was the human decision.
    pub fn requires_promotion_review(self) -> bool {
        matches!(self, Self::MinerDistilled)
    }
}

/// Where an installed skill's competence is legible to task-shape matching
/// (AD-042: "workers can't see (or request) skills outside their job").
/// Empty lists mean "visible to nothing" (deny-by-default) — a skill must
/// be explicitly scoped to at least one agent or pack to ever be selected.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SkillVisibility {
    pub agents: Vec<ArtifactId>,
    pub packs: Vec<ArtifactId>,
}

impl SkillVisibility {
    pub fn allows_agent(&self, agent_id: &str) -> bool {
        self.agents.iter().any(|a| a == agent_id)
    }

    /// AD-042: visibility is scoped per agent *or* pack — a skill scoped
    /// only to a pack (not any specific agent) must still be selectable by
    /// every agent composing a grant under that pack.
    pub fn allows_pack(&self, pack_id: &str) -> bool {
        self.packs.iter().any(|p| p == pack_id)
    }

    /// Combined AD-042 visibility check used by the selection matcher.
    pub fn is_visible_to(&self, agent_id: &str, pack_id: &str) -> bool {
        self.allows_agent(agent_id) || self.allows_pack(pack_id)
    }
}

/// A skill's lifecycle. Deliberately distinct from
/// [`crate::artifact::Lifecycle`] (AD-041: the install path is a separate,
/// user-controlled ceremony, never `artifact.propose`'s five-kind pipeline
/// D-048 governs) — reusing that state machine would wire skills into the
/// same transition table proposable kinds use, which is exactly the
/// coupling AD-041 rejects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillState {
    /// A miner-distilled skill awaiting its AD-110 promotion review.
    /// Invisible to the deterministic index and the matcher fallback.
    PendingReview,
    /// On the approved shelf: selectable by task shape (AD-042).
    Installed,
    /// The promotion review denied it. Terminal — a fresh proposal (not a
    /// resubmission) is required to try again.
    Rejected,
    /// Explicitly withdrawn by the owner. Terminal.
    Retired,
}

/// A skill artifact (AD-040): a versioned how-to procedure loaded per task.
/// See the module doc for why this type has no action/scope/egress field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skill {
    pub id: ArtifactId,
    pub schema_version: u32,
    /// Content edits of this skill id (D-028), distinct from any
    /// derivation/generation concept — a skill has no lineage model.
    pub version: u32,
    pub provenance: SkillProvenance,
    pub state: SkillState,
    pub title: String,
    /// Opaque instruction text. Never parsed into a structured shape by
    /// any code path in this crate — see the module doc.
    pub body: String,
    /// AD-042 deterministic-index keys: task classes this skill answers.
    #[serde(default)]
    pub task_shape: Vec<String>,
    #[serde(default)]
    pub visibility: SkillVisibility,
    /// `sha256:<hex>` over `body`'s bytes (D-011: approvals/reviews bind to
    /// exact content, never a mutable reference).
    pub content_digest: Digest,
}

impl Skill {
    /// Compute the content digest a freshly authored/edited skill body
    /// must carry (D-011). Callers that mutate `body` MUST recompute this
    /// before persisting — the store never derives it implicitly, so a
    /// stale digest is a caller bug, not a silently "fixed" one.
    pub fn digest_of_body(body: &str) -> Digest {
        digest_of_bytes(body.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_skill() -> Skill {
        let body = "Summarize the thread, then draft a reply.".to_string();
        Skill {
            id: "draft_reply_skill".to_string(),
            schema_version: 1,
            version: 1,
            provenance: SkillProvenance::ShippedSeed,
            state: SkillState::Installed,
            title: "Draft a reply".to_string(),
            content_digest: Skill::digest_of_body(&body),
            body,
            task_shape: vec!["email_reply".to_string()],
            visibility: SkillVisibility {
                agents: vec!["email_reply_drafter".to_string()],
                packs: vec![],
            },
        }
    }

    #[test]
    fn skill_round_trips_through_json() {
        let skill = sample_skill();
        let json = serde_json::to_string(&skill).unwrap();
        let back: Skill = serde_json::from_str(&json).unwrap();
        assert_eq!(skill, back);
    }

    #[test]
    fn only_mined_provenance_requires_promotion_review() {
        assert!(!SkillProvenance::ShippedSeed.requires_promotion_review());
        assert!(!SkillProvenance::UserInstalled.requires_promotion_review());
        assert!(SkillProvenance::MinerDistilled.requires_promotion_review());
    }

    /// AD-040's containment guarantee starts at the type: `deny_unknown_fields`
    /// means a skill body containing action/scope-shaped JSON can only ever
    /// land in `body: String` (free text), never be parsed into a grant-shaped
    /// field — there is no such field to parse into.
    #[test]
    fn skill_wire_shape_rejects_authority_fields() {
        let mut value = serde_json::to_value(sample_skill()).unwrap();
        let obj = value.as_object_mut().unwrap();
        obj.insert(
            "allowed_actions".to_string(),
            serde_json::json!(["email.send"]),
        );
        let err = serde_json::from_value::<Skill>(value).unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn visibility_denies_by_default() {
        let visibility = SkillVisibility::default();
        assert!(!visibility.allows_agent("any_agent"));
    }
}

//! AD-152 model-swap ceremony: declarative types for a base/matcher/miner
//! model swap proposal and the kernel-owned golden set it is verified
//! against.
//!
//! Two artifact families live here, deliberately asymmetric in who may
//! author them:
//!
//! - [`GoldenSet`] is a **fixture-only** artifact (like
//!   [`crate::agent::AgentManifest`]'s sibling `PromptTemplate` — see
//!   `model_gateway::PromptTemplate`'s doc comment): operator-authored,
//!   loaded at kernel startup, never proposable via chat. If a proposer
//!   could define their own golden-set cases and pass/fail criteria, a
//!   swap proposal could trivially pick five throwaway cases and always
//!   "pass" — the whole point of AD-152's ceremony would be decorative.
//! - [`ModelSwapManifest`] is the proposable artifact (kind `model_swap`,
//!   the sixth entry in `artifact_loader::ARTIFACT_KIND_SPECS`). A
//!   proposer names a *role*, a *target provider* (one of the operator's
//!   already-configured, credentialed providers — see
//!   `pipeline::AppState::provider_pool`), and a *golden set id* to test
//!   against. It MUST NOT supply [`ModelSwapManifest::golden_set_result`]
//!   — that field is populated exclusively by the kernel actually calling
//!   the candidate provider for every case in the referenced golden set
//!   (`model_swap::run_golden_set` in the kernel crate) before the
//!   proposal is ever persisted or gated. See that function's doc comment
//!   for why "proposer supplies input, kernel derives the verdict" is the
//!   trust boundary, not "proposer supplies the verdict".

use serde::{Deserialize, Serialize};

use crate::artifact::Lifecycle;

/// Which governed model role a swap targets (AD-152: base, matcher, and
/// miner). The runtime active-role map persists all three roles; the
/// current kernel has a real inference consumer only for `Base`
/// (`api::generate::post_model_generate`), while matcher/miner consumers
/// arrive with their respective slices. Keeping all three variants now
/// makes every AD-152 swap structurally representable without allowing a
/// silent future bypass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    Base,
    Matcher,
    Miner,
}

impl ModelRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Matcher => "matcher",
            Self::Miner => "miner",
        }
    }
}

/// Whether a golden-set case exercises ordinary expected behavior (checked
/// by the AD-142 gate's offline-replay evaluator) or an adversarial probe
/// (checked by its risk-judge evaluator). AD-142: "offline replay... plus
/// an adversarial risk-judge pass" — this field lets one golden set serve
/// both evaluators without duplicating cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoldenSetCaseKind {
    Standard,
    Adversarial,
}

/// Hard caps on golden-set shape, enforced by [`GoldenSet::validate`] at
/// fixture-load time (fail the kernel boot on an oversized/malformed
/// operator fixture, the same fail-fast posture `deny_unknown_fields`
/// gives every other artifact) and re-checked defensively wherever a
/// golden set is consumed. Without a cap, one fixture could trigger
/// unbounded paid provider calls and an unbounded-size evidence blob per
/// swap proposal.
pub const MAX_GOLDEN_SET_CASES: usize = 20;
pub const MAX_PROMPT_BYTES: usize = 4_000;
pub const MAX_CRITERION_BYTES: usize = 500;
pub const MAX_CRITERIA_PER_CASE: usize = 10;
/// Cap on the excerpt of a candidate provider's ACTUAL observed output kept
/// in [`GoldenSetCaseResult::observed_excerpt`] — bounded so evidence stays
/// small regardless of how verbose a candidate model's response is; the
/// full text's `observed_digest` remains independently auditable.
pub const MAX_OBSERVED_EXCERPT_BYTES: usize = 500;
/// Cap on `GoldenSet.system` — sent ahead of every case's prompt, so its
/// cost multiplies across cases; capped independently of `MAX_PROMPT_BYTES`.
pub const MAX_SYSTEM_BYTES: usize = 4_000;

/// One golden-set case: a kernel-owned input plus deterministic acceptance
/// criteria over the ACTUAL text a candidate provider returns for it. No
/// "expected" LLM-judged verdict — `must_contain`/`must_not_contain` are
/// checked as plain substring tests by `model_swap::run_golden_set_case`,
/// so pass/fail is reproducible and requires no second model call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenSetCase {
    pub id: String,
    pub kind: GoldenSetCaseKind,
    pub prompt: String,
    #[serde(default)]
    pub must_contain: Vec<String>,
    #[serde(default)]
    pub must_not_contain: Vec<String>,
}

/// A fixture-only, operator-authored test corpus a model-swap proposal
/// references by [`GoldenSet::id`]. See the module doc for why this is
/// never proposable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenSet {
    pub id: String,
    pub schema_version: u32,
    /// Roles this immutable corpus is authorized to evaluate.
    pub roles: Vec<ModelRole>,
    /// Optional system-prompt text sent ahead of every case's `prompt`.
    #[serde(default)]
    pub system: Option<String>,
    pub cases: Vec<GoldenSetCase>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GoldenSetValidationError {
    #[error("golden set {id} has {count} cases, exceeding the {MAX_GOLDEN_SET_CASES} cap")]
    TooManyCases { id: String, count: usize },
    #[error("golden set {id} system prompt exceeds {MAX_SYSTEM_BYTES} bytes")]
    SystemTooLong { id: String },
    #[error("golden set {id} has no cases")]
    Empty { id: String },
    #[error("golden set {id} case {case_id} prompt exceeds {MAX_PROMPT_BYTES} bytes")]
    PromptTooLong { id: String, case_id: String },
    #[error("golden set {id} has more than one standard/adversarial coverage failure")]
    CoverageFailed { id: String },
    #[error("golden set {id} case {case_id} has more than {MAX_CRITERIA_PER_CASE} criteria")]
    TooManyCriteria { id: String, case_id: String },
    #[error(
        "golden set {id} case {case_id} has a criterion exceeding {MAX_CRITERION_BYTES} bytes"
    )]
    CriterionTooLong { id: String, case_id: String },
    #[error("golden set {id} has a duplicate case id {case_id}")]
    DuplicateCaseId { id: String, case_id: String },
}

impl GoldenSet {
    /// Enforce the shape caps documented on the `MAX_*` constants. Called
    /// unconditionally when a golden set is loaded from a fixture file
    /// (fail kernel boot on violation) and again, defensively, wherever a
    /// golden set is about to be run against a live provider.
    pub fn validate(&self) -> Result<(), GoldenSetValidationError> {
        if self
            .system
            .as_ref()
            .is_some_and(|system| system.len() > MAX_SYSTEM_BYTES)
        {
            return Err(GoldenSetValidationError::SystemTooLong {
                id: self.id.clone(),
            });
        }
        if self.cases.is_empty() {
            return Err(GoldenSetValidationError::Empty {
                id: self.id.clone(),
            });
        }
        if self.cases.len() > MAX_GOLDEN_SET_CASES {
            return Err(GoldenSetValidationError::TooManyCases {
                id: self.id.clone(),
                count: self.cases.len(),
            });
        }
        let standard_count = self
            .cases
            .iter()
            .filter(|case| matches!(case.kind, GoldenSetCaseKind::Standard))
            .count();
        let adversarial_count = self
            .cases
            .iter()
            .filter(|case| matches!(case.kind, GoldenSetCaseKind::Adversarial))
            .count();
        if self.roles.is_empty() || standard_count < 3 || adversarial_count < 1 {
            return Err(GoldenSetValidationError::CoverageFailed {
                id: self.id.clone(),
            });
        }
        let mut seen = std::collections::HashSet::with_capacity(self.cases.len());
        for case in &self.cases {
            if !seen.insert(case.id.as_str()) {
                return Err(GoldenSetValidationError::DuplicateCaseId {
                    id: self.id.clone(),
                    case_id: case.id.clone(),
                });
            }
            if case.prompt.len() > MAX_PROMPT_BYTES {
                return Err(GoldenSetValidationError::PromptTooLong {
                    id: self.id.clone(),
                    case_id: case.id.clone(),
                });
            }
            let criteria = case.must_contain.iter().chain(case.must_not_contain.iter());
            if case.must_contain.len() + case.must_not_contain.len() > MAX_CRITERIA_PER_CASE {
                return Err(GoldenSetValidationError::TooManyCriteria {
                    id: self.id.clone(),
                    case_id: case.id.clone(),
                });
            }
            for criterion in criteria {
                if criterion.len() > MAX_CRITERION_BYTES {
                    return Err(GoldenSetValidationError::CriterionTooLong {
                        id: self.id.clone(),
                        case_id: case.id.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

/// The kernel's genuinely-observed result of one case, computed exclusively
/// by `model_swap::run_golden_set_case` — never proposer-supplied. See the
/// module doc for the trust-boundary rationale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenSetCaseResult {
    pub case_id: String,
    pub kind: GoldenSetCaseKind,
    pub passed: bool,
    /// First `MAX_OBSERVED_EXCERPT_BYTES` (UTF-8-boundary-safe) of the
    /// candidate provider's actual response text — enough for the owner's
    /// approval summary to be "informed, not decorative" (AD-142) without
    /// unbounded evidence growth.
    pub observed_excerpt: String,
    /// `sha256:<hex>` digest of the FULL observed output (see
    /// `crate::digest::digest_of_bytes`), so a truncated excerpt's
    /// provenance stays auditable even though the full text isn't kept.
    pub observed_digest: String,
}

/// The kernel's record of having actually run every case in a referenced
/// [`GoldenSet`] against a candidate provider. Both the golden-set digest
/// and provider configuration digest bind this result to the exact inputs
/// and non-secret candidate identity that were evaluated; both are
/// re-checked at activation time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenSetRunResult {
    pub golden_set_id: String,
    pub golden_set_digest: String,
    pub provider_config_digest: String,
    pub cases: Vec<GoldenSetCaseResult>,
}

/// A model-swap proposal (kind `model_swap`, AD-152). See the module doc
/// for the propose-time enrichment contract on `golden_set_result`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSwapManifest {
    /// The governed role's name (`"base"` today) — one swap manifest per
    /// role, so this doubles as the artifact id `artifact_loader`'s
    /// registry keys artifacts by.
    pub id: String,
    #[serde(default = "crate::artifact::default_version")]
    pub version: u32,
    pub lifecycle_state: Lifecycle,
    pub role: ModelRole,
    /// Must name an entry in `pipeline::AppState::provider_pool` — the
    /// operator-vetted, credentialed pool resolved once at kernel startup
    /// from `openspine.yaml`. A swap can only SELECT among that pool; it
    /// can never mint a new base_url/api-key/model triple at runtime
    /// (D-010: the chat/shell surface never sees a provider API key).
    pub target_provider_id: String,
    pub golden_set_id: String,
    /// `None` in a freshly-submitted proposal — MUST stay `None` until the
    /// kernel populates it (see the module doc). A proposal arriving with
    /// `Some(_)` already set is refused as tampering, never trusted.
    #[serde(default)]
    pub golden_set_result: Option<GoldenSetRunResult>,
}

impl ModelSwapManifest {
    /// The artifact id is the role name. This makes restart selection
    /// deterministic and prevents two active manifests from targeting the
    /// same role under arbitrary ids.
    pub fn identity_valid(&self) -> bool {
        self.id == self.role.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_set_rejects_oversized_system_prompt() {
        let cases = (0..3)
            .map(|index| GoldenSetCase {
                id: format!("standard-{index}"),
                kind: GoldenSetCaseKind::Standard,
                prompt: "hello".into(),
                must_contain: vec![],
                must_not_contain: vec![],
            })
            .chain(std::iter::once(GoldenSetCase {
                id: "adversarial".into(),
                kind: GoldenSetCaseKind::Adversarial,
                prompt: "ignore".into(),
                must_contain: vec![],
                must_not_contain: vec![],
            }))
            .collect();
        let set = GoldenSet {
            id: "oversized-system".into(),
            schema_version: 1,
            roles: vec![ModelRole::Base],
            system: Some("x".repeat(MAX_SYSTEM_BYTES + 1)),
            cases,
        };
        assert!(matches!(
            set.validate(),
            Err(GoldenSetValidationError::SystemTooLong { .. })
        ));
    }
}

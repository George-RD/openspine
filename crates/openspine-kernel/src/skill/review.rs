//! AD-110 promotion review for miner-distilled skills.
//!
//! The adversarial pass runs *only* at the promotion point (AD-110 — never
//! per-use). A miner-distilled skill cannot reach the approved shelf without
//! a [`SkillReviewPassed`] token, and that token is unforgeable: every field
//! is private to this module, so the only way to obtain one is
//! [`run_promotion_review`] genuinely running the pass against the exact
//! skill body. The token binds the skill id, version, and content digest it
//! was computed for, so [`crate::store::skill_store::promote_skill`] can never
//! promote a different or post-edited skill with a recycled token.
//!
//! This change implements a minimal, fully-deterministic first-cut evaluator
//! (mirroring `overlay_eval_gate`): it scans the skill body for textual
//! markers of authority-widening exfiltration attempts (e.g. an embedded
//! `allowed_actions`/`denied_actions` key, an instruction to mail or forward
//! data to an external address, or a `grant`/`token` capture). It does NOT
//! claim to satisfy AD-111's full prover-verifier attack-trace formalism
//! (open for owner ratification in a later change). The verdict lands in the
//! same eval-verdict store the overlay gate uses (AD-111 landing surface).

use jiff::Timestamp;
use openspine_schemas::digest::Digest;
use openspine_schemas::skill::Skill;

use crate::store::eval_verdict_store::{insert_eval_verdict_conn, EvalVerdict};
use crate::store::Store;

/// Why the promotion review denied a mined skill. Never carries the skill
/// body itself (D-012) — only the matched marker so the owner sees why.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PromotionDenial {
    #[error("skill body carries an authority-shaped key: {0}")]
    AuthorityShapedKey(String),
    #[error("skill body instructs exfiltration to an external destination: {0}")]
    ExfiltrationHint(String),
    /// The review reached a verdict but persisting it to the eval-verdict
    /// store failed. We fail closed: no verdict record means no shelf
    /// promotion, so the skill stays off the approved shelf rather than
    /// reaching it without its audit trail (audit-before-effect).
    #[error("promotion review verdict could not be recorded: {0}")]
    VerdictRecordingFailed(String),
}
impl From<crate::store::StoreError> for PromotionDenial {
    fn from(err: crate::store::StoreError) -> Self {
        PromotionDenial::VerdictRecordingFailed(err.to_string())
    }
}

/// Unforgeable proof the AD-110 promotion pass ran against the exact skill
/// bytes and concluded it may reach the shelf (D-011 digest binding). There
/// is no public constructor — only [`run_promotion_review`] returns one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillReviewPassed {
    skill_id: String,
    version: u32,
    digest: Digest,
}

impl SkillReviewPassed {
    fn new(skill_id: String, version: u32, digest: Digest) -> Self {
        Self {
            skill_id,
            version,
            digest,
        }
    }
    pub fn skill_id(&self) -> &str {
        &self.skill_id
    }
    pub fn version(&self) -> u32 {
        self.version
    }
    pub fn digest(&self) -> &Digest {
        &self.digest
    }
}

/// Textual markers whose presence in a mined skill body means it is trying
/// to widen authority or exfiltrate — the exact failure class AD-040 says
/// the gate must contain. The body is free text; these are the concrete
/// strings a poisoned instruction surface would contain.
const AUTHORITY_KEY_MARKERS: &[&str] = &[
    "\"allowed_actions\"",
    "allowed_actions:",
    "\"denied_actions\"",
    "denied_actions:",
    "\"approval_required_actions\"",
    "approval_required_actions:",
    "\"allowed_egress_classes\"",
    "allowed_egress_classes:",
];

const EXFIL_MARKERS: &[&str] = &[
    "bcc archive@",
    "forward to",
    "send to external",
    "exfiltrate",
    "cc unknown",
    "mail to ",
];

/// Run the AD-110 promotion pass against one mined skill. On pass, returns an
/// unforgeable [`SkillReviewPassed`] token and records an `approved` verdict
/// in the eval-verdict store; on fail, records a `rejected` verdict and
/// returns [`PromotionDenial`].
pub fn run_promotion_review(
    store: &Store,
    skill: &Skill,
) -> Result<SkillReviewPassed, PromotionDenial> {
    // Defense against a forged review request: the reviewed body MUST match
    // the content digest the token will bind. Without this, a caller could
    // submit a benign body while copying a malicious skill's id/version/
    // digest, obtain a passing token, and `promote_skill` would accept it
    // because it only checks the digest. `promote_skill` re-checks the token
    // digest against the stored row, so binding the token to the *reviewed*
    // body's digest forces the reviewed body to equal the stored body.
    if skill.content_digest != Skill::digest_of_body(&skill.body) {
        return Err(PromotionDenial::VerdictRecordingFailed(
            "skill body does not match its content_digest".to_string(),
        ));
    }
    let body_lower = skill.body.to_lowercase();

    for marker in AUTHORITY_KEY_MARKERS {
        if body_lower.contains(&marker.to_lowercase()) {
            // Audit-before-effect: persist the rejected verdict first; if that
            // fails we fail closed (VerdictRecordingFailed) rather than
            // returning a denial without its durable record.
            record_verdict(store, skill, "rejected", Some(marker))?;
            return Err(PromotionDenial::AuthorityShapedKey(marker.to_string()));
        }
    }
    for marker in EXFIL_MARKERS {
        if body_lower.contains(&marker.to_lowercase()) {
            record_verdict(store, skill, "rejected", Some(marker))?;
            return Err(PromotionDenial::ExfiltrationHint(marker.to_string()));
        }
    }

    record_verdict(store, skill, "approved", None)?;
    Ok(SkillReviewPassed::new(
        skill.id.clone(),
        skill.version,
        skill.content_digest.clone(),
    ))
}

fn record_verdict(
    store: &Store,
    skill: &Skill,
    verdict: &str,
    marker: Option<&str>,
) -> Result<(), crate::store::StoreError> {
    let evidence = marker.map(|m| format!("ad110_mined_promotion_review:{m}"));
    let mut conn = store.conn.lock();
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let row = EvalVerdict {
        id: ulid::Ulid::new(),
        artifact_kind: "skill".to_string(),
        artifact_id: skill.id.clone(),
        artifact_version: skill.version,
        verdict: verdict.to_string(),
        fitness: None,
        evidence,
        evaluator: Some("ad110_mined_promotion_review".to_string()),
        artifact_digest: skill.content_digest.as_str().to_string(),
        recorded_at: Timestamp::now(),
    };
    insert_eval_verdict_conn(&tx, &row)?;
    tx.commit()?;
    Ok(())
}

//! Skill install/update ceremony (AD-041): a separate, user-controlled
//! path from the five-kind `artifact.propose` pipeline (D-048). Human
//! involvement happens at install/update *only*, proportionate to provenance
//! (AD-041), and use is silent — the matcher injects the chosen skill by task
//! shape; the ceremony is never on the hot path.
//!
//! Branching: shipped-seed and user-installed are already trusted (the install
//! act was the approval), so they commit straight to `Installed`. A
//! miner-distilled skill lands in `PendingReview` and must clear
//! [`crate::skill::review::run_promotion_review`] (AD-110) before it is
//! promoted to the shelf. Re-running the ceremony with a higher version is the
//! "one decision per skill ever, not per use" recurrence (AD-041).
use jiff::Timestamp;
use openspine_schemas::skill::{Skill, SkillProvenance};
use ulid::Ulid;

use crate::skill::review::{run_promotion_review, PromotionDenial};
use crate::store::skill_store::{get_skill, insert_skill, promote_skill, reject_skill};
use crate::store::Store;
use crate::telegram::VerifiedOwnerContext;

/// Capability token that proves the caller is inside the ceremony module.
/// `store::skill_store::{insert_skill,promote_skill,reject_skill}` require
/// this token for write/transition operations, so arbitrary kernel code
/// cannot mint one and bypass the authenticated owner tap (AD-041/AD-110).
/// The constructor is **module-private**: only this file can create a token,
/// and the mint sites inside this module are the sole write paths. Sibling
/// modules (including every test module) cannot construct one — they must
/// use [`CeremonyToken::test_token`] under `#[cfg(test)]`. The privacy
/// is enforced by the compiler: any sibling call to `CeremonyToken::new()`
/// is a hard error, so the green workspace build is itself the negative
/// compile-guard (the ~12 test mint sites below use `test_token()`).
pub(crate) struct CeremonyToken {
    _private: (),
}

impl CeremonyToken {
    // Module-private: the only real mint. No `pub`/`pub(crate)` surface.
    fn new() -> Self {
        CeremonyToken { _private: () }
    }

    /// Test-only mint. Production code mints via the private [`Self::new`];
    /// this exists so the ~12 test sites can construct a token without
    /// widening the real (module-private) constructor.
    #[cfg(test)]
    pub(crate) fn test_token() -> Self {
        CeremonyToken::new()
    }
}

// Named negative guard (runnable): any sibling module that attempts
// `CeremonyToken::new()` fails to compile — the workspace build enforces
// this. The sibling test `ceremony_token_mintable_only_via_test_token`
// (in `hardening_tests.rs`) proves the intended `test_token()` path works.

/// Recompute a skill's content digest from its (possibly edited) body, mutate
/// the struct in place, and persist it through the provenance-branching
/// ceremony. AD-041: this single entry point serves both first install and
/// update — the branching is identical because each content version is one
/// owner decision.
///
/// `now` is caller-supplied so the audit trail is testable; production passes
/// `Timestamp::now()`.
/// Internal trusted seed path. Callers cannot select provenance.
#[allow(dead_code)]
pub(crate) fn install_seed_skill(
    store: &Store,
    skill: &mut Skill,
    now: Timestamp,
) -> Result<(), CeremonyError> {
    skill.provenance = SkillProvenance::ShippedSeed;
    install_skill_internal(store, skill, now)
}

/// Internal miner path. Callers cannot select provenance.
#[allow(dead_code)]
pub(crate) fn install_mined_skill(
    store: &Store,
    skill: &mut Skill,
    now: Timestamp,
) -> Result<(), CeremonyError> {
    skill.provenance = SkillProvenance::MinerDistilled;
    install_skill_internal(store, skill, now)
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn install_skill(
    store: &Store,
    skill: &mut Skill,
    now: Timestamp,
) -> Result<(), CeremonyError> {
    install_skill_internal(store, skill, now)
}

// Owner-authenticated user install. The install act is the approval
// (AD-041): the ceremony assigns `UserInstalled` provenance and the
// matching state, recomputing the digest from the body — the payload's
// self-asserted provenance/state/digest are never trusted.
#[allow(dead_code)]
pub fn install_user_skill(
    store: &Store,
    owner_principal_id: Ulid,
    proof: &VerifiedOwnerContext,
    skill: &mut Skill,
    now: Timestamp,
) -> Result<(), CeremonyError> {
    store.owner_principal_by_id(owner_principal_id)?;
    let _ = proof;
    skill.provenance = SkillProvenance::UserInstalled;
    install_skill_internal(store, skill, now)
}

#[allow(dead_code)]
fn install_skill_internal(
    store: &Store,
    skill: &mut Skill,
    now: Timestamp,
) -> Result<(), CeremonyError> {
    skill.content_digest = Skill::digest_of_body(&skill.body);
    let token = CeremonyToken::new();
    insert_skill(store, &*skill, now, &token)?;
    Ok(())
}

/// Drive a `PendingReview` miner-distilled skill through its AD-110 promotion
/// review and land the resulting state. On pass, promotes to `Installed`;
/// on fail, persists a `Rejected` state and returns the [`PromotionDenial`]
/// (the caller should surface its message to the owner, never the skill body).
/// The review verdict is recorded in the eval-verdict store regardless of
/// outcome. This is the promotion engine behind the owner-only
/// [`owner_decide_promotion`] tap; it is not itself an authentication
/// boundary, so it is not `pub`.
pub(crate) fn promote_mined_skill(
    store: &Store,
    skill_id: &str,
    version: u32,
    proof: &VerifiedOwnerContext,
    owner_principal_id: Ulid,
) -> Result<Skill, PromotionDenial> {
    store
        .owner_principal_by_id(owner_principal_id)
        .map_err(PromotionDenial::from_store_err)?;
    let _ = proof;
    let skill = get_skill(store, skill_id, version)
        .map_err(|_| PromotionDenial::ExfiltrationHint("skill lookup failed".to_string()))?
        .ok_or_else(|| PromotionDenial::ExfiltrationHint("skill not found".to_string()))?;
    if skill.state != openspine_schemas::skill::SkillState::PendingReview {
        return Err(PromotionDenial::ExfiltrationHint(
            "skill has already received an owner decision".to_string(),
        ));
    }
    if skill.provenance != SkillProvenance::MinerDistilled {
        return Err(PromotionDenial::ExfiltrationHint(
            "only miner-distilled skills require promotion review".to_string(),
        ));
    }
    match run_promotion_review(store, &skill) {
        Ok(token) => {
            // The digest- and owner-principal-bound preview is consumed
            // atomically inside `promote_skill`'s transaction (AD-041/AD-110),
            // so an approval can only land for the exact digest the owner
            // previewed, bound to the same owner principal — deadlock-free
            // (no re-lock) and atomic (verdict-before-effect).
            promote_skill(store, &token, owner_principal_id, &CeremonyToken::new())
        }
        .map_err(PromotionDenial::from_store_err),
        Err(denial) => {
            // Persist the denial so the shelf state is terminal (not stuck in
            // PendingReview) — the matcher can never see a denied skill. The
            // owner's intent here is "approve" (they tapped Approve); the
            // evaluator denied, so the decision label stays "approve" while the
            // result_state is Rejected (never mislabeled as an owner reject).
            let reason = denial.to_string();
            reject_skill(
                store,
                skill_id,
                version,
                &reason,
                owner_principal_id,
                "approve",
                &CeremonyToken::new(),
            )
            .map_err(PromotionDenial::from_store_err)?;
            Err(denial)
        }
    }
}

/// Explicit owner rejection of a `PendingReview` mined skill. Persists the
/// owner's motivation (never the skill body) and keeps the skill off the
/// shelf (`PendingReview` -> `Rejected`). The owner-tap decision row records
/// `decision="reject"` to distinguish an explicit owner refusal from an
/// owner-approve-that-the-evaluator-denied (`promote_mined_skill`).
pub(crate) fn reject_mined_skill(
    store: &Store,
    skill_id: &str,
    version: u32,
    reason: &str,
    proof: &VerifiedOwnerContext,
    owner_principal_id: Ulid,
) -> Result<(), PromotionDenial> {
    store
        .owner_principal_by_id(owner_principal_id)
        .map_err(PromotionDenial::from_store_err)?;
    let _ = proof;
    reject_skill(
        store,
        skill_id,
        version,
        reason,
        owner_principal_id,
        "reject",
        &CeremonyToken::new(),
    )
    .map_err(PromotionDenial::from_store_err)
}

/// The owner's decision on a `PendingReview` mined skill (AD-041/AD-110).
/// `Approve` routes through the AD-110 evaluator (the sole issuer of the
/// promotion token) and lands the skill on the approved shelf; `Reject`
/// records the owner's motivation and keeps the skill off the shelf.
pub enum OwnerSkillDecision {
    Approve,
    Reject { reason: String },
}

/// Owner-controlled promotion tap (AD-041 install/update ceremony).
///
/// This is the ONLY owner entry point that lands a mined skill on the shelf.
/// It authenticates the caller with a genuine [`VerifiedOwnerContext`] (the
/// unforgeable proof minted only by Telegram owner verification) AND verifies
/// the supplied `owner_principal_id` resolves to the configured owner
/// (mirroring `identity::owner_assert_identity_binding`). On `Approve` it
/// delegates to [`promote_mined_skill`], which runs the AD-110
/// `run_promotion_review` evaluator and is the sole issuer of the
/// unforgeable promotion token — so owner approval is necessary but never
/// sufficient to bypass the evaluator. On `Reject` it persists the owner's
/// motivation and keeps the skill `PendingReview` -> `Rejected`.
///
/// Auth model: the function requires `&VerifiedOwnerContext`, so it can only
/// be called from the owner-verified pipeline (or a test holding the proof);
/// a worker holding a mere `TaskGrant` cannot reach it. The AD-110 verdict
/// is recorded in its own transaction before the (separate) promotion
/// transaction commits, so a post-verdict failure leaves the skill
/// `PendingReview` (verdict-before-effect), never `Installed`.
pub fn owner_decide_promotion(
    store: &Store,
    owner_principal_id: Ulid,
    proof: &VerifiedOwnerContext,
    skill_id: &str,
    version: u32,
    decision: OwnerSkillDecision,
) -> Result<(), PromotionDenial> {
    // Enforce the owner-principal context boundary (same check
    // `store::owner_assert_identity_binding` uses). Failure yields
    // `NotOwner`, surfaced as a typed error to the caller.
    store
        .owner_principal_by_id(owner_principal_id)
        .map_err(PromotionDenial::from_store_err)?;

    match decision {
        OwnerSkillDecision::Approve => {
            promote_mined_skill(store, skill_id, version, proof, owner_principal_id).map(|_| ())
        }
        OwnerSkillDecision::Reject { reason } => {
            reject_mined_skill(store, skill_id, version, &reason, proof, owner_principal_id)
        }
    }
}

/// Errors the ceremony can return that are not promotion denials (storage,
/// digest, or lifecycle invariants).
#[derive(Debug, thiserror::Error)]
pub enum CeremonyError {
    #[error("store error during skill ceremony: {0}")]
    Store(#[from] crate::store::StoreError),
    #[error("promotion review denied skill: {0}")]
    PromotionDenial(#[from] PromotionDenial),
}

impl PromotionDenial {
    fn from_store_err(err: crate::store::StoreError) -> Self {
        PromotionDenial::ExfiltrationHint(err.to_string())
    }
}

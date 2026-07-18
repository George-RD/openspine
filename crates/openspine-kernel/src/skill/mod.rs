//! Skills: a versioned artifact class that shapes competence, never
//! authority (AD-040). See `crate::openspine-schemas` `skill` module for the
//! type-level containment guarantee, and `store::skill_store` for the table.
//!
//! This crate-local module is the ceremony + selection layer:
//! - [`ceremony`] — install/update (AD-041) branching by provenance and the
//!   AD-110 promotion path for mined skills.
//! - [`review`] — the AD-110 promotion pass + the unforgeable
//!   [`review::SkillReviewPassed`] token.
//! - [`selection`] — the AD-042 read-only matcher (inject, never install).
//!
//! AD-043's external-skill import pipeline (progressive-disclosure
//! restructuring, static effect/egress classification, offline quarantine
//! eval) is *leaning* and deliberately NOT implemented here — `SkillProvenance`
//! carries exactly three variants. Adding it later is a new change, not a
//! hidden branch in this one.

pub mod ceremony;
pub mod review;
pub mod selection;
// Re-export the AD-042 matcher so the HTTP action handler can reach it
// through the crate-visible `crate::skill` path.
pub use selection::select_skills_for_task;

#[cfg(test)]
mod containment_tests;
#[cfg(test)]
mod hardening_tests;
#[cfg(test)]
mod promotion_tap_tests;
#[cfg(test)]
mod tests;

//! Canonical plan-step payload and one-loop approval carrier (AD-011).
//!
//! Kernel approval-effect handlers MUST re-derive `Plan::digest()` from
//! trusted artifact-store bytes at approval-effect time and MUST NOT trust
//! a caller/carrier-supplied digest directly (D-055.4, mirroring
//! `crates/openspine-kernel/src/pipeline/approval.rs`'s existing draft
//! re-derivation). This module intentionally has no "approve" constructor:
//! `ApprovalRecord` construction is a kernel responsibility.
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

use crate::action::ActionId;
use crate::digest::{self, Digest};

/// One effectful step, including data-handling steps. Arguments bind exact
/// execution identity; summary is the additive owner-facing description.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanStep {
    pub action: ActionId,
    pub arguments: serde_json::Value,
    #[serde(default)]
    pub summary: String,
}

/// Ordered full list of effectful plan steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema_version: u32,
    pub steps: Vec<PlanStep>,
}

impl Plan {
    /// Digest the complete serialized plan object (including
    /// `schema_version`) through D-028 canonical JSON. Hashing the object
    /// (rather than only its array field) keeps this value identical to
    /// the artifact-store digest of the serialized plan payload.
    pub fn digest(&self) -> Digest {
        let value = serde_json::to_value(self).expect("Plan always serializes to JSON");
        digest::digest_of(&value)
    }
}

/// Clarifying question carrying the exact plan digest to be approved.
///
/// `question` is rendered deterministically from `plan` — every
/// digest-bound field of every step (action, canonical arguments, and
/// summary), not summary text alone — so the shown text and the hashed
/// object can never diverge (WYSIWYS, D-045 parity). Callers MUST refuse
/// to dispatch this question through a channel that cannot display it in
/// full (e.g. Telegram length truncation), exactly as the existing draft
/// preview path refuses rather than truncating.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanApprovalQuestion {
    pub schema_version: u32,
    pub question: String,
    pub plan_digest: Digest,
    pub target_digest: Digest,
}

impl PlanApprovalQuestion {
    /// Render `intro` followed by every step's full digest-bound identity:
    /// action id, canonical arguments, and summary. `plan_digest` is
    /// computed from `plan`, never supplied separately, so the carrier
    /// cannot be constructed with a digest that doesn't match its own
    /// rendered text.
    pub fn new(intro: impl Into<String>, plan: &Plan, target_digest: Digest) -> Self {
        let mut question = intro.into();
        let _ = write!(question, "\nPlan schema_version: {}", plan.schema_version);
        for (i, step) in plan.steps.iter().enumerate() {
            let _ = write!(
                question,
                "\n{}. [{}] {} — arguments: {}",
                i + 1,
                step.action.as_str(),
                step.summary,
                step.arguments
            );
        }
        Self {
            schema_version: 1,
            question,
            plan_digest: plan.digest(),
            target_digest,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::Digest;

    fn step(action: &str, args: serde_json::Value, summary: &str) -> PlanStep {
        PlanStep {
            action: ActionId::new(action),
            arguments: args,
            summary: summary.into(),
        }
    }
    fn sample() -> Plan {
        Plan {
            schema_version: 1,
            steps: vec![
                step(
                    "calendar.book",
                    serde_json::json!({"time":"14:00"}),
                    "Book a slot",
                ),
                step(
                    "reminder.set",
                    serde_json::json!({"at":"13:45"}),
                    "Set reminder",
                ),
            ],
        }
    }

    #[test]
    fn digest_is_order_argument_and_schema_sensitive() {
        let original = sample();
        let reordered = Plan {
            schema_version: 1,
            steps: original.steps.iter().cloned().rev().collect(),
        };
        let changed = Plan {
            schema_version: 1,
            steps: vec![
                step(
                    "calendar.book",
                    serde_json::json!({"time":"15:00"}),
                    "Book a slot",
                ),
                original.steps[1].clone(),
            ],
        };
        let versioned = Plan {
            schema_version: 2,
            steps: original.steps.clone(),
        };
        assert_ne!(original.digest(), reordered.digest());
        assert_ne!(original.digest(), changed.digest());
        assert_ne!(original.digest(), versioned.digest());
    }

    #[test]
    fn digest_includes_data_handling_and_canonicalizes_keys() {
        let a = Plan {
            schema_version: 1,
            steps: vec![step("x", serde_json::json!({"a":1,"b":2}), "x")],
        };
        let b = Plan {
            schema_version: 1,
            steps: vec![step("x", serde_json::json!({"b":2,"a":1}), "x")],
        };
        let with_scrub = Plan {
            schema_version: 1,
            steps: vec![
                step(
                    "data.scrub",
                    serde_json::json!({"fields":["ssn"]}),
                    "Scrub before searching",
                ),
                a.steps[0].clone(),
            ],
        };
        assert_eq!(a.digest(), b.digest());
        assert_ne!(a.digest(), with_scrub.digest());
    }

    #[test]
    fn digest_matches_serialized_plan_payload() {
        let plan = sample();
        let serialized = serde_json::to_value(&plan).unwrap();
        assert_eq!(plan.digest(), crate::digest::digest_of(&serialized));
    }

    #[test]
    fn question_renders_every_digest_bound_field() {
        let plan = sample();
        let target = Digest::parse(format!("sha256:{}", "a".repeat(64))).unwrap();
        let question = PlanApprovalQuestion::new("Does this plan work?", &plan, target);
        assert_eq!(question.schema_version, 1);
        assert!(question.question.contains("schema_version: 1"));
        assert_eq!(question.plan_digest, plan.digest());
        assert!(question.question.contains("calendar.book"));
        assert!(question.question.contains("14:00"));
        assert!(question.question.contains("Book a slot"));
        assert!(question.question.contains("reminder.set"));
        assert!(question.question.contains("13:45"));
    }

    #[test]
    fn serde_rejects_unknown_fields_and_round_trips() {
        let plan = sample();
        let back: Plan = serde_json::from_str(&serde_json::to_string(&plan).unwrap()).unwrap();
        assert_eq!(plan, back);
        assert!(serde_json::from_str::<PlanStep>(
            r#"{"action":"x","arguments":null,"summary":"s","extra":true}"#
        )
        .is_err());
        let question = PlanApprovalQuestion::new(
            "q",
            &plan,
            Digest::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
        );
        let question_back: PlanApprovalQuestion =
            serde_json::from_str(&serde_json::to_string(&question).unwrap()).unwrap();
        assert_eq!(question, question_back);
    }
}

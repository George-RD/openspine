//! AD-011 integration: plan payload digests use the existing gate path.
use std::collections::HashMap;

use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::plan::{Plan, PlanStep};
use serde_json::json;

use super::tests::{approval_for, grant_with, request_for, MockContext};
use super::token_tests::test_catalog;
use super::*;

fn step(action: &str, args: serde_json::Value, summary: &str) -> PlanStep {
    PlanStep {
        action: ActionId::new(action),
        arguments: args,
        summary: summary.into(),
    }
}

#[test]
fn approved_plan_digest_allows_through_gate() {
    let grant = grant_with(&[], &["email.create_draft"], &[]);
    let plan = Plan {
        schema_version: 1,
        steps: vec![
            step("calendar.book", json!({"time":"14:00"}), "Book"),
            step("reminder.set", json!({"at":"13:45"}), "Remind"),
        ],
    };
    let mut req = request_for("email.create_draft");
    req.payload_ref = Some(ArtifactRef {
        digest: plan.digest(),
        schema_version: 1,
    });
    let approval = approval_for(&req, ApprovalDecision::Approved, 900);
    let ctx = MockContext {
        approvals: HashMap::from([(req.id, approval)]),
    };
    assert_eq!(
        gate(
            &grant,
            &req,
            ActionOrigin::Shell,
            &ctx,
            &test_catalog(),
            &NoEgress,
            Timestamp::now()
        )
        .decision,
        GateDecision::Allow
    );
}

#[test]
fn mutated_plan_after_approval_is_denied_at_gate() {
    let grant = grant_with(&[], &["email.create_draft"], &[]);
    let original = Plan {
        schema_version: 1,
        steps: vec![
            step("calendar.book", json!({"time":"14:00"}), "Book"),
            step("reminder.set", json!({"at":"13:45"}), "Remind"),
        ],
    };
    let mut req = request_for("email.create_draft");
    req.payload_ref = Some(ArtifactRef {
        digest: original.digest(),
        schema_version: 1,
    });
    let approval = approval_for(&req, ApprovalDecision::Approved, 900);
    let ctx = MockContext {
        approvals: HashMap::from([(req.id, approval)]),
    };
    let mutated = Plan {
        schema_version: 1,
        steps: vec![
            original.steps[0].clone(),
            step(
                "data.scrub",
                json!({"fields":["ssn"]}),
                "Scrub before searching",
            ),
            original.steps[1].clone(),
        ],
    };
    req.payload_ref = Some(ArtifactRef {
        digest: mutated.digest(),
        schema_version: 1,
    });
    assert_eq!(
        gate(
            &grant,
            &req,
            ActionOrigin::Shell,
            &ctx,
            &test_catalog(),
            &NoEgress,
            Timestamp::now()
        )
        .decision,
        GateDecision::Deny {
            reason: DenialReason::ApprovalDigestMismatch
        }
    );
}

#[test]
fn argument_mutation_denied_when_summary_is_unchanged() {
    let grant = grant_with(&[], &["email.create_draft"], &[]);
    let original = Plan {
        schema_version: 1,
        steps: vec![step(
            "calendar.book",
            json!({"time":"14:00"}),
            "Book a slot",
        )],
    };
    let mut req = request_for("email.create_draft");
    req.payload_ref = Some(ArtifactRef {
        digest: original.digest(),
        schema_version: 1,
    });
    let approval = approval_for(&req, ApprovalDecision::Approved, 900);
    let ctx = MockContext {
        approvals: HashMap::from([(req.id, approval)]),
    };
    let mutated = Plan {
        schema_version: 1,
        steps: vec![step(
            "calendar.book",
            json!({"time":"15:00"}),
            "Book a slot",
        )],
    };
    req.payload_ref = Some(ArtifactRef {
        digest: mutated.digest(),
        schema_version: 1,
    });
    assert_eq!(
        gate(
            &grant,
            &req,
            ActionOrigin::Shell,
            &ctx,
            &test_catalog(),
            &NoEgress,
            Timestamp::now()
        )
        .decision,
        GateDecision::Deny {
            reason: DenialReason::ApprovalDigestMismatch
        }
    );
}

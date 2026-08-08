//! Activation-guard and drift-trigger tests for the reviewed-scope binding
//! (#128), split from `standing_rules_scoped_tests.rs` to keep both files
//! under the 500-line gate. These cover the two activation entry points and
//! the AD-010 drift trigger's treatment of retained (`reserved`) usage.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::standing_rule::BudgetWindow;
use ulid::Ulid;

use super::learned_artifacts::{
    CompatibilityStatus, LearnedArtifact, NominationStatus, Provenance,
};
use super::standing_rules_tests::manifest;
use super::Store;
use openspine_schemas::artifact::{ArtifactNamespace, ArtifactRef};
use openspine_schemas::digest::digest_of_bytes;

/// A minimal learned artifact for the activation commit. Its content is
/// irrelevant here: the reviewed-scope guard refuses before the activation
/// transaction opens, so nothing about the artifact is ever written.
fn learned_artifact() -> LearnedArtifact {
    LearnedArtifact {
        kind: "route".into(),
        artifact_id: "learned-route".into(),
        version: 1,
        namespace: ArtifactNamespace::Overlay,
        provenance: Provenance::ProducedBy {
            source_event_id: Ulid::new(),
            source_exchange: ArtifactRef {
                digest: digest_of_bytes(b"exchange"),
                schema_version: 1,
            },
            source_scope: Ulid::new(),
        },
        accepted_via: None,
        learned_at: Timestamp::now(),
        compatibility: CompatibilityStatus::Compatible,
        nomination: NominationStatus::None,
        pending_reconfirmation_id: None,
        pending_yaml_digest: None,
        accepted_dependency_fingerprint: None,
        source_path: None,
        accepted_base_epoch: None,
    }
}

/// #128 MINOR A regression guard: the AD-010 drift trigger must still fire for
/// a rule that saturates through *retained* (delivery-unknown) reservations.
/// Those rows stay `reserved` rather than becoming `committed`, so counting
/// only committed usage here would silently stop flagging exactly the rule an
/// owner most needs to re-review — writes that keep going unconfirmed.
#[test]
fn retained_reserved_usage_still_drives_the_drift_trigger() {
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::from_second(1_000_000).unwrap();
    let m = manifest(
        "rule-drift-reserved",
        "digest.send",
        7 * 24 * 3600,
        BudgetWindow {
            max: 100,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 1,
            window_secs: 60,
        },
        None,
    );
    store.activate_standing_rule(&m, None, now).unwrap();
    let activated: i64 = store
        .conn
        .lock()
        .query_row(
            "SELECT activated_at FROM standing_rules WHERE rule_id = 'rule-drift-reserved'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    // Saturate three distinct rate windows with rows left `reserved`, exactly
    // as a delivery-unknown outcome now leaves them.
    for window in 0..3i64 {
        store
            .conn
            .lock()
            .execute(
                "INSERT INTO standing_rule_usage                  (rule_id, version, kind, used_at, status, reservation_id)                  VALUES ('rule-drift-reserved', 1, 'rate', ?1, 'reserved', ?2)",
                rusqlite::params![
                    activated + window * 60 * 1_000_000_000 + 1,
                    format!("res-{window}")
                ],
            )
            .unwrap();
    }
    assert!(
        store
            .active_standing_rule_for_action(&ActionId::new("digest.send"), now)
            .unwrap()
            .is_some(),
        "the rule is active before the trigger is evaluated"
    );

    store
        .note_standing_rule_use("rule-drift-reserved", now)
        .unwrap();

    let status: String = store
        .conn
        .lock()
        .query_row(
            "SELECT status FROM standing_rules WHERE rule_id = 'rule-drift-reserved'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        status, "needs_review",
        "three saturated rate windows of retained usage flag the rule for owner re-review"
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.drift_detected")
            .unwrap(),
        1,
        "the drift trigger leaves durable owner-actionable evidence"
    );
}

/// #128: the `commit_artifact_activation` entry point is guarded too. The
/// activation-time reviewed-scope check runs before that transaction opens, so
/// an artifact activation carrying an incomplete standing rule is refused, the
/// durable audit survives, and neither the rule row nor the learned-artifact
/// row is written.
#[test]
fn artifact_activation_commit_refuses_an_unbound_scope_rule() {
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::now();
    let unbound = manifest(
        "rule-activation-commit",
        "email.create_draft",
        3600,
        BudgetWindow {
            max: 3,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 3,
            window_secs: 60,
        },
        None,
    );
    let err = store
        .commit_artifact_activation(crate::store::activation::ActivationCommit {
            learned: learned_artifact(),
            proposed_id: Ulid::new(),
            grant_id: None,
            payload_ref: None,
            dangling: true,
            superseded_old_version: None,
            live_descriptor_version: None,
            live_implementation_version: None,
            live_policy_version: None,
            standing_rule: Some((unbound, None)),
            owner_review_approval: None,
        })
        .expect_err("an incomplete binding must not activate through the artifact commit");

    assert!(
        err.to_string().contains("must bind a reviewed scope"),
        "unexpected error: {err}"
    );
    assert!(
        store
            .active_standing_rule_for_action(&ActionId::new("email.create_draft"), now)
            .unwrap()
            .is_none(),
        "a refused activation writes no rule row"
    );
    assert!(
        store.list_learned_artifacts().unwrap().is_empty(),
        "the refusal happens before the activation transaction, so nothing else lands"
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.activation_refused")
            .unwrap(),
        1,
        "the refusal leaves durable owner-actionable evidence"
    );
}

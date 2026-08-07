//! Eval-verdict epoch binding and read-time staleness
//! (change `make-reusable-authority-evaluation-proposal-specific`).
//!
//! Done-when: a verdict records the epochs it was computed under, and
//! staleness is a question answered at read time by comparing what the
//! verdict bound against what is live — never by sweeping or rewriting rows.

use jiff::Timestamp;
use ulid::Ulid;

use super::eval_verdict_store::{insert_eval_verdict_conn, EvalVerdict, VerdictEpochs};
use super::Store;

fn digest(tag: &str) -> String {
    format!("sha256:{tag}")
}

/// Every axis bound to a distinct value, so a mutation on any one axis is
/// observable and cannot be masked by another axis matching.
fn full_epochs() -> VerdictEpochs {
    VerdictEpochs {
        proposal_digest: Some(digest("proposal-1")),
        compatibility_digest: Some(digest("compat-1")),
        reviewed_scope_digest: Some(digest("scope-1")),
        evidence_set_digest: Some(digest("evidence-1")),
        descriptor_version: Some(3),
        implementation_version: Some(7),
        policy_version: Some(11),
    }
}

fn verdict_with(epochs: VerdictEpochs, recorded_at: Timestamp) -> EvalVerdict {
    EvalVerdict {
        id: Ulid::new(),
        artifact_kind: "proposal".to_string(),
        artifact_id: "prop-a".to_string(),
        artifact_version: 1,
        verdict: "approved".to_string(),
        fitness: Some(0.9),
        evidence: None,
        evaluator: Some("overlay-eval-gate/replay@v1".to_string()),
        artifact_digest: digest("proposal-1"),
        recorded_at,
        epochs,
    }
}

#[test]
fn verdict_epochs_round_trip_through_the_store() {
    // Every read path must return the recorded epochs verbatim: a dropped
    // column would silently turn a bound verdict into an unbound one, which
    // reads as "current" forever.
    let store = Store::open_in_memory().expect("open store");
    let row = verdict_with(full_epochs(), Timestamp::now());
    store.insert_eval_verdict(&row).expect("insert");

    // The `_conn` insert variant must carry the same columns; a second
    // artifact keeps the per-artifact reads unambiguous.
    let mut via_conn = verdict_with(full_epochs(), Timestamp::now());
    via_conn.artifact_id = "prop-b".to_string();
    via_conn.epochs.policy_version = Some(12);
    {
        let mut conn = store.conn.lock();
        let tx = conn.transaction().expect("tx");
        insert_eval_verdict_conn(&tx, &via_conn).expect("insert via conn");
        tx.commit().expect("commit");
    }

    let by_artifact = store
        .eval_verdicts_for_artifact("proposal", "prop-a", 1)
        .expect("by artifact");
    assert_eq!(by_artifact.len(), 1);
    assert_eq!(by_artifact[0].epochs, full_epochs());

    let latest = store
        .latest_eval_verdict("proposal", "prop-b", 1)
        .expect("latest")
        .expect("present");
    assert_eq!(latest.epochs, via_conn.epochs);
    assert_eq!(latest.epochs.policy_version, Some(12));

    let by_verdict = store
        .eval_verdicts_by_verdict("approved")
        .expect("by verdict");
    assert_eq!(by_verdict.len(), 2);
    assert!(
        by_verdict
            .iter()
            .all(|r| r.epochs.compatibility_digest == Some(digest("compat-1"))),
        "every read path must hydrate epochs; got {:?}",
        by_verdict.iter().map(|r| &r.epochs).collect::<Vec<_>>()
    );
}

#[test]
fn verdict_is_current_when_every_recorded_epoch_matches() {
    let recorded = full_epochs();
    let live = full_epochs();
    assert!(recorded.is_current_against(&live));
    assert!(recorded.stale_axes(&live).is_empty());

    // Currency is not symmetry with "all None": an unbound live snapshot
    // against an unbound verdict is also current.
    let none = VerdictEpochs::default();
    assert!(none.is_current_against(&live));
    assert!(none.is_current_against(&none));
}

#[test]
fn changed_compatibility_digest_makes_verdict_stale() {
    let recorded = full_epochs();
    let mut live = full_epochs();
    live.compatibility_digest = Some(digest("compat-2"));

    assert!(!recorded.is_current_against(&live));
    assert_eq!(recorded.stale_axes(&live), vec!["compatibility_digest"]);
}

#[test]
fn changed_reviewed_scope_or_evidence_digest_makes_verdict_stale() {
    let recorded = full_epochs();

    let mut scope_moved = full_epochs();
    scope_moved.reviewed_scope_digest = Some(digest("scope-2"));
    assert!(!recorded.is_current_against(&scope_moved));
    assert_eq!(
        recorded.stale_axes(&scope_moved),
        vec!["reviewed_scope_digest"]
    );

    let mut evidence_moved = full_epochs();
    evidence_moved.evidence_set_digest = Some(digest("evidence-2"));
    assert!(!recorded.is_current_against(&evidence_moved));
    assert_eq!(
        recorded.stale_axes(&evidence_moved),
        vec!["evidence_set_digest"]
    );
}

#[test]
fn changed_descriptor_implementation_or_policy_version_makes_verdict_stale() {
    let recorded = full_epochs();

    for (name, mutate) in [
        (
            "descriptor_version",
            (|e: &mut VerdictEpochs| e.descriptor_version = Some(4)) as fn(&mut VerdictEpochs),
        ),
        ("implementation_version", |e: &mut VerdictEpochs| {
            e.implementation_version = Some(8)
        }),
        ("policy_version", |e: &mut VerdictEpochs| {
            e.policy_version = Some(12)
        }),
    ] {
        let mut live = full_epochs();
        mutate(&mut live);
        assert!(
            !recorded.is_current_against(&live),
            "bumping {name} must make the verdict stale"
        );
        assert_eq!(recorded.stale_axes(&live), vec![name]);
    }

    // A version that moves *backwards* is equally a mismatch: currency is
    // equality, not monotonicity.
    let mut rolled_back = full_epochs();
    rolled_back.policy_version = Some(10);
    assert!(!recorded.is_current_against(&rolled_back));
}

#[test]
fn recorded_epoch_whose_live_value_disappeared_is_stale() {
    // The axis vanishing is not "nothing to compare" — the verdict bound
    // itself to a value that no longer exists, so it must not be reused.
    let recorded = full_epochs();

    let mut scope_gone = full_epochs();
    scope_gone.reviewed_scope_digest = None;
    assert!(!recorded.is_current_against(&scope_gone));
    assert_eq!(
        recorded.stale_axes(&scope_gone),
        vec!["reviewed_scope_digest"]
    );

    let mut version_gone = full_epochs();
    version_gone.implementation_version = None;
    assert!(!recorded.is_current_against(&version_gone));
    assert_eq!(
        recorded.stale_axes(&version_gone),
        vec!["implementation_version"]
    );
}

#[test]
fn unrecorded_epoch_axis_is_not_compared() {
    // A proposal kind with no reviewed scope records None there; the live
    // world having a value on that axis must not invalidate the verdict.
    let recorded = VerdictEpochs {
        proposal_digest: Some(digest("proposal-1")),
        ..VerdictEpochs::default()
    };
    let live = full_epochs();

    assert!(recorded.is_current_against(&live));
    assert!(recorded.stale_axes(&live).is_empty());

    // Only the axis it did record can make it stale.
    let mut proposal_moved = full_epochs();
    proposal_moved.proposal_digest = Some(digest("proposal-2"));
    assert_eq!(
        recorded.stale_axes(&proposal_moved),
        vec!["proposal_digest"]
    );
}

#[test]
fn stale_axes_names_every_mismatching_axis() {
    // Denial text must list all failures, not just the first one found.
    let recorded = full_epochs();
    let live = VerdictEpochs {
        proposal_digest: Some(digest("proposal-2")),
        compatibility_digest: Some(digest("compat-1")),
        reviewed_scope_digest: None,
        evidence_set_digest: Some(digest("evidence-2")),
        descriptor_version: Some(3),
        implementation_version: Some(8),
        policy_version: None,
    };

    assert_eq!(
        recorded.stale_axes(&live),
        vec![
            "proposal_digest",
            "reviewed_scope_digest",
            "evidence_set_digest",
            "implementation_version",
            "policy_version",
        ]
    );
    assert!(!recorded.is_current_against(&live));
}

#[test]
fn legacy_rows_without_epochs_read_back_as_all_none() {
    // The migration adds nullable columns and backfills nothing, so a row
    // written before #134 stays readable and records "nothing bound".
    let store = Store::open_in_memory().expect("open store");
    let recorded_at = Timestamp::now();
    {
        let conn = store.conn.lock();
        conn.execute(
            "INSERT INTO eval_verdicts \
             (id, artifact_kind, artifact_id, artifact_version, verdict, \
              fitness, evidence, evaluator, artifact_digest, recorded_at) \
             VALUES (?1, 'proposal', 'legacy', 1, 'approved', NULL, NULL, NULL, ?2, ?3)",
            rusqlite::params![
                Ulid::new().to_string(),
                digest("legacy"),
                i64::try_from(recorded_at.as_nanosecond()).expect("nanos"),
            ],
        )
        .expect("legacy insert must succeed without the epoch columns");
    }

    let latest = store
        .latest_eval_verdict("proposal", "legacy", 1)
        .expect("latest")
        .expect("present");
    assert_eq!(latest.epochs, VerdictEpochs::default());

    // All-None never claims currency it did not earn, and never claims
    // staleness either: nothing was bound, so nothing is compared.
    assert!(latest.epochs.is_current_against(&full_epochs()));
    assert!(latest.epochs.stale_axes(&full_epochs()).is_empty());
}

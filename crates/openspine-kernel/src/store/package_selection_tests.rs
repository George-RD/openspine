//! Selection record, decision consumption and rollback (ANO-822).
use super::learned_artifacts::CompatibilityStatus;
use super::package_selection_fixtures::{
    decision, identity, install, learned, refusal, select, status_of,
};
use super::Store;
use crate::package::selection_types::{ReconfirmationKey, SelectionDecision, SelectionRefusal};
use jiff::{SignedDuration, Timestamp};
use openspine_schemas::digest::digest_of_bytes;
use ulid::Ulid;

type Alteration = Box<dyn Fn(&mut SelectionDecision)>;

#[test]
fn first_selection_commits_one_audited_generation_and_validates() {
    let store = Store::open_in_memory().unwrap();
    let (a, b) = (install(&store, 1), install(&store, 2));
    assert!(store.package_selection().unwrap().is_none());
    store.validate_package_selection().unwrap();
    let now = Timestamp::now();
    select(&store, &a, now);
    select(&store, &b, now);
    let record = store.package_selection().unwrap().unwrap();
    assert_eq!(
        (
            record.generation,
            record.invalidation_generation,
            record.installation_id
        ),
        (2, 0, b.installation_id)
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("package.selection_presented")
            .unwrap(),
        2
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("package.selection_committed")
            .unwrap(),
        2
    );
    store.validate_package_selection().unwrap();
    store.validate_package_ledger().unwrap();
}

#[test]
fn a_decision_is_consumed_once_and_a_replay_cannot_rewind() {
    let store = Store::open_in_memory().unwrap();
    let (a, b) = (install(&store, 1), install(&store, 2));
    let now = Timestamp::now();
    let first = decision(&store, &a, now);
    store
        .record_package_selection_decision(&first, now)
        .unwrap();
    store.commit_package_selection(&first, now).unwrap();
    let replay = store.commit_package_selection(&first, now).unwrap_err();
    assert_eq!(refusal(replay), SelectionRefusal::AlreadyConsumed);
    // A->B->A: the generation moves forward each time and the old A->B
    // decision presented at generation 1 can never be consumed afterwards.
    let a_to_b = decision(&store, &b, now);
    store
        .record_package_selection_decision(&a_to_b, now)
        .unwrap();
    select(&store, &b, now);
    select(&store, &a, now);
    let record = store.package_selection().unwrap().unwrap();
    assert_eq!(
        (record.generation, record.installation_id),
        (3, a.installation_id)
    );
    let stale = store.commit_package_selection(&a_to_b, now).unwrap_err();
    assert_eq!(refusal(stale), SelectionRefusal::GenerationChanged);
    store.validate_package_selection().unwrap();
}

#[test]
fn racing_selectors_cannot_both_consume_the_same_generation() {
    let store = Store::open_in_memory().unwrap();
    let (a, b, c) = (install(&store, 1), install(&store, 2), install(&store, 3));
    select(&store, &a, Timestamp::now());
    let now = Timestamp::now();
    let to_b = decision(&store, &b, now);
    let to_c = decision(&store, &c, now);
    store.record_package_selection_decision(&to_b, now).unwrap();
    store.record_package_selection_decision(&to_c, now).unwrap();
    let results: Vec<_> = std::thread::scope(|scope| {
        let handles = [&to_b, &to_c].map(|decision| {
            let store = store.clone();
            scope.spawn(move || store.commit_package_selection(decision, now))
        });
        handles
            .map(|handle| handle.join().unwrap())
            .into_iter()
            .collect()
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let loser = results.into_iter().find_map(Result::err).unwrap();
    assert_eq!(refusal(loser), SelectionRefusal::GenerationChanged);
    assert_eq!(store.package_selection().unwrap().unwrap().generation, 2);
    store.validate_package_selection().unwrap();
}

#[test]
fn any_altered_binding_or_expiry_selects_nothing() {
    let store = Store::open_in_memory().unwrap();
    let (a, b) = (install(&store, 1), install(&store, 2));
    let now = Timestamp::now();
    let presented = decision(&store, &a, now);
    store
        .record_package_selection_decision(&presented, now)
        .unwrap();
    let alterations: Vec<Alteration> = vec![
        Box::new(|d| d.plan_digest = digest_of_bytes(b"other plan")),
        Box::new(|d| d.owner_binding = "hmac-sha256:foreign".into()),
        Box::new(|d| d.principal = "foreign".into()),
        Box::new(|d| d.runtime_compatibility = digest_of_bytes(b"other runtime")),
        Box::new(|d| d.control_fingerprint = digest_of_bytes(b"other controls")),
        Box::new(|d| d.source.identity = identity(98)),
        Box::new(|d| d.expires_at += SignedDuration::from_secs(1)),
    ];
    for alter in alterations {
        let mut forged = presented.clone();
        alter(&mut forged);
        let error = store.commit_package_selection(&forged, now).unwrap_err();
        assert_eq!(refusal(error), SelectionRefusal::Mismatch);
    }
    // Another target is a different decision that was never presented.
    let mut other = decision(&store, &b, now);
    other.decision_id = presented.decision_id;
    let error = store.commit_package_selection(&other, now).unwrap_err();
    assert_eq!(refusal(error), SelectionRefusal::Mismatch);
    // Expired, and a clock before presentation, both refuse.
    for at in [
        presented.expires_at,
        presented.presented_at - SignedDuration::from_secs(1),
    ] {
        let error = store.commit_package_selection(&presented, at).unwrap_err();
        assert_eq!(refusal(error), SelectionRefusal::Expired);
    }
    assert!(store.package_selection().unwrap().is_none());
    assert_eq!(
        store
            .count_audit_events_of_kind("package.selection_committed")
            .unwrap(),
        0
    );
}

#[test]
fn decisions_are_bounded_and_must_describe_current_state() {
    let store = Store::open_in_memory().unwrap();
    let a = install(&store, 1);
    let now = Timestamp::now();
    let mut long = decision(&store, &a, now);
    long.expires_at = now + SignedDuration::from_mins(31);
    assert_eq!(
        refusal(
            store
                .record_package_selection_decision(&long, now)
                .unwrap_err()
        ),
        SelectionRefusal::Malformed
    );
    let mut wrong_generation = decision(&store, &a, now);
    wrong_generation.expected_generation = 4;
    assert_eq!(
        refusal(
            store
                .record_package_selection_decision(&wrong_generation, now)
                .unwrap_err()
        ),
        SelectionRefusal::GenerationChanged
    );
    let mut unknown_target = decision(&store, &a, now);
    unknown_target.target.installation_id = Ulid::new();
    assert_eq!(
        refusal(
            store
                .record_package_selection_decision(&unknown_target, now)
                .unwrap_err()
        ),
        SelectionRefusal::TargetUnavailable
    );
    let mut cross_product = decision(&store, &a, now);
    cross_product.source.identity.package_id = "bell".into();
    assert_eq!(
        refusal(
            store
                .record_package_selection_decision(&cross_product, now)
                .unwrap_err()
        ),
        SelectionRefusal::Malformed
    );
    let unsorted = {
        let mut d = decision(&store, &a, now);
        d.base_epoch_changed = true;
        d.reconfirmations = ["b", "a"]
            .map(|id| ReconfirmationKey {
                kind: "route".into(),
                artifact_id: id.into(),
                version: 1,
            })
            .to_vec();
        d
    };
    assert_eq!(
        refusal(
            store
                .record_package_selection_decision(&unsorted, now)
                .unwrap_err()
        ),
        SelectionRefusal::Malformed
    );
    select(&store, &a, now);
    // Selecting what is already selected is not a transition.
    let same = decision(&store, &a, now);
    assert_eq!(
        refusal(
            store
                .record_package_selection_decision(&same, now)
                .unwrap_err()
        ),
        SelectionRefusal::Malformed
    );
}

#[test]
fn audit_failure_rolls_back_selection_decision_and_consequences() {
    let store = Store::open_in_memory().unwrap();
    let (a, b) = (install(&store, 1), install(&store, 2));
    let now = Timestamp::now();
    select(&store, &a, now);
    store
        .record_learned_artifact(&learned(
            "route",
            "mine",
            CompatibilityStatus::OwnerAccepted,
        ))
        .unwrap();
    let mut changing = decision(&store, &b, now);
    changing.base_epoch_changed = true;
    changing.reconfirmations = vec![ReconfirmationKey {
        kind: "route".into(),
        artifact_id: "mine".into(),
        version: 1,
    }];
    store
        .record_package_selection_decision(&changing, now)
        .unwrap();
    store
        .with_immediate_tx(|tx| {
            tx.execute_batch(
                "CREATE TRIGGER fail_selection_receipt BEFORE INSERT ON audit_log
                 WHEN NEW.kind = 'package.selection_committed'
                 BEGIN SELECT RAISE(ABORT, 'injected audit failure'); END;",
            )?;
            Ok(())
        })
        .unwrap();
    assert!(store.commit_package_selection(&changing, now).is_err());
    assert_eq!(store.package_selection().unwrap().unwrap().generation, 1);
    assert_eq!(
        status_of(&store, "mine"),
        CompatibilityStatus::OwnerAccepted
    );
    store
        .with_immediate_tx(|tx| {
            tx.execute_batch("DROP TRIGGER fail_selection_receipt;")?;
            Ok(())
        })
        .unwrap();
    // The failed attempt did not consume the decision: it can still commit.
    store.commit_package_selection(&changing, now).unwrap();
    assert_eq!(
        status_of(&store, "mine"),
        CompatibilityStatus::ReconfirmationRequired
    );
    store.validate_package_selection().unwrap();
}

#[test]
fn epoch_change_invalidates_reusable_authority_and_never_revives_it() {
    let store = Store::open_in_memory().unwrap();
    let (a, b) = (install(&store, 1), install(&store, 2));
    let now = Timestamp::now();
    for (id, status) in [
        ("accepted", CompatibilityStatus::OwnerAccepted),
        ("plain", CompatibilityStatus::Compatible),
        ("erased", CompatibilityStatus::Erased),
    ] {
        store
            .record_learned_artifact(&learned("route", id, status))
            .unwrap();
    }
    select(&store, &a, now);
    assert_eq!(
        store
            .package_selection()
            .unwrap()
            .unwrap()
            .invalidation_generation,
        0
    );
    let mut to_b = decision(&store, &b, now);
    to_b.base_epoch_changed = true;
    to_b.reconfirmations = ["accepted", "plain"]
        .map(|id| ReconfirmationKey {
            kind: "route".into(),
            artifact_id: id.into(),
            version: 1,
        })
        .to_vec();
    store.record_package_selection_decision(&to_b, now).unwrap();
    store.commit_package_selection(&to_b, now).unwrap();
    assert_eq!(
        store
            .package_selection()
            .unwrap()
            .unwrap()
            .invalidation_generation,
        1
    );
    assert_eq!(
        status_of(&store, "accepted"),
        CompatibilityStatus::ReconfirmationRequired
    );
    assert_eq!(
        status_of(&store, "plain"),
        CompatibilityStatus::ReconfirmationRequired
    );
    assert_eq!(status_of(&store, "erased"), CompatibilityStatus::Erased);
    // Returning to A is a new epoch change: the generation and the
    // invalidation generation both advance, nothing is revived.
    let mut back = decision(&store, &a, now);
    back.base_epoch_changed = true;
    store.record_package_selection_decision(&back, now).unwrap();
    store.commit_package_selection(&back, now).unwrap();
    let record = store.package_selection().unwrap().unwrap();
    assert_eq!((record.generation, record.invalidation_generation), (3, 2));
    assert_eq!(
        status_of(&store, "accepted"),
        CompatibilityStatus::ReconfirmationRequired
    );
    // An unchanged epoch keeps the invalidation generation.
    select(&store, &b, now);
    assert_eq!(
        store
            .package_selection()
            .unwrap()
            .unwrap()
            .invalidation_generation,
        2
    );
    store.validate_package_selection().unwrap();
}

#[test]
fn a_consequence_that_no_longer_applies_refuses_and_rolls_back() {
    let store = Store::open_in_memory().unwrap();
    let (a, b) = (install(&store, 1), install(&store, 2));
    let now = Timestamp::now();
    select(&store, &a, now);
    store
        .record_learned_artifact(&learned("route", "gone", CompatibilityStatus::Erased))
        .unwrap();
    let mut stale = decision(&store, &b, now);
    stale.base_epoch_changed = true;
    stale.reconfirmations = vec![ReconfirmationKey {
        kind: "route".into(),
        artifact_id: "gone".into(),
        version: 1,
    }];
    store
        .record_package_selection_decision(&stale, now)
        .unwrap();
    let error = store.commit_package_selection(&stale, now).unwrap_err();
    assert_eq!(refusal(error), SelectionRefusal::ConsequenceStale);
    assert_eq!(store.package_selection().unwrap().unwrap().generation, 1);
    assert_eq!(status_of(&store, "gone"), CompatibilityStatus::Erased);
}

#[test]
fn epoch_change_sends_active_standing_rules_to_review_and_stales_their_exceptions() {
    let now = Timestamp::now();
    let (store, pending_id, _timer) =
        super::standing_rules_staleness_tests::scheduled_exception("selected-rule", now);
    let action =
        openspine_schemas::action::ActionId::new(super::standing_rules_staleness_tests::ACTION);
    let (a, b) = (install(&store, 1), install(&store, 2));
    select(&store, &a, now);
    // An unchanged epoch leaves the live rule and its open exception alone.
    select(&store, &b, now);
    assert!(store
        .active_standing_rule_for_action(&action, now)
        .unwrap()
        .is_some());
    assert_eq!(
        super::standing_rules_staleness_tests::resolution(&store, &pending_id),
        None
    );

    let mut changing = decision(&store, &a, now);
    changing.base_epoch_changed = true;
    store
        .record_package_selection_decision(&changing, now)
        .unwrap();
    store.commit_package_selection(&changing, now).unwrap();
    // The live reusable authority is no longer reusable until reviewed again.
    assert!(store
        .active_standing_rule_for_action(&action, now)
        .unwrap()
        .is_none());
    assert_eq!(
        super::standing_rules_staleness_tests::resolution(&store, &pending_id).as_deref(),
        Some("stale")
    );
    let status: String = store
        .conn
        .lock()
        .query_row(
            "SELECT status FROM standing_rules WHERE rule_id = 'selected-rule'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "needs_review");
    let receipt = store.with_deferred_read(|tx| {
        Ok(tx.query_row(
            "SELECT meta_json FROM audit_log WHERE kind = 'package.selection_committed' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )?)
    }).unwrap();
    assert!(receipt.contains("selected-rule"), "{receipt}");
    store.validate_package_selection().unwrap();
}

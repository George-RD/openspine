//! Focused lifecycle transition tests for standing rules.

use jiff::Timestamp;
use openspine_schemas::standing_rule::BudgetWindow;

use super::{standing_rules_tests::manifest, PauseStandingRuleOutcome, Store};
fn run_pair<T: Send + 'static>(store: &Store, operation: fn(&Store) -> T) -> Vec<T> {
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let handles = (0..2)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                operation(&store)
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect()
}

#[test]
fn new_version_supersedes_a_paused_rule() {
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::from_second(2_000_000).unwrap();
    let v1 = manifest(
        "rule-supersede-paused",
        "calendar.book",
        3600,
        BudgetWindow {
            max: 2,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 2,
            window_secs: 3600,
        },
        None,
    );
    store.activate_standing_rule(&v1, None, now).unwrap();
    store
        .pause_standing_rule("rule-supersede-paused", Timestamp::now())
        .unwrap();
    let mut v2 = v1.clone();
    v2.version = 2;
    store.activate_standing_rule(&v2, None, now).unwrap();
    assert!(!store
        .resume_standing_rule("rule-supersede-paused", 1)
        .unwrap());
    assert!(store
        .standing_rule_is_current("rule-supersede-paused", 2)
        .unwrap());
}

#[test]
fn concurrent_lifecycle_intents_write_one_transition_audit() {
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::from_second(2_000_000).unwrap();
    let rule = manifest(
        "rule-concurrent-lifecycle",
        "calendar.book",
        3600,
        BudgetWindow {
            max: 2,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 2,
            window_secs: 3600,
        },
        None,
    );
    store.activate_standing_rule(&rule, None, now).unwrap();

    let paused = run_pair(&store, |store| {
        store
            .pause_standing_rule("rule-concurrent-lifecycle", Timestamp::now())
            .unwrap()
    });
    assert_eq!(
        paused
            .iter()
            .filter(|outcome| matches!(outcome, PauseStandingRuleOutcome::Paused))
            .count(),
        1
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.paused")
            .unwrap(),
        1
    );

    let resumed = run_pair(&store, |store| {
        store
            .resume_standing_rule("rule-concurrent-lifecycle", 1)
            .unwrap()
    });
    assert_eq!(resumed.iter().filter(|changed| **changed).count(), 1);
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.resumed")
            .unwrap(),
        1
    );

    let revoked = run_pair(&store, |store| {
        store
            .revoke_standing_rule("rule-concurrent-lifecycle", Timestamp::now())
            .unwrap()
    });
    assert_eq!(revoked.iter().filter(|changed| **changed).count(), 1);
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.revoked")
            .unwrap(),
        1
    );
}

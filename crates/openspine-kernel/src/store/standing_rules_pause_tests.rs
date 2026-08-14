//! Pause/resume live-consultation behaviour for standing rules (#129), split
//! from `standing_rules_tests.rs` for the 500-line gate.

use super::standing_rules_tests::manifest;
use super::{PauseStandingRuleOutcome, Store};
use jiff::Timestamp;
use openspine_schemas::standing_rule::BudgetWindow;

#[test]
fn owner_revoke_action_removes_rule_from_live_consultation() {
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::from_second(2_000_000).unwrap();
    let rule = manifest(
        "rule-owner-revoke",
        "calendar.book",
        3600,
        BudgetWindow {
            max: 2,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 2,
            window_secs: 60,
        },
        None,
    );
    store.activate_standing_rule(&rule, None, now).unwrap();
    assert!(store
        .standing_rule_is_current("rule-owner-revoke", 1)
        .unwrap());
    assert!(store
        .revoke_standing_rule("rule-owner-revoke", now)
        .unwrap());
    assert!(!store
        .standing_rule_is_current("rule-owner-revoke", 1)
        .unwrap());
}

#[test]
fn pause_removes_rule_from_live_consultation_and_resume_restores_it() {
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::from_second(2_000_000).unwrap();
    let rule = manifest(
        "rule-pause-resume",
        "calendar.book",
        3600,
        BudgetWindow {
            max: 2,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 2,
            window_secs: 60,
        },
        None,
    );
    store.activate_standing_rule(&rule, None, now).unwrap();
    assert!(store
        .standing_rule_is_current("rule-pause-resume", 1)
        .unwrap());
    // Pause removes it from live consultation.
    assert!(matches!(
        store
            .pause_standing_rule("rule-pause-resume", Timestamp::now())
            .unwrap(),
        PauseStandingRuleOutcome::Paused
    ));
    assert!(!store
        .standing_rule_is_current("rule-pause-resume", 1)
        .unwrap());
    // Pausing again is a safe no-op.
    assert!(matches!(
        store
            .pause_standing_rule("rule-pause-resume", Timestamp::now())
            .unwrap(),
        PauseStandingRuleOutcome::AlreadyPaused
    ));
    // Resume restores it.
    assert!(store.resume_standing_rule("rule-pause-resume", 1).unwrap());
    assert!(store
        .standing_rule_is_current("rule-pause-resume", 1)
        .unwrap());
    // Resuming again is a safe no-op.
    assert!(!store.resume_standing_rule("rule-pause-resume", 1).unwrap());
}

#[test]
fn pause_and_resume_write_distinct_audit_events() {
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::from_second(2_000_000).unwrap();
    let rule = manifest(
        "rule-pause-audit",
        "calendar.book",
        3600,
        BudgetWindow {
            max: 2,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 2,
            window_secs: 60,
        },
        None,
    );
    store.activate_standing_rule(&rule, None, now).unwrap();
    store
        .pause_standing_rule("rule-pause-audit", Timestamp::now())
        .unwrap();
    store.resume_standing_rule("rule-pause-audit", 1).unwrap();
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.paused")
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.resumed")
            .unwrap(),
        1
    );
}

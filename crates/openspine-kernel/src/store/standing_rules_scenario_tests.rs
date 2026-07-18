//! Named 1:1 scenario and fault-injection regression tests for the
//! standing-rule artifact class (AD-010, AD-106, AD-012, SrFinalSec
//! retraction). Split out of `standing_rules_tests.rs` to keep both files
//! under the 500-line gate. Every test asserts durable store state / audit
//! outcomes, not helper return values.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use rusqlite::{params, OptionalExtension};

use super::standing_rules_tests::manifest;
use super::Store;

/// Identity `(rule_id, version)` a reservation row currently carries, or
/// `None` if it was cancelled/finalized away.
fn reservation_identity(store: &Store, reservation_id: &str) -> Option<(String, i64)> {
    store
        .conn
        .lock()
        .query_row(
            "SELECT rule_id, version FROM standing_rule_usage \
             WHERE reservation_id = ?1 AND status = 'reserved' LIMIT 1",
            params![reservation_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .unwrap()
}

/// Count reserved (not yet finalized) usage rows for a rule — white-box
/// introspection of the budget the gate has committed on disk.
fn reserved_usage_count(store: &Store, rule_id: &str) -> i64 {
    store
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(DISTINCT reservation_id) FROM standing_rule_usage WHERE rule_id = ?1 AND status = 'reserved'",
            params![rule_id],
            |r| r.get(0),
        )
        .unwrap()
}

/// Count committed usage rows for a rule.
fn committed_usage_count(store: &Store, rule_id: &str) -> i64 {
    store
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(DISTINCT reservation_id) FROM standing_rule_usage WHERE rule_id = ?1 AND status = 'committed'",
            params![rule_id],
            |r| r.get(0),
        )
        .unwrap()
}

fn rule_status(store: &Store, rule_id: &str) -> String {
    store
        .conn
        .lock()
        .query_row(
            "SELECT status FROM standing_rules WHERE rule_id = ?1",
            params![rule_id],
            |r| r.get(0),
        )
        .unwrap_or_default()
}

#[test]
fn standing_rule_read_failure_cancels_reservation_no_leak() {
    // SrFinalSec retraction regression: `standing_rule_remaining` is a fallible
    // DB read that runs AFTER `consult_and_reserve_standing_rule` has committed
    // a reservation. If it fails, the gate must cancel that reservation so no
    // quota/rate leaks. Here the failed read returns Err and leaves the
    // already-reserved headroom exactly as it was before the gate ran.
    let store = Store::open_in_memory().unwrap();
    let m = manifest(
        "rule-read-fail",
        "task.dispatch",
        7 * 24 * 3600,
        openspine_schemas::standing_rule::BudgetWindow {
            max: 3,
            window_secs: 7 * 24 * 3600,
        },
        openspine_schemas::standing_rule::BudgetWindow {
            max: 3,
            window_secs: 3600,
        },
        None,
    );
    let now = Timestamp::from_second(1_000_000).unwrap();
    store.activate_standing_rule(&m, None, now).unwrap();
    let action = ActionId::new("task.dispatch");

    // A pre-existing reserved unit (the caller's own earlier reservation).
    let (rule0, res0) = store
        .consult_and_reserve_standing_rule(&action, now)
        .unwrap()
        .expect("rule matched");
    let res0 = res0.expect("headroom available");
    assert_eq!(reserved_usage_count(&store, &rule0.rule_id), 1);

    // Arm a one-shot failure of the post-reservation headroom read, then run
    // the gate path that performs the read.
    store.fail_next_standing_rule_remaining_for_test();
    let result = crate::standing_rules_gate::consult_standing_rule_gate(&store, &action, now, None);
    assert!(
        result.is_err(),
        "a headroom read failure must surface as an error, not silently leak"
    );
    // Only the caller's own reservation remains: the gate's would-be
    // reservation was cancelled, so no extra row leaked.
    assert_eq!(
        reserved_usage_count(&store, &rule0.rule_id),
        1,
        "the failed gate read must cancel its own reservation (no leak)"
    );
    assert!(
        reservation_identity(&store, &res0).is_some(),
        "the pre-existing reservation is untouched by the gate's failure"
    );
    // The action was never effectively allowed (no effective-Allow audit).
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.dark_window_admitted")
            .unwrap(),
        0
    );
}

#[test]
fn standing_rule_rate_window_saturates_independent_of_quota() {
    // AD-106 independent windows: rate saturation must deny even when quota is
    // far from exhausted. Quota is huge; rate max is 2.
    let store = Store::open_in_memory().unwrap();
    let m = manifest(
        "rule-rate-only",
        "digest.send",
        7 * 24 * 3600,
        openspine_schemas::standing_rule::BudgetWindow {
            max: 100,
            window_secs: 7 * 24 * 3600,
        },
        openspine_schemas::standing_rule::BudgetWindow {
            max: 2,
            window_secs: 3600,
        },
        None,
    );
    let now = Timestamp::from_second(1_000_000).unwrap();
    store.activate_standing_rule(&m, None, now).unwrap();
    let action = ActionId::new("digest.send");

    for _ in 0..2 {
        let (rule, res) = store
            .consult_and_reserve_standing_rule(&action, now)
            .unwrap()
            .expect("rule matched");
        let res = res.expect("rate headroom available");
        assert!(store
            .finalize_standing_rule_reservation(&rule.rule_id, rule.version, &res, now)
            .unwrap());
    }
    // Rate saturated; quota still has 98 units free.
    let (third, third_res) = store
        .consult_and_reserve_standing_rule(&action, now)
        .unwrap()
        .expect("rule still matches by action_id");
    assert_eq!(third.rule_id, "rule-rate-only");
    assert!(
        third_res.is_none(),
        "rate window saturated independently of quota — no reservation granted"
    );
    assert_eq!(
        committed_usage_count(&store, "rule-rate-only"),
        2,
        "exactly the two rate units were committed"
    );
}

#[test]
fn standing_rule_concurrent_final_unit_race_exactly_one_wins() {
    // AD-106 concurrent admission: a single remaining unit must be consumed by
    // exactly one of two racing callers; the other sees saturation, not a
    // double-spend.
    let store = Store::open_in_memory().unwrap();
    let m = manifest(
        "rule-race",
        "invoice.send",
        7 * 24 * 3600,
        openspine_schemas::standing_rule::BudgetWindow {
            max: 1,
            window_secs: 7 * 24 * 3600,
        },
        openspine_schemas::standing_rule::BudgetWindow {
            max: 1,
            window_secs: 3600,
        },
        None,
    );
    let now = Timestamp::from_second(1_000_000).unwrap();
    store.activate_standing_rule(&m, None, now).unwrap();

    let wins = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let s = store.clone();
        let w = wins.clone();
        handles.push(std::thread::spawn(move || {
            let action = ActionId::new("invoice.send");
            if let Some((rule, Some(res))) =
                s.consult_and_reserve_standing_rule(&action, now).unwrap()
            {
                if s.finalize_standing_rule_reservation(&rule.rule_id, rule.version, &res, now)
                    .unwrap()
                {
                    w.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(
        wins.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "exactly one concurrent caller wins the single unit"
    );
    assert_eq!(
        committed_usage_count(&store, "rule-race"),
        1,
        "no double-spend: exactly one unit committed"
    );
    let (q, r) = store.standing_rule_remaining("rule-race", now).unwrap();
    assert_eq!((q, r), (0, 0), "budget fully consumed");
}

#[test]
fn standing_rule_drift_saturates_needs_review() {
    // AD-010 drift: three distinct calibrated rate windows each saturated moves
    // the rule to `needs_review`, surfacing re-review; subsequent consultation
    // falls back to normal owner approval (no match).
    let store = Store::open_in_memory().unwrap();
    let m = manifest(
        "rule-drift",
        "reminder.create",
        7 * 24 * 3600,
        openspine_schemas::standing_rule::BudgetWindow {
            max: 100,
            window_secs: 7 * 24 * 3600,
        },
        openspine_schemas::standing_rule::BudgetWindow {
            max: 1,
            window_secs: 3600,
        },
        None,
    );
    let base = Timestamp::from_second(1_000_000).unwrap();
    store.activate_standing_rule(&m, None, base).unwrap();
    let action = ActionId::new("reminder.create");

    for i in 0..3 {
        // Distinct rate windows (spaced beyond the 3600s window).
        let now = base + std::time::Duration::from_secs(3700 * (i + 1));
        let (rule, res) = store
            .consult_and_reserve_standing_rule(&action, now)
            .unwrap()
            .expect("rule matched");
        let res = res.expect("rate headroom available in a fresh window");
        assert!(store
            .finalize_standing_rule_reservation(&rule.rule_id, rule.version, &res, now)
            .unwrap());
    }
    assert_eq!(
        rule_status(&store, "rule-drift"),
        "needs_review",
        "three saturated rate windows must move the rule to needs_review"
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.drift_detected")
            .unwrap(),
        1,
        "drift detection is audited exactly once"
    );
    // After drift, live consultation no longer matches the action.
    assert!(
        store
            .active_standing_rule_for_action(&action, base + std::time::Duration::from_secs(20_000))
            .unwrap()
            .is_none(),
        "drifted rule is absent from live consultation"
    );
}

#[test]
fn standing_rule_gate_response_exposes_headroom() {
    // AD-013/AD-106: a successful (Allow) consultation returns remaining
    // headroom in the gate response so agents self-adjust.
    let store = Store::open_in_memory().unwrap();
    let m = manifest(
        "rule-headroom",
        "digest.send",
        7 * 24 * 3600,
        openspine_schemas::standing_rule::BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        openspine_schemas::standing_rule::BudgetWindow {
            max: 4,
            window_secs: 3600,
        },
        None,
    );
    let now = Timestamp::from_second(1_000_000).unwrap();
    store.activate_standing_rule(&m, None, now).unwrap();
    let outcome = crate::standing_rules_gate::consult_standing_rule_gate(
        &store,
        &ActionId::new("digest.send"),
        now,
        None,
    )
    .unwrap();
    assert!(outcome.matched);
    assert!(outcome.allow);
    assert_eq!(
        outcome.budget_info(),
        Some((4, 3)),
        "headroom returned in the gate response after reserving one unit"
    );
}

#[test]
fn standing_rule_activation_writes_live_row_and_audit() {
    // Standing-rule activation writes the runtime row and its audit evidence
    // transactionally, and the newly activated rule is immediately live for
    // consultation (the activation path has no partial-success gap).
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::from_second(2_000_000).unwrap();
    let rule = manifest(
        "rule-activate",
        "calendar.book",
        3600,
        openspine_schemas::standing_rule::BudgetWindow {
            max: 2,
            window_secs: 3600,
        },
        openspine_schemas::standing_rule::BudgetWindow {
            max: 2,
            window_secs: 60,
        },
        None,
    );
    store.activate_standing_rule(&rule, None, now).unwrap();
    assert!(
        store.standing_rule_is_current("rule-activate", 1).unwrap(),
        "activated rule is live and current"
    );
    assert!(store
        .active_standing_rule_for_action(&ActionId::new("calendar.book"), now)
        .unwrap()
        .is_some());
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.activated")
            .unwrap(),
        1,
        "activation writes its audit evidence transactionally"
    );
}

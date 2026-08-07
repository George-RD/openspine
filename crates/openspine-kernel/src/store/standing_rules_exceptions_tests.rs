//! Bounded dark-window exception tests (#135).
//!
//! The amplification these guard against: the pending fingerprint hashes
//! action, grant, chat, and payload, so before this change a caller whose
//! quota was exhausted could vary any of those and mint a fresh pending
//! exception — with its own timer and its own eventual silence-based Allow —
//! for each variation. The cap is what makes the reviewed budget mean
//! something once the owner stops answering.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::standing_rule::{BudgetWindow, DarkWindowConfig, DarkWindowDefault};
use rusqlite::params;
use ulid::Ulid;

use super::standing_rules::standing_rule_fingerprint;
use super::standing_rules_exceptions::DarkWindowSchedule;
use super::standing_rules_tests::manifest;
use super::Store;

const ACTION: &str = "reminder.create";

fn dark_window_rule(rule_id: &str, max_pending_exceptions: u32) -> Store {
    let store = Store::open_in_memory().unwrap();
    let m = manifest(
        rule_id,
        ACTION,
        7 * 24 * 3600,
        BudgetWindow {
            max: 1,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 1,
            window_secs: 3600,
        },
        Some(DarkWindowConfig {
            timeout_secs: 60,
            default: DarkWindowDefault::Allow,
            max_pending_exceptions,
        }),
    );
    crate::store::standing_rules_tests::activate_or_install_legacy_allow(
        &store,
        &m,
        Timestamp::from_second(1_000_000).unwrap(),
    );
    store
}

fn rule(store: &Store, now: Timestamp) -> super::standing_rules::StandingRule {
    store
        .active_standing_rule_for_action(&ActionId::new(ACTION), now)
        .unwrap()
        .expect("active rule")
}

fn payload(seed: u8) -> Option<ArtifactRef> {
    Some(ArtifactRef {
        digest: digest_of_bytes(&[seed; 16]),
        schema_version: 1,
    })
}

fn open_pending_count(store: &Store, rule_id: &str) -> i64 {
    // Read through the same version-aware helper the cap enforces with, so a
    // divergence between the two would fail here rather than hide.
    store
        .outstanding_dark_window_exceptions(rule_id, 1)
        .unwrap() as i64
}

fn timer_count(store: &Store) -> i64 {
    store
        .conn
        .lock()
        .query_row("SELECT COUNT(*) FROM workflow_timers", [], |row| row.get(0))
        .unwrap()
}

fn audit_count(store: &Store, kind: &str) -> i64 {
    store
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE kind = ?1",
            params![kind],
            |row| row.get(0),
        )
        .unwrap()
}

/// The amplification regression. A hundred over-budget requests varying every
/// component of the fingerprint — payload, grant, and owner-surface chat id —
/// leave exactly one live pending exception at the default cap of one.
#[test]
fn many_distinct_over_budget_requests_yield_one_exception() {
    let store = dark_window_rule("rule-cap-one", 1);
    let now = Timestamp::from_second(1_000_000).unwrap();
    let rule = rule(&store, now);
    let action = ActionId::new(ACTION);

    let mut scheduled = 0;
    let mut suppressed = 0;
    for i in 0..100u8 {
        let grant_id = Ulid::new();
        let chat = 1_000 + i64::from(i);
        let payload_ref = payload(i);
        let fingerprint = standing_rule_fingerprint(&action, grant_id, chat, &payload_ref);
        let outcome = store
            .schedule_standing_rule_dark_window(
                &rule,
                grant_id,
                chat,
                payload_ref,
                &fingerprint,
                None,
                None,
                now + std::time::Duration::from_secs(60),
                now,
            )
            .unwrap();
        match outcome {
            DarkWindowSchedule::Scheduled(_) => scheduled += 1,
            DarkWindowSchedule::SuppressedAtCap => suppressed += 1,
            DarkWindowSchedule::AlreadyCovered => panic!("each request is distinct"),
        }
    }

    assert_eq!(scheduled, 1, "exactly one request took the single slot");
    assert_eq!(suppressed, 99, "every other distinct request was refused");
    assert_eq!(open_pending_count(&store, "rule-cap-one"), 1);
    assert_eq!(
        timer_count(&store),
        1,
        "a refused schedule creates no timer, so silence can fire at most once"
    );
    assert_eq!(
        audit_count(&store, "standing_rule.exception_suppressed_at_cap"),
        99,
        "each refusal leaves durable owner-actionable evidence"
    );
}

/// A reviewed allowance greater than one is honoured exactly — it is a bound,
/// not an on/off switch.
#[test]
fn a_reviewed_allowance_of_two_admits_exactly_two() {
    let store = dark_window_rule("rule-cap-two", 2);
    let now = Timestamp::from_second(1_000_000).unwrap();
    let rule = rule(&store, now);
    let action = ActionId::new(ACTION);
    let mut scheduled = 0;
    for i in 0..10u8 {
        let grant_id = Ulid::new();
        let payload_ref = payload(i);
        let fingerprint = standing_rule_fingerprint(&action, grant_id, 7, &payload_ref);
        if matches!(
            store
                .schedule_standing_rule_dark_window(
                    &rule,
                    grant_id,
                    7,
                    payload_ref,
                    &fingerprint,
                    None,
                    None,
                    now + std::time::Duration::from_secs(60),
                    now,
                )
                .unwrap(),
            DarkWindowSchedule::Scheduled(_)
        ) {
            scheduled += 1;
        }
    }
    assert_eq!(scheduled, 2);
    assert_eq!(open_pending_count(&store, "rule-cap-two"), 2);
}

/// Repeating one request is idempotent and must not spend a second slot: a
/// repeat is not a new exception. With a cap of two, the repeat leaves room
/// for one genuinely distinct request.
#[test]
fn repeating_one_request_consumes_no_second_slot() {
    let store = dark_window_rule("rule-cap-dedup", 2);
    let now = Timestamp::from_second(1_000_000).unwrap();
    let rule = rule(&store, now);
    let action = ActionId::new(ACTION);
    let grant_id = Ulid::new();
    let payload_ref = payload(1);
    let fingerprint = standing_rule_fingerprint(&action, grant_id, 7, &payload_ref);

    for _ in 0..5 {
        store
            .schedule_standing_rule_dark_window(
                &rule,
                grant_id,
                7,
                payload_ref.clone(),
                &fingerprint,
                None,
                None,
                now + std::time::Duration::from_secs(60),
                now,
            )
            .unwrap();
    }
    assert_eq!(open_pending_count(&store, "rule-cap-dedup"), 1);
    assert_eq!(timer_count(&store), 1, "no duplicate timer");
    assert_eq!(
        audit_count(&store, "standing_rule.exception_suppressed_at_cap"),
        0,
        "an idempotent repeat is never refused at the cap"
    );

    // The second slot is still available to a genuinely distinct request.
    let other_payload = payload(2);
    let other_fingerprint = standing_rule_fingerprint(&action, grant_id, 7, &other_payload);
    assert!(matches!(
        store
            .schedule_standing_rule_dark_window(
                &rule,
                grant_id,
                7,
                other_payload,
                &other_fingerprint,
                None,
                None,
                now + std::time::Duration::from_secs(60),
                now,
            )
            .unwrap(),
        DarkWindowSchedule::Scheduled(_)
    ));
    assert_eq!(open_pending_count(&store, "rule-cap-dedup"), 2);
}

/// Resolving an exception frees its slot: the cap bounds *outstanding*
/// exceptions, not a lifetime total. A lifetime bound would silently retire a
/// rule the owner never retired.
#[test]
fn resolving_an_exception_frees_its_slot() {
    let store = dark_window_rule("rule-cap-free", 1);
    let now = Timestamp::from_second(1_000_000).unwrap();
    let rule = rule(&store, now);
    let action = ActionId::new(ACTION);
    let grant_id = Ulid::new();
    let first = payload(1);
    let first_fp = standing_rule_fingerprint(&action, grant_id, 7, &first);
    store
        .schedule_standing_rule_dark_window(
            &rule,
            grant_id,
            7,
            first,
            &first_fp,
            None,
            None,
            now + std::time::Duration::from_secs(60),
            now,
        )
        .unwrap();
    let pending_id: String = store
        .conn
        .lock()
        .query_row(
            "SELECT pending_id FROM standing_rule_pending_actions WHERE request_fingerprint = ?1",
            params![first_fp],
            |row| row.get(0),
        )
        .unwrap();

    let second = payload(2);
    let second_fp = standing_rule_fingerprint(&action, grant_id, 7, &second);
    assert_eq!(
        store
            .schedule_standing_rule_dark_window(
                &rule,
                grant_id,
                7,
                second.clone(),
                &second_fp,
                None,
                None,
                now + std::time::Duration::from_secs(60),
                now,
            )
            .unwrap(),
        DarkWindowSchedule::SuppressedAtCap,
        "the single slot is taken"
    );

    // The owner answers the outstanding exception.
    store
        .resolve_pending_action(&pending_id, false, now)
        .unwrap();

    assert!(matches!(
        store
            .schedule_standing_rule_dark_window(
                &rule,
                grant_id,
                7,
                second,
                &second_fp,
                None,
                None,
                now + std::time::Duration::from_secs(60),
                now,
            )
            .unwrap(),
        DarkWindowSchedule::Scheduled(_)
    ));
    assert_eq!(open_pending_count(&store, "rule-cap-free"), 1);
}

/// Concurrent callers racing for the final slot: exactly one wins.
///
/// This proves the invariant, not the mechanism. One in-process `Store` holds
/// a single `Mutex<Connection>`, so these threads serialize on that mutex and
/// the test would also pass under `Deferred`. The cross-process and
/// cross-connection guarantee comes from `BEGIN IMMEDIATE` taking the write
/// lock at the first statement — the same serialization D-050 relies on for
/// the final unit of quota — and is not what is exercised here.
#[test]
fn concurrent_requests_cannot_cross_the_final_slot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    let store = std::sync::Arc::new(Store::open(&path).unwrap());
    let now = Timestamp::from_second(1_000_000).unwrap();
    let m = manifest(
        "rule-cap-race",
        ACTION,
        7 * 24 * 3600,
        BudgetWindow {
            max: 1,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 1,
            window_secs: 3600,
        },
        Some(DarkWindowConfig {
            timeout_secs: 60,
            default: DarkWindowDefault::Allow,
            max_pending_exceptions: 1,
        }),
    );
    crate::store::standing_rules_tests::activate_or_install_legacy_allow(&store, &m, now);
    let rule = rule(&store, now);
    let action = ActionId::new(ACTION);

    let winners = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut handles = Vec::new();
    for i in 0..8u8 {
        let store = store.clone();
        let rule = rule.clone();
        let action = action.clone();
        let winners = winners.clone();
        handles.push(std::thread::spawn(move || {
            let grant_id = Ulid::new();
            let payload_ref = payload(i);
            let fingerprint = standing_rule_fingerprint(&action, grant_id, 7, &payload_ref);
            let outcome = store
                .schedule_standing_rule_dark_window(
                    &rule,
                    grant_id,
                    7,
                    payload_ref,
                    &fingerprint,
                    None,
                    None,
                    now + std::time::Duration::from_secs(60),
                    now,
                )
                .unwrap();
            if matches!(outcome, DarkWindowSchedule::Scheduled(_)) {
                winners.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(
        winners.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "exactly one concurrent caller took the final slot"
    );
    assert_eq!(open_pending_count(&store, "rule-cap-race"), 1);
    assert_eq!(timer_count(&store), 1);
}

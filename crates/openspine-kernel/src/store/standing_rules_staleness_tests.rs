//! Lifecycle staleness and Allow-eligibility tests (#135).
//!
//! `claim_standing_rule_dark_window` has always treated `stale` as terminal;
//! nothing ever wrote it. These pin the writers that now do, and the
//! activation refusal that gives the `responsibility-contract` prohibition on
//! communication dark-window Allow its first enforcing code.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::standing_rule::{BudgetWindow, DarkWindowConfig, DarkWindowDefault};
use rusqlite::params;
use ulid::Ulid;

use super::standing_rules::standing_rule_fingerprint;
use super::standing_rules_tests::manifest;
use super::Store;

pub(super) const ACTION: &str = "reminder.create";

pub(super) fn rule_manifest(
    rule_id: &str,
    action: &str,
    default: DarkWindowDefault,
) -> openspine_schemas::standing_rule::StandingRuleManifest {
    manifest(
        rule_id,
        action,
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
            default,
            max_pending_exceptions: 1,
        }),
    )
}

/// Schedule one exception and return `(store, pending_id, timer_id)`.
pub(super) fn scheduled_exception(rule_id: &str, now: Timestamp) -> (Store, String, String) {
    scheduled_exception_with_default(rule_id, now, DarkWindowDefault::Allow)
}

fn scheduled_exception_with_default(
    rule_id: &str,
    now: Timestamp,
    default: DarkWindowDefault,
) -> (Store, String, String) {
    let store = Store::open_in_memory().unwrap();
    crate::store::standing_rules_tests::activate_or_install_legacy_allow(
        &store,
        &rule_manifest(rule_id, ACTION, default),
        now,
    );
    let rule = store
        .active_standing_rule_for_action(&ActionId::new(ACTION), now)
        .unwrap()
        .expect("active rule");
    let grant_id = Ulid::new();
    let payload_ref = Some(ArtifactRef {
        digest: digest_of_bytes(b"payload"),
        schema_version: 1,
    });
    let fingerprint = standing_rule_fingerprint(&ActionId::new(ACTION), grant_id, 7, &payload_ref);
    let timer_id = store
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
        .unwrap()
        .timer_id()
        .expect("scheduled")
        .to_string();
    let pending_id: String = store
        .conn
        .lock()
        .query_row(
            "SELECT pending_id FROM standing_rule_pending_actions WHERE request_fingerprint = ?1",
            params![fingerprint],
            |row| row.get(0),
        )
        .unwrap();
    (store, pending_id, timer_id)
}

pub(super) fn resolution(store: &Store, pending_id: &str) -> Option<String> {
    store
        .conn
        .lock()
        .query_row(
            "SELECT resolution FROM standing_rule_pending_actions WHERE pending_id = ?1",
            params![pending_id],
            |row| row.get(0),
        )
        .unwrap()
}

/// Revoking a rule must leave nothing fireable behind: the open exception is
/// staled in the revoke's own transaction, and the timer grants no authority.
#[test]
fn revocation_stales_open_exceptions_before_the_timer_can_fire() {
    let now = Timestamp::from_second(1_000_000).unwrap();
    let (store, pending_id, timer_id) = scheduled_exception("rule-stale-revoke", now);
    assert_eq!(resolution(&store, &pending_id), None, "open before revoke");

    store
        .revoke_standing_rule("rule-stale-revoke", now)
        .unwrap();

    assert_eq!(
        resolution(&store, &pending_id).as_deref(),
        Some("stale"),
        "revocation stales the open exception"
    );
    let claimed = store
        .claim_standing_rule_dark_window(&timer_id, now + std::time::Duration::from_secs(60))
        .unwrap();
    assert!(
        claimed.is_none(),
        "a staled exception grants no authority when its timer fires"
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.pending_exceptions_staled")
            .unwrap(),
        1
    );
}

/// Activating a higher version must not leave the prior version's reviewed
/// exceptions fireable against the new rule.
#[test]
fn a_superseded_version_leaves_nothing_fireable() {
    let now = Timestamp::from_second(1_000_000).unwrap();
    // A Deny default so BOTH activations run through the real production
    // `activate_standing_rule` path: the staleness writer this pins lives in
    // the activation transaction, and an Allow rule can no longer reach it.
    // Staleness on a version bump does not depend on which default the rule
    // carries — a Deny exception occupies a cap slot exactly as an Allow one
    // does.
    let (store, pending_id, timer_id) =
        scheduled_exception_with_default("rule-stale-bump", now, DarkWindowDefault::Deny);

    let mut v2 = rule_manifest("rule-stale-bump", ACTION, DarkWindowDefault::Deny);
    v2.version = 2;
    store.activate_standing_rule(&v2, None, now).unwrap();

    assert_eq!(
        resolution(&store, &pending_id).as_deref(),
        Some("stale"),
        "the prior version's exception is staled by the version bump"
    );
    assert!(store
        .claim_standing_rule_dark_window(&timer_id, now + std::time::Duration::from_secs(60))
        .unwrap()
        .is_none());
}

/// A lapsed rule must not leave a fireable exception. Expiry moves the rule to
/// `needs_review` and stales its open exceptions in the same transaction.
#[test]
fn expiry_stales_open_exceptions() {
    let now = Timestamp::from_second(1_000_000).unwrap();
    let (store, pending_id, timer_id) = scheduled_exception("rule-stale-expiry", now);

    // Past the lapse-after-unused deadline.
    let later = now + std::time::Duration::from_secs(8 * 24 * 3600);
    assert!(
        store
            .active_standing_rule_for_action(&ActionId::new(ACTION), later)
            .unwrap()
            .is_none(),
        "the rule has lapsed"
    );

    assert_eq!(
        resolution(&store, &pending_id).as_deref(),
        Some("stale"),
        "lapsing stales the open exception"
    );
    assert!(store
        .claim_standing_rule_dark_window(&timer_id, later)
        .unwrap()
        .is_none());
}

/// A staled exception is resolved, so it no longer occupies a cap slot. A
/// re-activated rule version starts with its own allowance.
#[test]
fn a_staled_exception_frees_its_slot() {
    let now = Timestamp::from_second(1_000_000).unwrap();
    let (store, _pending_id, _timer) = scheduled_exception("rule-stale-slot", now);
    let outstanding: i64 = store
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM standing_rule_pending_actions WHERE resolved_at IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(outstanding, 1);

    store.revoke_standing_rule("rule-stale-slot", now).unwrap();

    let outstanding: i64 = store
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM standing_rule_pending_actions WHERE resolved_at IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        outstanding, 0,
        "a revoked rule's exceptions are not outstanding"
    );
}

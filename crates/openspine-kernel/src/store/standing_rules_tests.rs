//! Scenario tests for the standing-rule artifact class: manifest window
//! validation, expiry, and the atomic gate-time budget consultation/
//! reservation surface (AD-106) — including the version-atomicity guarantee
//! that protects an in-flight reservation from a later activation swapping
//! the bound action underneath it (P1-4). Dark-window pending-action
//! scenarios live in `standing_rule_scheduling_tests.rs` to keep every file
//! under the 500-line gate.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::standing_rule::{
    BudgetWindow, DarkWindowConfig, DarkWindowDefault, StandingRuleManifest,
};
use rusqlite::{params, OptionalExtension};

use super::Store;

/// Build a standing-rule manifest with the given shape. `expires_after_secs`
/// drives the AD-010 lapse-on-unused expiry; `quota`/`rate` are the AD-106
/// volume/velocity windows; `dark_window` is the optional AD-012 leaning
/// conditional default.
pub(crate) fn manifest(
    id: &str,
    action: &str,
    expires_after_secs: i64,
    quota: BudgetWindow,
    rate: BudgetWindow,
    dark_window: Option<DarkWindowConfig>,
) -> StandingRuleManifest {
    StandingRuleManifest {
        id: id.to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        action_id: ActionId::new(action),
        description: format!("standing rule {id} for {action}"),
        quota,
        rate,
        expires_after_secs,
        dark_window,
    }
}

/// Identity `(rule_id, version)` a reservation row currently carries, or
/// `None` if it was cancelled/finalized away. Same-crate white-box lookup
/// (`Store::conn` is `pub(crate)`) — no public getter exposes a raw
/// reservation row, and this is exactly what the version-atomicity guarantee
/// needs to inspect directly.
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

#[test]
fn standing_rule_lapses_after_expiry_unused() {
    // AD-010 expiry: a rule that is never used lapses on its own once `now`
    // passes `activated_at + expires_after_secs`.
    let store = Store::open_in_memory().unwrap();
    let m = manifest(
        "rule-expiry",
        "appointment.book",
        3600, // 1 hour
        BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 1,
            window_secs: 3600,
        },
        None,
    );
    let activated = Timestamp::from_second(1_000_000).unwrap();
    store.activate_standing_rule(&m, None, activated).unwrap();

    // Within the window: still live.
    let live = store
        .active_standing_rule_for_action(&ActionId::new("appointment.book"), activated)
        .unwrap();
    assert!(live.is_some(), "rule should be live before expiry");

    // Well past expiry, never used: lapses and is no longer consultable.
    let after = activated + std::time::Duration::from_secs(3600 + 1);
    let lapsed = store
        .active_standing_rule_for_action(&ActionId::new("appointment.book"), after)
        .unwrap();
    assert!(
        lapsed.is_none(),
        "rule should lapse after expires_after_secs of disuse"
    );
}

#[test]
fn manifest_validate_rejects_non_positive_windows() {
    // P1: a non-positive window silently admits every request (the trailing
    // `now - window_secs*1e9` cutoff lands at or after `now`), and a
    // non-positive dark-window timeout collapses the owner-response window
    // into an immediately-due path. `validate()` must reject each case and
    // accept the otherwise-identical positive baseline.
    let base = manifest(
        "rule-validate",
        "digest.send",
        3600,
        BudgetWindow {
            max: 1,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 1,
            window_secs: 60,
        },
        Some(DarkWindowConfig {
            timeout_secs: 1800,
            default: DarkWindowDefault::Allow,
        }),
    );
    assert!(base.validate().is_ok(), "baseline manifest must be valid");

    let mut bad_quota_zero = base.clone();
    bad_quota_zero.quota.window_secs = 0;
    assert!(bad_quota_zero.validate().is_err());

    let mut bad_quota_negative = base.clone();
    bad_quota_negative.quota.window_secs = -1;
    assert!(bad_quota_negative.validate().is_err());

    let mut bad_rate = base.clone();
    bad_rate.rate.window_secs = 0;
    assert!(bad_rate.validate().is_err());
    let mut bad_rate_negative = base.clone();
    bad_rate_negative.rate.window_secs = -1;
    assert!(bad_rate_negative.validate().is_err());

    let mut bad_expiry_negative = base.clone();
    bad_expiry_negative.expires_after_secs = -1;
    assert!(bad_expiry_negative.validate().is_err());

    let mut bad_dark_window_negative = base.clone();
    bad_dark_window_negative.dark_window = Some(DarkWindowConfig {
        timeout_secs: -1,
        default: DarkWindowDefault::Allow,
    });
    assert!(bad_dark_window_negative.validate().is_err());

    let mut bad_expiry = base.clone();
    bad_expiry.expires_after_secs = 0;
    assert!(bad_expiry.validate().is_err());

    let mut bad_dark_window = base.clone();
    bad_dark_window.dark_window = Some(DarkWindowConfig {
        timeout_secs: 0,
        default: DarkWindowDefault::Allow,
    });
    assert!(bad_dark_window.validate().is_err());
}

#[test]
fn consult_and_reserve_atomic_budget_saturates_after_max_uses() {
    // AD-106 quota, D-050 atomicity: `consult_and_reserve_standing_rule`
    // reserves headroom inside the same transaction as the lookup. Under
    // budget it returns `Some((rule, Some(reservation_id)))`; once `max`
    // uses are committed it still matches the rule (`Some`) but grants no
    // reservation (`None`) — saturation is distinct from "no rule matched".
    let store = Store::open_in_memory().unwrap();
    let m = manifest(
        "rule-atomic-quota",
        "digest.send",
        7 * 24 * 3600,
        BudgetWindow {
            max: 2,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 100,
            window_secs: 3600,
        },
        None,
    );
    let now = Timestamp::from_second(1_000_000).unwrap();
    store.activate_standing_rule(&m, None, now).unwrap();
    let action = ActionId::new("digest.send");

    let (rule1, res1) = store
        .consult_and_reserve_standing_rule(&action, now)
        .unwrap()
        .expect("rule matched");
    let res1 = res1.expect("headroom available on first use");
    assert_eq!(rule1.rule_id, "rule-atomic-quota");
    assert!(store
        .finalize_standing_rule_reservation(&rule1.rule_id, rule1.version, &res1, now)
        .unwrap());

    let (rule2, res2) = store
        .consult_and_reserve_standing_rule(&action, now)
        .unwrap()
        .expect("rule matched");
    let res2 = res2.expect("headroom available on second use (max = 2)");
    assert!(store
        .finalize_standing_rule_reservation(&rule2.rule_id, rule2.version, &res2, now)
        .unwrap());

    // Third consult: two committed uses saturate quota_max=2 — the rule
    // still matches by action_id, but no reservation is granted.
    let (rule3, res3) = store
        .consult_and_reserve_standing_rule(&action, now)
        .unwrap()
        .expect("rule still matches by action_id");
    assert_eq!(rule3.rule_id, "rule-atomic-quota");
    assert!(
        res3.is_none(),
        "quota saturated at max — no reservation granted"
    );

    let (remaining_quota, _remaining_rate) = store
        .standing_rule_remaining("rule-atomic-quota", now)
        .unwrap();
    assert_eq!(
        remaining_quota, 0,
        "no quota remaining after two committed uses"
    );
}

#[test]
fn consult_and_reserve_cancel_leaves_headroom_unchanged() {
    // P1-6 / AD-106 failed-effects rule: a reservation cancelled because the
    // effect failed (or was denied) must never consume budget — remaining
    // headroom after cancel must equal what it was before the reservation.
    let store = Store::open_in_memory().unwrap();
    let m = manifest(
        "rule-cancel",
        "task.dispatch",
        7 * 24 * 3600,
        BudgetWindow {
            max: 3,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 3,
            window_secs: 3600,
        },
        None,
    );
    let now = Timestamp::from_second(1_000_000).unwrap();
    store.activate_standing_rule(&m, None, now).unwrap();
    let action = ActionId::new("task.dispatch");

    let (before_quota, before_rate) = store.standing_rule_remaining("rule-cancel", now).unwrap();

    let (rule, reservation_id) = store
        .consult_and_reserve_standing_rule(&action, now)
        .unwrap()
        .expect("rule matched");
    let reservation_id = reservation_id.expect("headroom available");

    let (reserved_quota, reserved_rate) =
        store.standing_rule_remaining(&rule.rule_id, now).unwrap();
    assert_eq!(
        reserved_quota,
        before_quota - 1,
        "a live reservation consumes headroom immediately, before finalize"
    );
    assert_eq!(reserved_rate, before_rate - 1);

    store
        .cancel_standing_rule_reservation(&reservation_id)
        .unwrap();

    let (after_quota, after_rate) = store.standing_rule_remaining(&rule.rule_id, now).unwrap();
    assert_eq!(
        after_quota, before_quota,
        "cancel must restore headroom exactly"
    );
    assert_eq!(after_rate, before_rate);
}

#[test]
fn consult_and_reserve_is_atomic_wrt_version_reactivation() {
    // P1-4: a v2 activation that rebinds the artifact to a different action
    // must not let an in-flight v1 reservation be silently charged against
    // v2's budget. `consult_and_reserve_standing_rule` binds the reservation
    // to the exact rule_id/version it looked up inside one transaction; a
    // later `finalize` re-checks that version is still current and fails
    // closed (cancels, never commits) if it is not.
    let store = Store::open_in_memory().unwrap();
    let m1 = manifest(
        "rule-version-swap",
        "invoice.send.v1",
        7 * 24 * 3600,
        BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        None,
    );
    let now = Timestamp::from_second(1_000_000).unwrap();
    store.activate_standing_rule(&m1, None, now).unwrap();

    let (rule_v1, reservation_id) = store
        .consult_and_reserve_standing_rule(&ActionId::new("invoice.send.v1"), now)
        .unwrap()
        .expect("v1 rule matched");
    let reservation_id = reservation_id.expect("headroom available under v1");
    assert_eq!(rule_v1.rule_id, "rule-version-swap");
    assert_eq!(rule_v1.version, 1);
    assert_eq!(rule_v1.action_id, ActionId::new("invoice.send.v1"));

    // The reservation row itself is bound to v1's identity right away.
    assert_eq!(
        reservation_identity(&store, &reservation_id),
        Some(("rule-version-swap".to_string(), 1)),
        "reservation must record the exact (rule_id, version) that was looked up"
    );

    // Re-activate the SAME artifact id at v2, rebound to a DIFFERENT action.
    // `INSERT OR REPLACE` swaps the live `standing_rules` row in place.
    let mut m2 = m1.clone();
    m2.version = 2;
    m2.action_id = ActionId::new("invoice.send.v2");
    store.activate_standing_rule(&m2, None, now).unwrap();

    // The already-consulted rule value is immutable — still v1's identity.
    assert_eq!(rule_v1.rule_id, "rule-version-swap");
    assert_eq!(rule_v1.version, 1);
    assert_eq!(rule_v1.action_id, ActionId::new("invoice.send.v1"));

    // The v2 activation must not have retroactively re-attributed the
    // in-flight reservation to v2 — it still names v1's (rule_id, version).
    assert_eq!(
        reservation_identity(&store, &reservation_id),
        Some(("rule-version-swap".to_string(), 1)),
        "a later activation must never rewrite an in-flight reservation's identity"
    );

    // Finalizing the stale (rule_id, v1) reservation must fail closed: v1 is
    // no longer current, so finalize refuses to commit it under v2's budget.
    let finalized = store
        .finalize_standing_rule_reservation(&rule_v1.rule_id, rule_v1.version, &reservation_id, now)
        .unwrap();
    assert!(
        !finalized,
        "stale v1 reservation must not finalize against the v2 row"
    );
    assert_eq!(
        reservation_identity(&store, &reservation_id),
        None,
        "a refused finalize cancels (deletes) the reserved rows — never commits them"
    );
    assert!(
        !store
            .standing_rule_is_current("rule-version-swap", 1)
            .unwrap(),
        "v1 is no longer the active version after v2 activation"
    );
    assert!(
        store
            .standing_rule_is_current("rule-version-swap", 2)
            .unwrap(),
        "v2 is now current"
    );
}

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

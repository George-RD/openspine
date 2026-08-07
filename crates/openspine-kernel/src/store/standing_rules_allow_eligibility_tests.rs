//! Dark-window `Allow` eligibility tests (#135, D-162): the activation
//! refusal that gives the `responsibility-contract` prohibition its first
//! enforcing code, and the startup sweep that makes it true of stored state
//! rather than only of new activations. Split from
//! `standing_rules_staleness_tests.rs` to keep both files under the 500-line
//! gate.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::standing_rule::DarkWindowDefault;
use rusqlite::params;
use ulid::Ulid;

use super::standing_rules::standing_rule_fingerprint;
use super::standing_rules_staleness_tests::{
    resolution, rule_manifest, scheduled_exception, ACTION,
};
use super::Store;

/// The `responsibility-contract` prohibition gains enforcing code: activation
/// refuses an Allow default for any action absent from the (empty) eligibility
/// allowlist, leaves no active rule row, and records durable evidence.
#[test]
fn activation_refuses_an_allow_default_for_an_ineligible_action() {
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::now();
    let refused = store
        .activate_standing_rule(
            &rule_manifest("rule-allow-send", "email.send", DarkWindowDefault::Allow),
            None,
            now,
        )
        .expect_err("a communication Allow default must not activate");
    assert!(
        format!("{refused}").contains("dark-window Allow default"),
        "unexpected error: {refused}"
    );
    assert!(
        store
            .active_standing_rule_for_action(&ActionId::new("email.send"), now)
            .unwrap()
            .is_none(),
        "a refused activation leaves no active rule row"
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.activation_refused")
            .unwrap(),
        1,
        "the refusal leaves durable owner-actionable evidence"
    );
}

/// The reviewable owner-account write is refused on the same terms — it is
/// absent from the allowlist like everything else, and its descriptor
/// independently declares `DarkWindowPolicy::Prohibited`.
#[test]
fn activation_refuses_an_allow_default_for_the_reviewable_write() {
    let store = Store::open_in_memory().unwrap();
    assert!(store
        .activate_standing_rule(
            &rule_manifest(
                "rule-allow-draft",
                "email.create_draft",
                DarkWindowDefault::Allow
            ),
            None,
            Timestamp::now(),
        )
        .is_err());
}

/// A Deny default stays permitted for the same action: this change forbids
/// silence-admitted Allow, not dark windows.
#[test]
fn a_deny_default_stays_permitted() {
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::now();
    store
        .activate_standing_rule(
            &rule_manifest("rule-deny-send", "email.send", DarkWindowDefault::Deny),
            None,
            now,
        )
        .expect("a deny default is not an amplification risk");
    assert!(store
        .active_standing_rule_for_action(&ActionId::new("email.send"), now)
        .unwrap()
        .is_some());
}

/// The reviewed allowance is validated: zero would be a silently dead policy,
/// and an unbounded one is the amplification the cap exists to stop.
#[test]
fn the_reviewed_allowance_is_validated() {
    let mut zero = rule_manifest("rule-zero", ACTION, DarkWindowDefault::Deny);
    zero.dark_window.as_mut().unwrap().max_pending_exceptions = 0;
    assert!(zero.validate().is_err());

    let mut huge = rule_manifest("rule-huge", ACTION, DarkWindowDefault::Deny);
    huge.dark_window.as_mut().unwrap().max_pending_exceptions = 99;
    assert!(huge.validate().is_err());

    let ok = rule_manifest("rule-ok", ACTION, DarkWindowDefault::Deny);
    assert_eq!(
        ok.dark_window.unwrap().max_pending_exceptions,
        1,
        "the safe default is one"
    );
    assert!(ok.validate().is_ok());
}

/// D-160: a fired token whose reviewed context has drifted grants no
/// authority. The exception was minted against one reviewed scope; consuming
/// it under a different one fails closed, even though the rule row is still
/// active at that version and the request fingerprint still matches.
#[test]
fn a_drifted_context_cannot_spend_a_pre_drift_waiver() {
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::from_second(1_000_000).unwrap();
    crate::store::standing_rules_tests::activate_or_install_legacy_allow(
        &store,
        &rule_manifest("rule-ctx-bound", ACTION, DarkWindowDefault::Allow),
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
            payload_ref.clone(),
            &fingerprint,
            Some("sha256:reviewed-scope"),
            Some("sha256:epoch"),
            now + std::time::Duration::from_secs(60),
            now,
        )
        .unwrap()
        .timer_id()
        .expect("scheduled")
        .to_string();
    let fired = now + std::time::Duration::from_secs(60);
    assert!(
        store
            .claim_standing_rule_dark_window(&timer_id, fired)
            .unwrap()
            .is_some(),
        "the allow default fires"
    );
    let pending_id: String = store
        .conn
        .lock()
        .query_row(
            "SELECT pending_id FROM standing_rule_pending_actions WHERE request_fingerprint = ?1",
            params![fingerprint],
            |row| row.get(0),
        )
        .unwrap();

    // The reviewed scope has drifted since the exception was minted.
    assert!(
        store
            .consume_fired_pending_for_context(
                &pending_id,
                &ActionId::new(ACTION),
                grant_id,
                7,
                &payload_ref,
                Some("sha256:different-scope"),
                Some("sha256:epoch"),
                fired,
            )
            .unwrap()
            .is_none(),
        "a drifted reviewed scope grants no authority"
    );
    // A drifted compatibility epoch is refused on the same terms.
    assert!(store
        .consume_fired_pending_for_context(
            &pending_id,
            &ActionId::new(ACTION),
            grant_id,
            7,
            &payload_ref,
            Some("sha256:reviewed-scope"),
            Some("sha256:different-epoch"),
            fired,
        )
        .unwrap()
        .is_none());
    // The reviewed context still admits it, and exactly once.
    assert!(store
        .consume_fired_pending_for_context(
            &pending_id,
            &ActionId::new(ACTION),
            grant_id,
            7,
            &payload_ref,
            Some("sha256:reviewed-scope"),
            Some("sha256:epoch"),
            fired,
        )
        .unwrap()
        .is_some());
    assert!(
        store
            .consume_fired_pending_for_context(
                &pending_id,
                &ActionId::new(ACTION),
                grant_id,
                7,
                &payload_ref,
                Some("sha256:reviewed-scope"),
                Some("sha256:epoch"),
                fired,
            )
            .unwrap()
            .is_none(),
        "the exception allowance is one-use"
    );
}

/// D-161: a fired exception is audited under its own class, and finalizing it
/// does not refresh the lapse-after-unused clock — owner silence must not keep
/// alive the rule that clock exists to retire.
#[test]
fn a_fired_exception_is_audited_distinctly_and_does_not_extend_the_lapse_clock() {
    let now = Timestamp::from_second(1_000_000).unwrap();
    let (store, pending_id, timer_id) = scheduled_exception("rule-exception-audit", now);
    let fired = now + std::time::Duration::from_secs(60);
    store
        .claim_standing_rule_dark_window(&timer_id, fired)
        .unwrap()
        .expect("allow default fires");
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.exception_fired")
            .unwrap(),
        1,
        "the fire is recorded under the exception audit class, not as ordinary admission"
    );

    let last_used_before: Option<i64> = store
        .conn
        .lock()
        .query_row(
            "SELECT last_used_at FROM standing_rules WHERE rule_id = 'rule-exception-audit'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        last_used_before.is_none(),
        "the rule has no ordinary use yet"
    );

    store
        .finalize_standing_rule_exception_reservation("rule-exception-audit", 1, &pending_id, fired)
        .unwrap();

    let last_used_after: Option<i64> = store
        .conn
        .lock()
        .query_row(
            "SELECT last_used_at FROM standing_rules WHERE rule_id = 'rule-exception-audit'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        last_used_after.is_none(),
        "a fired exception must not refresh the lapse-after-unused clock"
    );
}

/// MAJOR 3: the activation guard is not retroactive, so a rule stored before
/// it existed must be retired when the database is next opened. Hydration
/// re-reads `dark_window_default` without re-checking eligibility, so without
/// this sweep such a rule stays active and stays fireable.
#[test]
fn opening_a_database_retires_a_stored_ineligible_allow_rule() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    let now = Timestamp::from_second(1_000_000).unwrap();
    let pending_id = {
        let store = Store::open(&path).unwrap();
        // A pre-guard row: active, Allow, for an action the catalog does not
        // declare eligible, with one exception already outstanding.
        crate::store::standing_rules_tests::install_legacy_allow_dark_window_rule(
            &store,
            &rule_manifest("rule-legacy-allow", "email.send", DarkWindowDefault::Allow),
            now,
        );
        let rule = store
            .active_standing_rule_for_action(&ActionId::new("email.send"), now)
            .unwrap()
            .expect("the legacy row is active before the sweep");
        let grant_id = Ulid::new();
        let payload_ref = Some(ArtifactRef {
            digest: digest_of_bytes(b"payload"),
            schema_version: 1,
        });
        let fingerprint =
            standing_rule_fingerprint(&ActionId::new("email.send"), grant_id, 7, &payload_ref);
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
            .unwrap();
        let pending_id: String = store
            .conn
            .lock()
            .query_row(
                "SELECT pending_id FROM standing_rule_pending_actions WHERE rule_id = ?1",
                params!["rule-legacy-allow"],
                |row| row.get(0),
            )
            .unwrap();
        pending_id
    };

    // Re-open: the startup sweep converges the stored state.
    let store = Store::open(&path).unwrap();
    assert!(
        store
            .active_standing_rule_for_action(&ActionId::new("email.send"), now)
            .unwrap()
            .is_none(),
        "a stored ineligible Allow rule is no longer live after the sweep"
    );
    let status: String = store
        .conn
        .lock()
        .query_row(
            "SELECT status FROM standing_rules WHERE rule_id = 'rule-legacy-allow'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "needs_review");
    assert_eq!(
        resolution(&store, &pending_id).as_deref(),
        Some("stale"),
        "its outstanding exception is staled in the same transaction"
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.ineligible_allow_retired")
            .unwrap(),
        1,
        "the retirement leaves durable owner-actionable evidence"
    );

    // Idempotent: the sweep's `WHERE status = 'active'` selector means a third
    // open finds nothing to retire and writes no second audit row.
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.ineligible_allow_retired")
            .unwrap(),
        1,
        "re-opening does not re-retire an already-retired rule"
    );
}

/// The sweep must not disturb a rule that is still legitimate: a Deny
/// dark-window rule survives every open untouched.
#[test]
fn the_sweep_leaves_a_deny_rule_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    let now = Timestamp::now();
    {
        let store = Store::open(&path).unwrap();
        store
            .activate_standing_rule(
                &rule_manifest("rule-deny-survives", "email.send", DarkWindowDefault::Deny),
                None,
                now,
            )
            .unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert!(store
        .active_standing_rule_for_action(&ActionId::new("email.send"), now)
        .unwrap()
        .is_some());
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.ineligible_allow_retired")
            .unwrap(),
        0
    );
}

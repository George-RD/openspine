// openspine:allow-large-module reason: standing-rule scheduling and dark-window regression tests (AD-012, D-073, P1-P3). Multiple independent test functions for scheduling, claim, consume, recovery, deadline, and version-partitioning scenarios.
//! Dark-window scheduling and pending-action regression tests (AD-012).

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
use crate::standing_rules_gate::consult_standing_rule_gate;

fn pending_id_for(store: &Store, rule_id: &str, fingerprint: &str) -> String {
    store
        .conn
        .lock()
        .query_row(
            "SELECT pending_id FROM standing_rule_pending_actions \
             WHERE rule_id = ?1 AND request_fingerprint = ?2",
            params![rule_id, fingerprint],
            |row| row.get(0),
        )
        .unwrap()
}

fn active_rule(store: &Store, action: &str, now: Timestamp) -> super::standing_rules::StandingRule {
    store
        .active_standing_rule_for_action(&ActionId::new(action), now)
        .unwrap()
        .expect("active standing rule")
}

fn payload() -> Option<ArtifactRef> {
    Some(ArtifactRef {
        digest: digest_of_bytes(b"encrypted action payload"),
        schema_version: 1,
    })
}

#[test]
fn deny_default_never_dispatches_and_is_terminal() {
    let store = Store::open_in_memory().unwrap();
    let manifest = manifest(
        "rule-dw-deny",
        "reminder.create",
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
            default: DarkWindowDefault::Deny,
        }),
    );
    let now = Timestamp::from_second(1_000_000).unwrap();
    store.activate_standing_rule(&manifest, None, now).unwrap();
    let rule = active_rule(&store, "reminder.create", now);
    let grant_id = Ulid::new();
    let bound_chat_id = 7;
    let payload_ref = payload();
    let fingerprint =
        standing_rule_fingerprint(&rule.action_id, grant_id, bound_chat_id, &payload_ref);

    let timer_id = store
        .schedule_standing_rule_dark_window(
            &rule,
            grant_id,
            bound_chat_id,
            payload_ref,
            &fingerprint,
            now + std::time::Duration::from_secs(60),
            now,
        )
        .unwrap()
        .expect("new timer inserted");
    assert!(
        store
            .claim_standing_rule_dark_window(&timer_id, now + std::time::Duration::from_secs(60))
            .unwrap()
            .is_none(),
        "deny default has no dispatch authority"
    );
    let pending_id = pending_id_for(&store, &rule.rule_id, &fingerprint);
    let resolution: Option<String> = store
        .conn
        .lock()
        .query_row(
            "SELECT resolution FROM standing_rule_pending_actions WHERE pending_id = ?1",
            params![pending_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(resolution.as_deref(), Some("denied"));
}

#[test]
fn fired_allow_token_is_digest_bound_and_one_use() {
    let store = Store::open_in_memory().unwrap();
    let manifest = manifest(
        "rule-dw-allow",
        "message.send",
        7 * 24 * 3600,
        BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        Some(DarkWindowConfig {
            timeout_secs: 60,
            default: DarkWindowDefault::Allow,
        }),
    );
    let now = Timestamp::from_second(1_000_000).unwrap();
    store.activate_standing_rule(&manifest, None, now).unwrap();
    let rule = active_rule(&store, "message.send", now);
    let grant_id = Ulid::new();
    let chat = 9;
    let payload_ref = payload();
    let fingerprint = standing_rule_fingerprint(&rule.action_id, grant_id, chat, &payload_ref);
    let timer_id = store
        .schedule_standing_rule_dark_window(
            &rule,
            grant_id,
            chat,
            payload_ref.clone(),
            &fingerprint,
            now + std::time::Duration::from_secs(60),
            now,
        )
        .unwrap()
        .expect("timer inserted");
    let pending = store
        .claim_standing_rule_dark_window(&timer_id, now + std::time::Duration::from_secs(60))
        .unwrap()
        .expect("allow default is dispatchable");
    assert_eq!(pending.resolution.as_deref(), Some("allowed"));
    let result = store
        .consume_standing_rule_fired_pending(
            &pending.pending_id,
            &rule.action_id,
            grant_id,
            chat,
            &payload_ref,
            now + std::time::Duration::from_secs(61),
        )
        .unwrap();
    assert_eq!(
        result,
        Some((
            rule.rule_id.clone(),
            rule.version,
            pending.pending_id.clone()
        ))
    );
    // Effective Allow admission is audited before any effect (D-073): the
    // one-use token consume appends `standing_rule.dark_window_admitted`.
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.dark_window_admitted")
            .unwrap(),
        1
    );
    // The pending payload is persisted only as an `ArtifactRef` (a digest
    // reference), never as inline plaintext.
    let stored: String = store
        .conn
        .lock()
        .query_row(
            "SELECT payload_ref_json FROM standing_rule_pending_actions WHERE pending_id = ?1",
            params![pending.pending_id],
            |r| r.get(0),
        )
        .unwrap();
    let parsed: ArtifactRef =
        serde_json::from_str(&stored).expect("payload_ref_json must be an ArtifactRef");
    assert_eq!(parsed.digest, payload_ref.as_ref().unwrap().digest);
    assert!(
        store
            .consume_standing_rule_fired_pending(
                &pending.pending_id,
                &rule.action_id,
                grant_id,
                chat,
                &payload_ref,
                now + std::time::Duration::from_secs(62),
            )
            .unwrap()
            .is_none(),
        "fired token is one-use"
    );
}

#[test]
fn fired_allow_token_rejects_different_fingerprint() {
    let store = Store::open_in_memory().unwrap();
    let manifest = manifest(
        "rule-dw-fingerprint",
        "message.send",
        7 * 24 * 3600,
        BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        Some(DarkWindowConfig {
            timeout_secs: 60,
            default: DarkWindowDefault::Allow,
        }),
    );
    let now = Timestamp::from_second(1_000_000).unwrap();
    store.activate_standing_rule(&manifest, None, now).unwrap();
    let rule = active_rule(&store, "message.send", now);
    let grant_id = Ulid::new();
    let chat = 10;
    let original_payload = payload();
    let fingerprint = standing_rule_fingerprint(&rule.action_id, grant_id, chat, &original_payload);
    let timer_id = store
        .schedule_standing_rule_dark_window(
            &rule,
            grant_id,
            chat,
            original_payload,
            &fingerprint,
            now + std::time::Duration::from_secs(60),
            now,
        )
        .unwrap()
        .expect("timer inserted");
    let pending = store
        .claim_standing_rule_dark_window(&timer_id, now + std::time::Duration::from_secs(60))
        .unwrap()
        .expect("allow default is dispatchable");
    let different_payload = Some(ArtifactRef {
        digest: digest_of_bytes(b"different payload"),
        schema_version: 1,
    });
    assert!(
        store
            .consume_standing_rule_fired_pending(
                &pending.pending_id,
                &rule.action_id,
                grant_id,
                chat,
                &different_payload,
                now + std::time::Duration::from_secs(61),
            )
            .unwrap()
            .is_none(),
        "token must be bound to the exact payload fingerprint"
    );
}

#[test]
fn scheduling_is_idempotent_across_terminal_resolution() {
    let store = Store::open_in_memory().unwrap();
    let manifest = manifest(
        "rule-dw-idem",
        "reminder.create",
        7 * 24 * 3600,
        BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        Some(DarkWindowConfig {
            timeout_secs: 60,
            default: DarkWindowDefault::Deny,
        }),
    );
    let now = Timestamp::from_second(1_000_000).unwrap();
    store.activate_standing_rule(&manifest, None, now).unwrap();
    let rule = active_rule(&store, "reminder.create", now);
    let grant_id = Ulid::new();
    let chat = 11;
    let payload_ref = payload();
    let fingerprint = standing_rule_fingerprint(&rule.action_id, grant_id, chat, &payload_ref);
    let first = store
        .schedule_standing_rule_dark_window(
            &rule,
            grant_id,
            chat,
            payload_ref.clone(),
            &fingerprint,
            now + std::time::Duration::from_secs(60),
            now,
        )
        .unwrap();
    assert!(first.is_some());
    let second = store
        .schedule_standing_rule_dark_window(
            &rule,
            grant_id,
            chat,
            payload_ref,
            &fingerprint,
            now + std::time::Duration::from_secs(60),
            now,
        )
        .unwrap();
    assert!(
        second.is_none(),
        "duplicate live timer must not be inserted"
    );
    let pending_id = pending_id_for(&store, &rule.rule_id, &fingerprint);
    store
        .claim_standing_rule_dark_window(&first.unwrap(), now + std::time::Duration::from_secs(60))
        .unwrap();
    let third = store
        .schedule_standing_rule_dark_window(
            &rule,
            grant_id,
            chat,
            None,
            &fingerprint,
            now + std::time::Duration::from_secs(120),
            now,
        )
        .unwrap();
    assert!(
        third.is_none(),
        "resolved terminal request must not reschedule"
    );
    assert!(!pending_id.is_empty());
}

#[test]
fn owner_resolution_before_fire_controls_claim() {
    let store = Store::open_in_memory().unwrap();
    let manifest = manifest(
        "rule-owner-resolve",
        "owner.reply",
        7 * 24 * 3600,
        BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        Some(DarkWindowConfig {
            timeout_secs: 60,
            default: DarkWindowDefault::Deny,
        }),
    );
    let now = Timestamp::from_second(1_000_000).unwrap();
    store.activate_standing_rule(&manifest, None, now).unwrap();
    let rule = active_rule(&store, "owner.reply", now);

    let allow_grant = Ulid::new();
    let allow_payload = payload();
    let allow_fingerprint =
        standing_rule_fingerprint(&rule.action_id, allow_grant, 21, &allow_payload);
    let allow_timer = store
        .schedule_standing_rule_dark_window(
            &rule,
            allow_grant,
            21,
            allow_payload,
            &allow_fingerprint,
            now + std::time::Duration::from_secs(60),
            now,
        )
        .unwrap()
        .expect("allow timer inserted");
    let allow_pending_id = pending_id_for(&store, &rule.rule_id, &allow_fingerprint);
    assert!(store
        .resolve_pending_action(&allow_pending_id, true, now)
        .unwrap());
    let allowed = store
        .claim_standing_rule_dark_window(&allow_timer, now + std::time::Duration::from_secs(60))
        .unwrap()
        .expect("owner allow should dispatch");
    assert_eq!(allowed.resolution.as_deref(), Some("allowed"));

    let deny_grant = Ulid::new();
    let deny_payload = payload();
    let deny_fingerprint =
        standing_rule_fingerprint(&rule.action_id, deny_grant, 22, &deny_payload);
    let deny_timer = store
        .schedule_standing_rule_dark_window(
            &rule,
            deny_grant,
            22,
            deny_payload,
            &deny_fingerprint,
            now + std::time::Duration::from_secs(60),
            now,
        )
        .unwrap()
        .expect("deny timer inserted");
    let deny_pending_id = pending_id_for(&store, &rule.rule_id, &deny_fingerprint);
    assert!(store
        .resolve_pending_action(&deny_pending_id, false, now)
        .unwrap());
    assert!(store
        .claim_standing_rule_dark_window(&deny_timer, now + std::time::Duration::from_secs(60))
        .unwrap()
        .is_none());
}

#[test]
fn allowed_pending_is_recoverable_until_consumed() {
    let store = Store::open_in_memory().unwrap();
    let manifest = manifest(
        "rule-dw-recovery",
        "recovery.send",
        7 * 24 * 3600,
        BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        Some(DarkWindowConfig {
            timeout_secs: 60,
            default: DarkWindowDefault::Allow,
        }),
    );
    let now = Timestamp::from_second(1_000_000).unwrap();
    store.activate_standing_rule(&manifest, None, now).unwrap();
    let rule = active_rule(&store, "recovery.send", now);
    let grant_id = Ulid::new();
    let chat = 31;
    let payload_ref = payload();
    let fingerprint = standing_rule_fingerprint(&rule.action_id, grant_id, chat, &payload_ref);
    let timer_id = store
        .schedule_standing_rule_dark_window(
            &rule,
            grant_id,
            chat,
            payload_ref.clone(),
            &fingerprint,
            now + std::time::Duration::from_secs(60),
            now,
        )
        .unwrap()
        .expect("timer inserted");
    let pending = store
        .claim_standing_rule_dark_window(&timer_id, now + std::time::Duration::from_secs(60))
        .unwrap()
        .expect("allow claim");
    let recoverable = store.pending_dark_window_recoverable().unwrap();
    assert!(recoverable
        .iter()
        .any(|p| p.pending_id == pending.pending_id));
    assert!(store
        .consume_standing_rule_fired_pending(
            &pending.pending_id,
            &rule.action_id,
            grant_id,
            chat,
            &payload_ref,
            now + std::time::Duration::from_secs(61),
        )
        .unwrap()
        .is_some());
    assert!(!store
        .pending_dark_window_recoverable()
        .unwrap()
        .iter()
        .any(|p| p.pending_id == pending.pending_id));
}

#[test]
fn claimed_fired_pending_is_surfaced_once_not_redispatched() {
    // P1/D-073: after the one-use token is consumed the row sits in the
    // `claimed` (receiptless) state — the connector effect has NOT run yet. A
    // crash here must NOT silently lose the action: recovery surfaces it for
    // owner attention exactly once and never re-runs the effect.
    let store = Store::open_in_memory().unwrap();
    let manifest = manifest(
        "rule-dw-claimed",
        "claimed.send",
        7 * 24 * 3600,
        BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        Some(DarkWindowConfig {
            timeout_secs: 60,
            default: DarkWindowDefault::Allow,
        }),
    );
    let now = Timestamp::from_second(1_000_000).unwrap();
    store.activate_standing_rule(&manifest, None, now).unwrap();
    let rule = active_rule(&store, "claimed.send", now);
    let grant_id = Ulid::new();
    let chat = 41;
    let payload_ref = payload();
    let fingerprint = standing_rule_fingerprint(&rule.action_id, grant_id, chat, &payload_ref);
    let timer_id = store
        .schedule_standing_rule_dark_window(
            &rule,
            grant_id,
            chat,
            payload_ref.clone(),
            &fingerprint,
            now + std::time::Duration::from_secs(60),
            now,
        )
        .unwrap()
        .expect("timer inserted");
    let pending = store
        .claim_standing_rule_dark_window(&timer_id, now + std::time::Duration::from_secs(60))
        .unwrap()
        .expect("allow claim");
    // Token consumed -> row moves to `claimed` (not `none`, not `dispatched`).
    let consumed = store
        .consume_standing_rule_fired_pending(
            &pending.pending_id,
            &rule.action_id,
            grant_id,
            chat,
            &payload_ref,
            now + std::time::Duration::from_secs(61),
        )
        .unwrap();
    assert!(consumed.is_some());
    // It is no longer in the token-never-consumed (`none`) recoverable set.
    assert!(!store
        .pending_dark_window_recoverable()
        .unwrap()
        .iter()
        .any(|p| p.pending_id == pending.pending_id));
    // It IS the receiptless claimed state recovery must surface.
    assert!(store
        .pending_dark_window_claimed_unredriven()
        .unwrap()
        .iter()
        .any(|p| p.pending_id == pending.pending_id));
    let before = store
        .count_audit_events_of_kind("standing_rule.dark_window_effect_unconfirmed")
        .unwrap();
    assert_eq!(before, 0);
    // Surface once.
    store
        .surface_dark_window_claimed_for_owner(&pending.pending_id, now)
        .unwrap();
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.dark_window_effect_unconfirmed")
            .unwrap(),
        1
    );
    // Idempotent: a repeat surface does not emit a second audit.
    store
        .surface_dark_window_claimed_for_owner(&pending.pending_id, now)
        .unwrap();
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.dark_window_effect_unconfirmed")
            .unwrap(),
        1
    );
    // Already surfaced -> excluded from the next sweep.
    assert!(!store
        .pending_dark_window_claimed_unredriven()
        .unwrap()
        .iter()
        .any(|p| p.pending_id == pending.pending_id));
    // The effect was never auto-run: row is still `claimed`, not `dispatched`.
    let state: String = store
        .conn
        .lock()
        .query_row(
            "SELECT dispatch_state FROM standing_rule_pending_actions WHERE pending_id = ?1",
            params![pending.pending_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "claimed");
    // Only after the connector succeeds does the attempt get durably recorded.
    store
        .mark_fired_effect_attempted(&pending.pending_id, "receipt-digest")
        .unwrap();
    let state: String = store
        .conn
        .lock()
        .query_row(
            "SELECT dispatch_state FROM standing_rule_pending_actions WHERE pending_id = ?1",
            params![pending.pending_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "dispatched");
    assert!(!store
        .pending_dark_window_claimed_unredriven()
        .unwrap()
        .iter()
        .any(|p| p.pending_id == pending.pending_id));
}
#[test]
fn exact_deadline_expiry_boundary_is_uniform() {
    // P3/D-073: a rule lapses the instant `now` reaches the deadline
    // (`deadline <= now`) in BOTH the live lookup path and the atomic consult
    // path — one canonical boundary, no extra-instant authority gap. Two
    // separate stores are used so each path's expiry mutation (sets
    // `needs_review`) cannot cross-contaminate the other's boundary check.
    let expires = 100i64; // seconds
    let make = || {
        let store = Store::open_in_memory().unwrap();
        let manifest = manifest(
            "rule-exact-deadline",
            "deadline.action",
            expires,
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
        let base = Timestamp::from_second(2_000_000).unwrap();
        store.activate_standing_rule(&manifest, None, base).unwrap();
        (store, ActionId::new("deadline.action"), base)
    };
    // Lookup path (its own store).
    let (lookup_store, action, base) = make();
    assert!(lookup_store
        .active_standing_rule_for_action(&action, base)
        .unwrap()
        .is_some());
    let just_before =
        base + std::time::Duration::from_secs(expires as u64) - std::time::Duration::from_nanos(1);
    assert!(lookup_store
        .active_standing_rule_for_action(&action, just_before)
        .unwrap()
        .is_some());
    let at_deadline = base + std::time::Duration::from_secs(expires as u64);
    assert!(lookup_store
        .active_standing_rule_for_action(&action, at_deadline)
        .unwrap()
        .is_none());
    // Consult path (its own store, unaffected by the lookup mutation above).
    let (consult_store, action, base) = make();
    assert!(
        consult_standing_rule_gate(&consult_store, &action, base, None)
            .unwrap()
            .allow
    );
    assert!(
        consult_standing_rule_gate(&consult_store, &action, just_before, None)
            .unwrap()
            .allow
    );
    assert!(
        !consult_standing_rule_gate(&consult_store, &action, at_deadline, None)
            .unwrap()
            .allow
    );
}

#[test]
fn reactivated_version_gets_distinct_pending_timer() {
    // Per `(rule_id, rule_version, request_fingerprint)` uniqueness: a v2
    // reactivation of the same rule with the same stable request identity
    // must get its own pending row and timer, not reuse the v1 row.
    let store = Store::open_in_memory().unwrap();
    let manifest_v1 = manifest(
        "rule-dw-version",
        "versioned.action",
        7 * 24 * 3600,
        BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        Some(DarkWindowConfig {
            timeout_secs: 60,
            default: DarkWindowDefault::Allow,
        }),
    );
    let now = Timestamp::from_second(1_000_000).unwrap();
    store
        .activate_standing_rule(&manifest_v1, None, now)
        .unwrap();
    let rule_v1 = active_rule(&store, "versioned.action", now);
    let grant1 = Ulid::new();
    let chat = 91;
    let payload_ref = payload();
    let fingerprint = standing_rule_fingerprint(&rule_v1.action_id, grant1, chat, &payload_ref);
    let timer1 = store
        .schedule_standing_rule_dark_window(
            &rule_v1,
            grant1,
            chat,
            payload_ref.clone(),
            &fingerprint,
            now + std::time::Duration::from_secs(60),
            now,
        )
        .unwrap()
        .expect("v1 timer inserted");
    let pending1 = pending_id_for(&store, &rule_v1.rule_id, &fingerprint);
    // Reactivate as v2, same rule identity and same request fingerprint.
    let mut manifest_v2 = manifest_v1.clone();
    manifest_v2.version = 2;
    store
        .activate_standing_rule(&manifest_v2, None, now)
        .unwrap();
    let rule_v2 = active_rule(&store, "versioned.action", now);
    assert_eq!(rule_v2.version, 2);
    let timer2 = store
        .schedule_standing_rule_dark_window(
            &rule_v2,
            grant1,
            chat,
            payload_ref,
            &fingerprint,
            now + std::time::Duration::from_secs(60),
            now,
        )
        .unwrap()
        .expect("v2 timer inserted (distinct from v1)");
    assert_ne!(timer1, timer2, "v2 must schedule a distinct timer");
    let pending2: String = store
        .conn
        .lock()
        .query_row(
            "SELECT pending_id FROM standing_rule_pending_actions \
             WHERE rule_id = ?1 AND rule_version = ?2 AND request_fingerprint = ?3",
            params![rule_v2.rule_id, 2i64, fingerprint],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(pending1, pending2, "v2 must get its own pending row");
}

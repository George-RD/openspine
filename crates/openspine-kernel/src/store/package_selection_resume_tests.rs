//! Paused and terminal authority cannot bypass package-epoch re-review.
use super::package_selection_fixtures::{decision, install, select};
use super::{standing_rules_staleness_tests::scheduled_exception, PauseStandingRuleOutcome};
use jiff::Timestamp;

#[test]
fn paused_rule_across_epoch_change_requires_full_review_before_resume() {
    let now = Timestamp::now();
    let (store, _, _) = scheduled_exception("paused-selection", now);
    assert_eq!(
        store.pause_standing_rule("paused-selection", now).unwrap(),
        PauseStandingRuleOutcome::Paused
    );
    let (a, b) = (install(&store, 1), install(&store, 2));
    select(&store, &a, now);
    let mut changing = decision(&store, &b, now);
    changing.base_epoch_changed = true;
    store
        .record_package_selection_decision(&changing, now)
        .unwrap();
    store.commit_package_selection(&changing, now).unwrap();
    let before: (String, Option<i64>) = store.with_deferred_read(|tx| Ok(tx.query_row(
        "SELECT status, needs_review_since FROM standing_rules WHERE rule_id = 'paused-selection'", [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?)).unwrap();
    assert_eq!(before, ("paused".into(), Some(now.as_nanosecond() as i64)));
    assert!(!store.resume_standing_rule("paused-selection", 1).unwrap());
    assert!(!store
        .standing_rule_is_current("paused-selection", 1)
        .unwrap());
    let status: String = store
        .with_deferred_read(|tx| {
            Ok(tx.query_row(
                "SELECT status FROM standing_rules WHERE rule_id = 'paused-selection'",
                [],
                |row| row.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(status, "needs_review");
    let receipt: String = store.with_deferred_read(|tx| Ok(tx.query_row(
        "SELECT meta_json FROM audit_log WHERE kind = 'package.selection_committed' ORDER BY seq DESC LIMIT 1", [], |row| row.get(0),
    )?)).unwrap();
    assert!(receipt.contains("paused-selection"), "{receipt}");
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.resume_requires_review")
            .unwrap(),
        1
    );
    store.validate_package_selection().unwrap();
    assert!(store.verify_audit_chain().unwrap());
}

#[test]
fn revoked_and_expired_paused_rules_cannot_resume_after_selection() {
    for revoked in [false, true] {
        let now = Timestamp::now();
        let (store, _, _) = scheduled_exception("terminal-selection", now);
        store
            .pause_standing_rule("terminal-selection", now)
            .unwrap();
        if revoked {
            store
                .revoke_standing_rule("terminal-selection", now)
                .unwrap();
        } else {
            store.with_conn_for_test(|conn| {
                conn.execute(
                    "UPDATE standing_rules SET activated_at = 0, last_used_at = NULL",
                    [],
                )
                .unwrap();
            });
        }
        let (a, b) = (install(&store, 1), install(&store, 2));
        select(&store, &a, now);
        let mut changing = decision(&store, &b, now);
        changing.base_epoch_changed = true;
        store
            .record_package_selection_decision(&changing, now)
            .unwrap();
        store.commit_package_selection(&changing, now).unwrap();
        assert!(!store.resume_standing_rule("terminal-selection", 1).unwrap());
        assert!(!store
            .standing_rule_is_current("terminal-selection", 1)
            .unwrap());
    }
}

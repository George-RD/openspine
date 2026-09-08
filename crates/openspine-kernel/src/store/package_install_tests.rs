//! Maintenance opening must not run the authority-changing runtime sweep.
use super::{standing_rules_staleness_tests::rule_manifest, Store};
use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::standing_rule::DarkWindowDefault;

#[test]
fn package_maintenance_does_not_sweep_authority_but_runtime_open_still_does() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("kernel.db");
    let now = Timestamp::now();
    let action = ActionId::new("email.send");
    let store = Store::open(&path).unwrap();
    super::standing_rules_tests::install_legacy_allow_dark_window_rule(
        &store,
        &rule_manifest("package-no-sweep", "email.send", DarkWindowDefault::Allow),
        now,
    );
    assert!(store
        .active_standing_rule_for_action(&action, now)
        .unwrap()
        .is_some());
    drop(store);
    let maintenance = Store::open_for_package_management(&path).unwrap();
    assert!(maintenance
        .active_standing_rule_for_action(&action, now)
        .unwrap()
        .is_some());
    assert_eq!(
        maintenance
            .count_audit_events_of_kind("standing_rule.ineligible_allow_retired")
            .unwrap(),
        0
    );
    drop(maintenance);
    let runtime = Store::open(&path).unwrap();
    assert!(runtime
        .active_standing_rule_for_action(&action, now)
        .unwrap()
        .is_none());
    assert_eq!(
        runtime
            .count_audit_events_of_kind("standing_rule.ineligible_allow_retired")
            .unwrap(),
        1
    );
}

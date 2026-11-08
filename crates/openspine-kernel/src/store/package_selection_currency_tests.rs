//! Managed selection closes the "verdict bound no epoch, so it is current" gap.
use super::eval_verdict_store::{EvalVerdict, VerdictEpochs};
use super::package_selection_fixtures::{install, select};
use super::Store;
use jiff::Timestamp;
use ulid::Ulid;

fn verdict(version: u32, compatibility_digest: Option<&str>) -> EvalVerdict {
    EvalVerdict {
        id: Ulid::new(),
        artifact_kind: "standing_rule".into(),
        artifact_id: "rule".into(),
        artifact_version: version,
        verdict: "pass".into(),
        fitness: None,
        evidence: None,
        evaluator: None,
        artifact_digest: format!("sha256:{}", "0".repeat(64)),
        recorded_at: Timestamp::now(),
        epochs: VerdictEpochs {
            compatibility_digest: compatibility_digest.map(str::to_owned),
            ..VerdictEpochs::default()
        },
    }
}

fn live(compatibility_digest: &str) -> VerdictEpochs {
    VerdictEpochs {
        compatibility_digest: Some(compatibility_digest.into()),
        ..VerdictEpochs::default()
    }
}

#[test]
fn an_unbound_verdict_is_current_in_legacy_mode_and_stale_once_managed() {
    let store = Store::open_in_memory().unwrap();
    store.insert_eval_verdict(&verdict(1, None)).unwrap();
    store
        .insert_eval_verdict(&verdict(2, Some("sha256:bound")))
        .unwrap();
    let check = |version| {
        store.reject_stale_eval_verdicts("standing_rule", "rule", version, &live("sha256:bound"))
    };
    check(1).expect("legacy behaviour is unchanged");
    check(2).unwrap();

    select(&store, &install(&store, 1), Timestamp::now());
    let refused =
        check(1).expect_err("an epoch-free verdict cannot show it postdates the transition");
    assert!(
        format!("{refused}").contains("compatibility_digest"),
        "{refused}"
    );
    check(2).expect("a verdict bound to the live compatibility digest stays current");
    assert!(
        store
            .count_audit_events_of_kind("eval_verdict.stale_at_activation")
            .unwrap()
            >= 1
    );
}

#[test]
fn the_in_transaction_activation_check_applies_the_same_managed_rule() {
    let store = Store::open_in_memory().unwrap();
    store.insert_eval_verdict(&verdict(1, None)).unwrap();
    let in_tx = |store: &Store| {
        store
            .with_immediate_tx(|tx| {
                Ok(
                    super::eval_verdict_currency::reject_stale_eval_verdicts_conn(
                        tx,
                        "standing_rule",
                        "rule",
                        1,
                        &live("sha256:bound"),
                    ),
                )
            })
            .unwrap()
    };
    assert!(in_tx(&store).is_ok());
    select(&store, &install(&store, 1), Timestamp::now());
    assert!(in_tx(&store).is_err());
}

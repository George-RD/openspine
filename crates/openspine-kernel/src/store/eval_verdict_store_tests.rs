//! Eval-verdict store tests (change `define-lineage-and-eval-store`).
//! Done-when: eval verdicts insert/query via indexed tables.

use jiff::Timestamp;
use std::time::Duration;
use ulid::Ulid;

use super::eval_verdict_store::EvalVerdict;
use super::Store;

fn digest(n: u8) -> String {
    format!("sha256:{}", format!("{n:x}").repeat(64))
}

fn verdict(
    kind: &str,
    artifact_id: &str,
    version: u32,
    label: &str,
    fitness: Option<f64>,
    recorded_at: Timestamp,
) -> EvalVerdict {
    EvalVerdict {
        id: Ulid::new(),
        artifact_kind: kind.to_string(),
        artifact_id: artifact_id.to_string(),
        artifact_version: version,
        verdict: label.to_string(),
        fitness,
        evidence: None,
        evaluator: Some("judge-family-a".to_string()),
        artifact_digest: digest(1),
        recorded_at,
    }
}

#[test]
fn fractional_timestamp_orders_after_exact_second() {
    let store = Store::open_in_memory().expect("open store");
    let exact = Timestamp::from_second(1_700_000_000).expect("exact second");
    let fractional = exact + Duration::from_millis(500);
    store
        .insert_eval_verdict(&verdict("route", "time", 1, "exact", None, exact))
        .expect("exact");
    store
        .insert_eval_verdict(&verdict(
            "route",
            "time",
            1,
            "fractional",
            Some(0.5),
            fractional,
        ))
        .expect("fractional");

    let history = store
        .eval_verdicts_for_artifact("route", "time", 1)
        .expect("history");
    assert_eq!(
        history
            .iter()
            .map(|row| row.verdict.as_str())
            .collect::<Vec<_>>(),
        ["exact", "fractional"]
    );
    assert_eq!(
        store
            .latest_eval_verdict("route", "time", 1)
            .expect("latest")
            .expect("present")
            .verdict,
        "fractional"
    );
}

#[test]
fn insert_and_query_by_artifact_returns_ordered_rows() {
    let store = Store::open_in_memory().expect("open store");
    let t0 = Timestamp::now() - Duration::from_secs(10);
    let t1 = Timestamp::now() - Duration::from_secs(5);
    let t2 = Timestamp::now();

    let v0 = verdict("route", "main", 1, "needs_review", None, t0);
    let mut v1 = verdict("route", "main", 1, "rejected", Some(0.2), t1);
    v1.evidence = Some(digest(9));
    let v2 = verdict("route", "main", 1, "approved", Some(0.95), t2);

    store.insert_eval_verdict(&v0).expect("insert v0");
    store.insert_eval_verdict(&v1).expect("insert v1");
    store.insert_eval_verdict(&v2).expect("insert v2");

    // A different artifact must not pollute the query.
    store
        .insert_eval_verdict(&verdict(
            "route",
            "other",
            1,
            "approved",
            Some(1.0),
            Timestamp::now(),
        ))
        .expect("insert other");

    let rows = store
        .eval_verdicts_for_artifact("route", "main", 1)
        .expect("query");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].verdict, "needs_review");
    assert_eq!(rows[1].verdict, "rejected");
    assert_eq!(rows[1].evidence.as_deref(), Some(digest(9).as_str()));
    assert_eq!(rows[2].verdict, "approved");
    assert_eq!(rows[2].fitness, Some(0.95));
}

#[test]
fn query_by_verdict_filters_across_artifacts() {
    let store = Store::open_in_memory().expect("open store");
    let now = Timestamp::now();
    store
        .insert_eval_verdict(&verdict("route", "a", 1, "approved", Some(0.9), now))
        .expect("a");
    store
        .insert_eval_verdict(&verdict("agent", "b", 2, "approved", Some(0.8), now))
        .expect("b");
    store
        .insert_eval_verdict(&verdict("route", "c", 1, "rejected", Some(0.1), now))
        .expect("c");

    let approved = store
        .eval_verdicts_by_verdict("approved")
        .expect("query approved");
    assert_eq!(approved.len(), 2);
    assert!(approved.iter().all(|r| r.verdict == "approved"));

    let rejected = store
        .eval_verdicts_by_verdict("rejected")
        .expect("query rejected");
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0].artifact_id, "c");

    let empty = store
        .eval_verdicts_by_verdict("unknown_label")
        .expect("query unknown");
    assert!(empty.is_empty());
}

#[test]
fn latest_eval_verdict_returns_newest_for_artifact() {
    let store = Store::open_in_memory().expect("open store");
    let older = Timestamp::now() - Duration::from_secs(30);
    let newer = Timestamp::now();

    store
        .insert_eval_verdict(&verdict("route", "main", 1, "needs_review", None, older))
        .expect("older");
    store
        .insert_eval_verdict(&verdict("route", "main", 1, "approved", Some(0.99), newer))
        .expect("newer");

    let latest = store
        .latest_eval_verdict("route", "main", 1)
        .expect("query")
        .expect("must exist");
    assert_eq!(latest.verdict, "approved");
    assert_eq!(latest.fitness, Some(0.99));

    // Different version is a different artifact identity.
    let none = store
        .latest_eval_verdict("route", "main", 2)
        .expect("query v2");
    assert!(none.is_none());
}

#[test]
fn verdict_vocabulary_is_open() {
    // Groundwork does not constrain the verdict label; concrete policy is
    // deferred to the later evaluation change.
    let store = Store::open_in_memory().expect("open store");
    let custom = verdict(
        "pack",
        "custom_pack",
        1,
        "prover_found_attack_trace",
        None,
        Timestamp::now(),
    );
    store.insert_eval_verdict(&custom).expect("insert custom");
    let rows = store
        .eval_verdicts_by_verdict("prover_found_attack_trace")
        .expect("query");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].evaluator.as_deref(), Some("judge-family-a"));
}

#[test]
fn eval_verdicts_table_has_required_indexes() {
    // Done-when requires "indexed tables". Assert the named indexes exist
    // so accidentally dropping/mis-shaping an index fails the suite rather
    // than silently degrading to full scans.
    let store = Store::open_in_memory().expect("open store");
    let conn = store.conn.lock();
    let mut stmt = conn
        .prepare("PRAGMA index_list(eval_verdicts)")
        .expect("pragma");
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect();
    assert!(
        names.iter().any(|n| n == "idx_eval_verdicts_artifact"),
        "missing idx_eval_verdicts_artifact; got {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "idx_eval_verdicts_verdict"),
        "missing idx_eval_verdicts_verdict; got {names:?}"
    );
}

/// True when `plan` mentions `index_name` as a whole token.
fn plan_uses_index(plan: &str, index_name: &str) -> bool {
    plan.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|tok| tok == index_name)
}

#[test]
fn artifact_and_verdict_queries_use_indexes() {
    // EXPLAIN QUERY PLAN must mention the intended index for the two
    // primary lookup shapes. A full scan would fail this assertion.
    let store = Store::open_in_memory().expect("open store");
    let conn = store.conn.lock();

    let artifact_plan: String = {
        let mut stmt = conn
            .prepare(
                "EXPLAIN QUERY PLAN SELECT id FROM eval_verdicts \
                 WHERE artifact_kind = ?1 AND artifact_id = ?2 AND artifact_version = ?3 \
                 ORDER BY recorded_at ASC",
            )
            .expect("prepare");
        let plans: Vec<String> = stmt
            .query_map(rusqlite::params!["route", "main", 1i64], |row| {
                row.get::<_, String>(3)
            })
            .expect("query")
            .map(|r| r.expect("row"))
            .collect();
        plans.join(" | ")
    };
    assert!(
        plan_uses_index(&artifact_plan, "idx_eval_verdicts_artifact"),
        "artifact lookup must use idx_eval_verdicts_artifact; plan was: {artifact_plan}"
    );

    let verdict_plan: String = {
        let mut stmt = conn
            .prepare("EXPLAIN QUERY PLAN SELECT id FROM eval_verdicts WHERE verdict = ?1")
            .expect("prepare");
        let plans: Vec<String> = stmt
            .query_map(rusqlite::params!["approved"], |row| row.get::<_, String>(3))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect();
        plans.join(" | ")
    };
    assert!(
        plan_uses_index(&verdict_plan, "idx_eval_verdicts_verdict"),
        "verdict lookup must use idx_eval_verdicts_verdict; plan was: {verdict_plan}"
    );
}

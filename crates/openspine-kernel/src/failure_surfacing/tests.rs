use crate::store::failure_surfacing_types::DetailReceipt;

use super::*;

#[test]
fn authority_and_escalation_are_immediate() {
    assert!(FailureClass::Authority.routes_immediately());
    assert!(FailureClass::Escalation.routes_immediately());
    assert!(!FailureClass::Connector.routes_immediately());
    assert!(!FailureClass::Resource.routes_immediately());
}

#[test]
fn connector_failure_batches_and_is_audited() {
    let state = crate::test_support::fixtures::test_state();
    batch_failure(
        &state,
        FailureClass::Connector,
        "telegram timeout",
        "telegram timeout",
    )
    .expect("batch");
    let items = state.store.owner_digest_items().expect("digest");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].class, "connector");
    assert_eq!(items[0].summary, "telegram timeout");
    assert!(
        items[0].text_ref.is_some(),
        "new rows must carry a verified ref"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("failure.digest_batched")
            .expect("audit count"),
        1
    );
}

#[test]
fn immediate_failure_cannot_enter_digest_lane() {
    let state = crate::test_support::fixtures::test_state();
    let err =
        batch_failure(&state, FailureClass::Authority, "denied", "denied").expect_err("reject");
    assert!(err.to_string().contains("immediate owner lane"));
    assert!(state.store.owner_digest_items().expect("digest").is_empty());
}

#[test]
fn notification_failure_is_truthful_and_retryable() {
    let store = Store::open_in_memory().expect("test store");
    let grant = ulid::Ulid::new();
    store
        .record_notify_failure(555, "failure", grant, "wiremock failure")
        .expect("record failure");
    assert_eq!(
        store
            .count_audit_events_of_kind("owner.notify_failed")
            .unwrap(),
        1
    );
    assert_eq!(
        store.count_audit_events_of_kind("owner.notified").unwrap(),
        0
    );
    let pending = store.pending_dead_letters().expect("dead letters");
    assert_eq!(pending.len(), 1);
    let claimed = store
        .claim_due_dead_letter(jiff::Timestamp::now())
        .expect("claim")
        .expect("due");
    store
        .complete_dead_letter_success(
            claimed.id,
            claimed.claim_token.as_deref().unwrap(),
            claimed.task_grant_id,
            &claimed.digest_item_ids,
            None,
        )
        .expect("complete")
        .then_some(())
        .expect("claim was current");
    assert!(store.pending_dead_letters().unwrap().is_empty());
}

#[test]
fn counter_persistence_failure_is_durably_batched_as_resource() {
    let state = crate::test_support::fixtures::test_state();
    state.store.break_connector_counters_for_test();
    record_connector_outcome_or_batch(&state, "telegram", true);
    let items = state.store.owner_digest_items().expect("digest");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].class, "resource");
    assert_eq!(items[0].summary, "Connector counter persistence failed");
}

#[test]
fn connector_counters_record_success_and_failure() {
    let store = Store::open_in_memory().expect("test store");
    record_connector_outcome(&store, "telegram", true).expect("success");
    record_connector_outcome(&store, "telegram", false).expect("failure");
    assert_eq!(store.connector_counter("telegram", "success").unwrap(), 1);
    assert_eq!(store.connector_counter("telegram", "failure").unwrap(), 1);
}

#[test]
fn generic_dead_letter_has_null_semantic_metadata_and_only_owner_notified() {
    let store = Store::open_in_memory().expect("test store");
    let grant = ulid::Ulid::new();
    // Generic owner notification: no detail context.
    store
        .record_notify_failure(555, "failure", grant, "wiremock failure")
        .expect("record failure");
    let dl = store.pending_dead_letters().expect("dead letters");
    assert_eq!(dl.len(), 1);
    assert_eq!(
        dl[0].semantic_kind, None,
        "generic row has no semantic kind"
    );
    assert_eq!(dl[0].detail_ref, None);
    assert_eq!(dl[0].page_index, None);
    assert_eq!(dl[0].page_count, None);
    assert_eq!(dl[0].availability_outcome, None);
    let claimed = store
        .claim_due_dead_letter(jiff::Timestamp::now())
        .expect("claim")
        .expect("due");
    // Generic completion (detail = None) records only `owner.notified` and
    // must NOT emit a contract-specific detail receipt.
    assert!(store
        .complete_dead_letter_success(
            claimed.id,
            claimed.claim_token.as_deref().unwrap(),
            grant,
            &[],
            None,
        )
        .expect("complete"));
    assert_eq!(
        store.count_audit_events_of_kind("owner.notified").unwrap(),
        1
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("failure.digest_detail_viewed")
            .unwrap(),
        0
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("failure.digest_detail_unavailable")
            .unwrap(),
        0
    );
}

#[test]
fn detail_dead_letter_completion_is_fenced_against_duplicate_receipt() {
    let store = Store::open_in_memory().expect("test store");
    let grant = ulid::Ulid::new();
    let detail = DetailReceipt {
        detail_ref: Some("ref".to_string()),
        page_index: 1,
        page_count: 1,
        unavailable_reason: None,
    };
    store
        .record_notify_failure_with_digest(555, "ref", grant, "wiremock", &[], Some(&detail))
        .expect("record");
    let claimed = store
        .claim_due_dead_letter(jiff::Timestamp::now() + std::time::Duration::from_secs(1))
        .expect("claim")
        .expect("due");
    let token = claimed.claim_token.clone().unwrap();
    // First completion: fenced, writes exactly one detail receipt.
    assert!(store
        .complete_dead_letter_success(claimed.id, &token, grant, &[], Some(&detail))
        .expect("first complete"));
    assert_eq!(
        store
            .count_audit_events_of_kind("failure.digest_detail_viewed")
            .unwrap(),
        1
    );
    // Duplicate completion with the now-stale token: no-op, no second receipt.
    assert!(!store
        .complete_dead_letter_success(claimed.id, &token, grant, &[], Some(&detail))
        .expect("second complete"));
    assert_eq!(
        store
            .count_audit_events_of_kind("failure.digest_detail_viewed")
            .unwrap(),
        1
    );
    assert_eq!(
        store.count_audit_events_of_kind("owner.notified").unwrap(),
        1
    );
}

#[test]
fn unavailable_detail_retry_emits_unavailable_receipt_with_page_metadata() {
    let store = Store::open_in_memory().expect("test store");
    let grant = ulid::Ulid::new();
    let detail = DetailReceipt {
        detail_ref: Some("ref".to_string()),
        page_index: 2,
        page_count: 3,
        unavailable_reason: Some("legacy".to_string()),
    };
    store
        .record_notify_failure_with_digest(555, "ref", grant, "wiremock", &[], Some(&detail))
        .expect("record");
    let claimed = store
        .claim_due_dead_letter(jiff::Timestamp::now())
        .expect("claim")
        .expect("due");
    let token = claimed.claim_token.clone().unwrap();
    assert!(store
        .complete_dead_letter_success(claimed.id, &token, grant, &[], Some(&detail))
        .expect("complete"));
    assert_eq!(
        store
            .count_audit_events_of_kind("failure.digest_detail_unavailable")
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("failure.digest_detail_viewed")
            .unwrap(),
        0
    );
    // The unavailable receipt preserves both the reason and `page=N/M`.
    let events = store.all_audit_event_jsons().unwrap();
    let receipt: serde_json::Value = events
        .iter()
        .map(|e| serde_json::from_str::<serde_json::Value>(e).unwrap())
        .find(|e| e["kind"] == "failure.digest_detail_unavailable")
        .expect("unavailable receipt present");
    assert_eq!(receipt["reason"], "legacy; page=2/3");
}

#[test]
fn legacy_dead_letter_rows_migrate_and_read_as_generic() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("ospine-legacy-{}.db", ulid::Ulid::new()));
    let _ = std::fs::remove_file(&path);
    // Simulate a pre-migration database: `notify_dead_letters` without the
    // semantic-metadata columns.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE notify_dead_letters (
               id TEXT PRIMARY KEY,
               enqueued_at TEXT NOT NULL,
               chat_id INTEGER NOT NULL,
               text_ref TEXT NOT NULL,
               task_grant_id TEXT,
               digest_item_ids TEXT NOT NULL DEFAULT '',
               attempts INTEGER NOT NULL DEFAULT 0,
               next_attempt_at TEXT NOT NULL,
               claimed_until TEXT,
               claim_token TEXT,
               state TEXT NOT NULL DEFAULT 'pending'
             );",
        )
        .unwrap();
        let id = ulid::Ulid::new().to_string();
        let grant = ulid::Ulid::new().to_string();
        conn.execute(
            "INSERT INTO notify_dead_letters \
             (id, enqueued_at, chat_id, text_ref, task_grant_id, next_attempt_at) \
             VALUES (?1, '2026-01-01T00:00:00Z', 555, 'ref', ?2, '2099-01-01T00:00:00Z')",
            rusqlite::params![id, grant],
        )
        .unwrap();
    }
    // Current `Store::open` runs the migration, adding the new columns.
    let store = Store::open(&path).expect("open legacy db");
    let dl = store.pending_dead_letters().expect("read legacy rows");
    assert_eq!(dl.len(), 1, "legacy row must survive migration");
    assert_eq!(
        dl[0].semantic_kind, None,
        "legacy row has NULL semantic metadata"
    );
    assert_eq!(dl[0].detail_ref, None);
    assert_eq!(dl[0].page_index, None);
    assert_eq!(dl[0].page_count, None);
    assert_eq!(dl[0].availability_outcome, None);
    // Reopening must be idempotent and still read the legacy row.
    let store2 = Store::open(&path).expect("reopen legacy db");
    let dl2 = store2
        .pending_dead_letters()
        .expect("read legacy rows again");
    assert_eq!(dl2.len(), 1);
    assert_eq!(dl2[0].semantic_kind, None);
    let _ = std::fs::remove_file(&path);
}

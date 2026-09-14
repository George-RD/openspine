use super::*;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use openspine_schemas::grant::TaskGrant;
use std::time::Duration;

fn at() -> Timestamp {
    "2099-01-01T12:00:00Z".parse().unwrap()
}

pub(super) fn insert_grant(store: &Store, expires_at: Timestamp) -> TaskGrant {
    let mut grant = crate::store::tests::sample_grant("census-grant-token");
    grant.issued_at = expires_at - Duration::from_secs(120);
    grant.expires_at = expires_at;
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    let pending = ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", "c".repeat(64))).unwrap(),
        schema_version: 1,
    };
    store
        .insert_task_grant(&grant, &pending, &crate::test_support::telegram_surface(555))
        .unwrap();
    grant
}

fn persisted_row(store: &Store) -> (String, String) {
    store.with_conn_for_test(|conn| {
        conn.query_row("SELECT expires_at, grant_json FROM task_grants", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap()
    })
}

fn replace_index(store: &Store, value: &str) {
    store.with_conn_for_test(|conn| {
        assert_eq!(
            conn.execute("UPDATE task_grants SET expires_at = ?1", [value])
                .unwrap(),
            1
        );
    });
}

fn replace_json(store: &Store, value: &str) {
    store.with_conn_for_test(|conn| {
        assert_eq!(
            conn.execute("UPDATE task_grants SET grant_json = ?1", [value])
                .unwrap(),
            1
        );
    });
}

fn assert_grant_counts(store: &Store, as_of: Timestamp, expected: OutstandingWorkCounts) {
    let before = persisted_row(store);
    let snapshot = store.package_outstanding_work(as_of).unwrap();
    assert_eq!(snapshot.source(OutstandingWorkSource::TaskGrants), expected);
    assert_eq!(
        snapshot.is_quiescent(),
        expected.outstanding == 0 && expected.unknown == 0
    );
    assert_eq!(persisted_row(store), before, "review must not repair or sweep grants");
}

fn assert_unknown(store: &Store) {
    assert_grant_counts(
        store,
        at(),
        OutstandingWorkCounts {
            outstanding: 0,
            terminal: 0,
            unknown: 1,
        },
    );
}

#[test]
fn typed_grant_expiry_uses_captured_nanosecond_boundary() {
    let store = Store::open_in_memory().unwrap();
    let grant = insert_grant(&store, at());
    for as_of in [at() - Duration::from_nanos(1), at(), at() + Duration::from_nanos(1)] {
        let expired = grant.is_expired(as_of);
        assert_grant_counts(
            &store,
            as_of,
            OutstandingWorkCounts {
                outstanding: if expired { 0 } else { 1 },
                terminal: if expired { 1 } else { 0 },
                unknown: 0,
            },
        );
    }
}

#[test]
fn live_grant_with_stale_expired_index_is_unknown_not_terminal() {
    let store = Store::open_in_memory().unwrap();
    let grant = insert_grant(&store, at() + Duration::from_secs(60));
    replace_index(&store, &crate::store::sql_timestamp(at() - Duration::from_secs(60)));
    // Prove the same row is still live through both real runtime lookup paths.
    let by_id = store.find_task_grant_by_id(grant.id).unwrap().unwrap().0;
    let by_token = store.find_task_grant_by_token(&grant.task_token).unwrap().unwrap().0;
    assert!(!by_id.is_expired(at()));
    assert!(!by_token.is_expired(at()));
    assert_unknown(&store);
}

#[test]
fn expired_grant_with_live_index_is_unknown() {
    let store = Store::open_in_memory().unwrap();
    insert_grant(&store, at() - Duration::from_secs(60));
    replace_index(&store, &crate::store::sql_timestamp(at() + Duration::from_secs(60)));
    assert_unknown(&store);
}

#[test]
fn malformed_grant_expiry_index_is_unknown() {
    for index in ["", "0", "not-a-timestamp", "2099-13-01T12:00:00Z"] {
        let store = Store::open_in_memory().unwrap();
        insert_grant(&store, at() + Duration::from_secs(60));
        replace_index(&store, index);
        assert_unknown(&store);
    }
}

#[test]
fn malformed_grant_json_cannot_hide_behind_expired_index() {
    for json in ["not-json", "{}"] {
        let store = Store::open_in_memory().unwrap();
        insert_grant(&store, at() - Duration::from_secs(60));
        replace_json(&store, json);
        assert_unknown(&store);
    }
    let store = Store::open_in_memory().unwrap();
    insert_grant(&store, at() - Duration::from_secs(60));
    let mut json: serde_json::Value = serde_json::from_str(&persisted_row(&store).1).unwrap();
    json["expires_at"] = serde_json::json!("invalid");
    replace_json(&store, &serde_json::to_string(&json).unwrap());
    assert_unknown(&store);
}

#[test]
fn grant_identity_or_schema_drift_is_unknown() {
    for (field, value) in [
        ("id", serde_json::json!(ulid::Ulid::new().to_string())),
        ("schema_version", serde_json::json!(2)),
    ] {
        let store = Store::open_in_memory().unwrap();
        insert_grant(&store, at() - Duration::from_secs(60));
        let mut json: serde_json::Value = serde_json::from_str(&persisted_row(&store).1).unwrap();
        json[field] = value;
        replace_json(&store, &serde_json::to_string(&json).unwrap());
        assert_unknown(&store);
    }
}

#[test]
fn equivalent_expiry_encodings_compare_instants_not_text() {
    for index in ["2099-01-01T12:00:00Z", "2099-01-01T16:00:00+04:00"] {
        let store = Store::open_in_memory().unwrap();
        insert_grant(&store, at());
        replace_index(&store, index);
        assert_grant_counts(
            &store,
            at(),
            OutstandingWorkCounts {
                outstanding: 0,
                terminal: 1,
                unknown: 0,
            },
        );
    }
}

use super::*;
use openspine_schemas::action::ActionRequest;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::grant::TaskGrant;
use std::time::Duration;

fn at() -> Timestamp {
    "2099-01-01T12:00:00Z".parse().unwrap()
}

fn grant(store: &Store, expiry: Timestamp) -> TaskGrant {
    let mut grant = crate::store::tests::sample_grant(&Ulid::new().to_string());
    grant.issued_at = expiry - Duration::from_secs(120);
    grant.expires_at = expiry;
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    store
        .insert_task_grant(
            &grant,
            &ArtifactRef {
                digest: digest_of_bytes(b"pending approval"),
                schema_version: 1,
            },
            &crate::test_support::telegram_surface(555),
        )
        .unwrap();
    grant
}

fn request(store: &Store, grant_id: Ulid, action: &str) -> ActionRequest {
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant_id,
        action: action.into(),
        target_ref: None,
        payload_ref: Some(ArtifactRef {
            digest: digest_of_bytes(b"approved payload"),
            schema_version: 1,
        }),
        target_digest: Some(digest_of_bytes(b"approved target")),
        selection_token_id: None,
        params: Default::default(),
        skill_attribution: None,
        requested_at: at() - Duration::from_secs(120),
        schema_version: 1,
    };
    store.insert_action_request(&request).unwrap();
    request
}

fn persisted(store: &Store) -> Vec<(String, String, i64)> {
    store.with_conn_for_test(|conn| {
        conn.prepare("SELECT id, request_json, used FROM action_requests ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    })
}

fn assert_counts(store: &Store, as_of: Timestamp, outstanding: u64, terminal: u64, unknown: u64) {
    let before = persisted(store);
    let audit_before = store.all_audit_event_jsons().unwrap();
    let snapshot = store.package_outstanding_work(as_of).unwrap();
    assert_eq!(
        snapshot.source(OutstandingWorkSource::ActionRequests),
        OutstandingWorkCounts { outstanding, terminal, unknown }
    );
    assert_eq!(persisted(store), before, "census must not consume requests");
    assert_eq!(store.all_audit_event_jsons().unwrap(), audit_before);
    if outstanding > 0 || unknown > 0 {
        assert!(!snapshot.is_quiescent());
    }
}

#[test]
fn ordinary_approval_uses_the_captured_grant_expiry_boundary() {
    let store = Store::open_in_memory().unwrap();
    let grant = grant(&store, at());
    request(&store, grant.id, "email.create_draft");
    assert_counts(&store, at() - Duration::from_nanos(1), 1, 0, 0);
    assert_counts(&store, at(), 0, 1, 0);
    assert_counts(&store, at() + Duration::from_nanos(1), 0, 1, 0);
    assert!(store.package_outstanding_work(at()).unwrap().is_quiescent());
}

#[test]
fn swept_ordinary_approval_is_terminal_without_consumption() {
    let store = Store::open_in_memory().unwrap();
    let grant = grant(&store, at() - Duration::from_secs(172800));
    let request = request(&store, grant.id, "email.create_draft");
    store.sweep_expired_grants(at()).unwrap();
    assert!(store.find_task_grant_by_id(grant.id).unwrap().is_none());
    assert_eq!(store.find_action_request(request.id).unwrap(), Some(request));
    assert_counts(&store, at(), 0, 1, 0);
    assert!(store.package_outstanding_work(at()).unwrap().is_quiescent());
}

#[test]
fn grantless_reconfirmation_still_blocks() {
    let store = Store::open_in_memory().unwrap();
    request(&store, Ulid::new(), "artifact.reconfirm");
    assert_counts(&store, at(), 1, 0, 0);
}

#[test]
fn expired_reconfirmation_still_blocks() {
    let store = Store::open_in_memory().unwrap();
    let grant = grant(&store, at() - Duration::from_secs(60));
    request(&store, grant.id, "artifact.reconfirm");
    assert_counts(&store, at(), 1, 0, 0);
}

#[test]
fn invalid_request_json_identity_schema_or_action_is_unknown() {
    for mutation in ["json", "id", "schema_version", "action", "task_grant_id"] {
        let store = Store::open_in_memory().unwrap();
        let request = request(&store, Ulid::new(), "email.create_draft");
        let mut json = serde_json::to_value(&request).unwrap();
        let value = match mutation {
            "json" => "not-json".to_string(),
            field => {
                json[field] = match field {
                    "id" => serde_json::json!(Ulid::new()),
                    "schema_version" => serde_json::json!(2),
                    _ => serde_json::json!("not-a-known-binding"),
                };
                serde_json::to_string(&json).unwrap()
            }
        };
        store.with_conn_for_test(|conn| {
            conn.execute("UPDATE action_requests SET request_json = ?1", [value])
                .unwrap();
        });
        assert_counts(&store, at(), 0, 0, 1);
    }
}

#[test]
fn inconsistent_grant_expiry_keeps_ordinary_request_unknown() {
    let store = Store::open_in_memory().unwrap();
    let grant = grant(&store, at() + Duration::from_secs(60));
    request(&store, grant.id, "email.create_draft");
    store.with_conn_for_test(|conn| {
        conn.execute(
            "UPDATE task_grants SET expires_at = ?1",
            [crate::store::sql_timestamp(at() - Duration::from_secs(60))],
        )
        .unwrap();
    });
    assert_counts(&store, at(), 0, 0, 1);
}

#[test]
fn consumed_request_stays_terminal_when_the_grant_is_absent() {
    let store = Store::open_in_memory().unwrap();
    let request = request(&store, Ulid::new(), "artifact.reconfirm");
    assert!(store.try_consume_action_request(request.id).unwrap());
    assert_counts(&store, at(), 0, 1, 0);
}

#[test]
fn mixed_approval_rows_are_partitioned_without_mutation() {
    let store = Store::open_in_memory().unwrap();
    let live = grant(&store, at() + Duration::from_secs(60));
    let expired = grant(&store, at() - Duration::from_secs(60));
    request(&store, live.id, "email.create_draft");
    request(&store, expired.id, "email.create_draft");
    request(&store, Ulid::new(), "artifact.reconfirm");
    request(&store, Ulid::new(), "future.unsupported_action");
    assert_counts(&store, at(), 2, 1, 1);
}

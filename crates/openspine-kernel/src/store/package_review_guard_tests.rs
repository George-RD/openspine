#[test]
fn missing_canonical_reader_cannot_certify_terminal_proposal() {
    let h = fixture(at());
    let snapshot = h.state.store.package_outstanding_work(at()).unwrap();
    assert_eq!(snapshot.source(OutstandingWorkSource::ProposedArtifacts), expected(0, 0, 1));
    assert!(!snapshot.is_quiescent());
}

#[test]
fn shared_activation_request_cannot_clear_multiple_proposals() {
    let h = fixture(at());
    h.state.store.with_conn_for_test(|conn| {
        conn.execute(
            "INSERT INTO proposed_artifacts
             (id, kind, artifact_id, version, state, yaml_digest, task_grant_id, action_request_id, proposed_at)
             SELECT ?1, kind, 'another-rule', version, state, yaml_digest, task_grant_id, action_request_id, proposed_at
             FROM proposed_artifacts WHERE artifact_id = 'census-rule'",
            [Ulid::new().to_string()],
        ).unwrap();
    });
    assert_eq!(proposal_counts(&h, at()), expected(0, 0, 2));
}

#[test]
fn exact_review_bound_to_another_principal_is_unknown() {
    let h = fixture(at());
    h.state.store.with_conn_for_test(|conn| {
        conn.execute(
            "UPDATE owner_reviews SET owner_principal_id = ?1 WHERE id = ?2",
            rusqlite::params![Ulid::new().to_string(), h.review.id.to_string()],
        ).unwrap();
    });
    assert_eq!(proposal_counts(&h, at()), expected(0, 0, 1));
}

#[test]
fn unsupported_review_reference_schema_is_unknown() {
    let h = fixture(at());
    h.state.store.with_conn_for_test(|conn| {
        conn.execute("UPDATE owner_reviews SET artifact_ref_schema_version = 2", []).unwrap();
    });
    assert_eq!(proposal_counts(&h, at()), expected(0, 0, 1));
}

#[test]
fn terminal_review_stays_terminal_after_its_source_grant_is_swept() {
    let h = fixture(at());
    let later = at() + Duration::from_secs(172800);
    h.state.store.sweep_expired_grants(later).unwrap();
    assert!(h.state.store.find_task_grant_by_id(h.request.task_grant_id).unwrap().is_none());
    assert_eq!(proposal_counts(&h, later), expected(0, 1, 0));
}

#[test]
fn activation_request_identity_grant_or_payload_drift_is_unknown() {
    for axis in ["id", "task_grant_id", "payload"] {
        let h = fixture(at());
        let mut json = serde_json::to_value(&h.request).unwrap();
        if axis == "payload" {
            json["payload_ref"]["digest"] = serde_json::json!(digest_of_bytes(b"other payload"));
        } else {
            json[axis] = serde_json::json!(Ulid::new());
        }
        h.state.store.with_conn_for_test(|conn| {
            conn.execute(
                "UPDATE action_requests SET request_json = ?1 WHERE id = ?2",
                rusqlite::params![serde_json::to_string(&json).unwrap(), h.request.id.to_string()],
            ).unwrap();
        });
        assert_eq!(proposal_counts(&h, at()), expected(0, 0, 1), "{axis}");
    }
}

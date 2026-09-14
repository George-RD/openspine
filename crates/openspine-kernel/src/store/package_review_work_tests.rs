use super::*;
include!("package_review_test_fixtures.rs");

#[test]
fn rejected_evaluated_review_leaves_only_terminal_proposal_history() {
    let h = fixture(at() + Duration::from_secs(60));
    decide(&h, DecisionIntent::Reject, at()).unwrap();
    assert_eq!(h.state.store.owner_review_row(h.review.id).unwrap().unwrap().state, OwnerReviewState::Rejected);
    assert_eq!(proposal_counts(&h, at()), expected(0, 1, 0));
    assert!(h.state.store.package_outstanding_work_with_reviews(at(), &h.state.artifacts).unwrap().is_quiescent());
}

#[test]
fn untouched_pending_review_expires_at_the_captured_instant_without_a_sweep() {
    let h = fixture(at());
    assert_eq!(proposal_counts(&h, at() - Duration::from_nanos(1)), expected(1, 0, 0));
    assert_eq!(proposal_counts(&h, at()), expected(0, 1, 0));
    assert_eq!(proposal_counts(&h, at() + Duration::from_nanos(1)), expected(0, 1, 0));
    assert_eq!(h.state.store.owner_review_row(h.review.id).unwrap().unwrap().state, OwnerReviewState::Pending);
}

#[test]
fn real_expired_decision_does_not_leave_permanent_proposal_work() {
    let h = fixture(at());
    assert!(matches!(decide(&h, DecisionIntent::Approve, at()), Err(crate::pipeline::owner_review_decision::OwnerReviewDecisionError::Expired)));
    assert_eq!(h.state.store.owner_review_row(h.review.id).unwrap().unwrap().state, OwnerReviewState::Expired);
    assert_eq!(proposal_counts(&h, at()), expected(0, 1, 0));
}

#[test]
fn live_evaluated_review_remains_work_after_ordinary_grant_expiry() {
    let h = fixture(at() + Duration::from_secs(60));
    let snapshot = h.state.store.package_outstanding_work_with_reviews(at(), &h.state.artifacts).unwrap();
    assert_eq!(snapshot.source(OutstandingWorkSource::ActionRequests), expected(0, 1, 0));
    assert_eq!(snapshot.source(OutstandingWorkSource::ProposedArtifacts), expected(1, 0, 0));
    assert!(!snapshot.is_quiescent());
}

#[test]
fn another_live_exact_review_prevents_terminal_classification() {
    let h = fixture(at() + Duration::from_secs(60));
    decide(&h, DecisionIntent::Reject, at()).unwrap();
    let mut next = h.review.clone();
    next.id = Ulid::new();
    let binding = next.evaluation_binding.clone().unwrap();
    next = next.with_evaluation_binding(binding);
    persist(&h.state, &next, at() + Duration::from_secs(60));
    assert_eq!(proposal_counts(&h, at()), expected(1, 0, 0));
}

#[test]
fn unrelated_terminal_review_with_the_same_digest_cannot_clear_a_proposal() {
    for axis in ["kind", "id", "version"] {
        let h = fixture(at());
        let mut binding = h.review.evaluation_binding.clone().unwrap();
        match axis {
            "kind" => binding.artifact_kind = "route".into(),
            "id" => binding.artifact_id = "another-rule".into(),
            _ => binding.artifact_version = 2,
        }
        let review = h.review.clone().with_evaluation_binding(binding);
        replace_review(&h, &review);
        assert_eq!(proposal_counts(&h, at()), expected(1, 0, 0), "{axis}");
    }
}

#[test]
fn conflicting_exact_review_binding_is_unknown() {
    for axis in ["request", "digest"] {
        let h = fixture(at());
        let mut binding = h.review.evaluation_binding.clone().unwrap();
        match axis {
            "request" => binding.action_request_id = Ulid::new(),
            _ => binding.proposal_digest = digest_of_bytes(b"different proposal"),
        }
        let review = h.review.clone().with_evaluation_binding(binding);
        replace_review(&h, &review);
        assert_eq!(proposal_counts(&h, at()), expected(0, 0, 1), "{axis}");
    }
}

#[test]
fn missing_or_corrupt_review_bytes_are_unknown_not_terminal() {
    for remove in [false, true] {
        let h = fixture(at());
        let path = h.state.artifacts.blob_path_for_test(&h.review_ref);
        if remove {
            std::fs::remove_file(&path).unwrap();
        } else {
            let mut bytes = std::fs::read(&path).unwrap();
            *bytes.last_mut().unwrap() ^= 1;
            std::fs::write(&path, bytes).unwrap();
        }
        assert_eq!(proposal_counts(&h, at()), expected(0, 0, 1));
    }
}

#[test]
fn review_identity_schema_or_binding_drift_is_unknown() {
    for axis in ["id", "schema", "binding"] {
        let h = fixture(at());
        let mut review = h.review.clone();
        match axis {
            "id" => review.id = Ulid::new(),
            "schema" => review.schema_version = 2,
            _ => review.title = "mutated without rebinding".into(),
        }
        if axis != "binding" {
            let binding = review.evaluation_binding.clone().unwrap();
            review = review.with_evaluation_binding(binding);
        }
        replace_review(&h, &review);
        assert_eq!(proposal_counts(&h, at()), expected(0, 0, 1), "{axis}");
    }
}

#[test]
fn terminal_review_does_not_hide_a_live_ordinary_callback() {
    let h = fixture(at() + Duration::from_secs(60));
    decide(&h, DecisionIntent::Reject, at()).unwrap();
    let mut grant = h.state.store.find_task_grant_by_id(h.request.task_grant_id).unwrap().unwrap().0;
    grant.expires_at = at() + Duration::from_secs(60);
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    h.state.store.refresh_task_grant(&grant).unwrap();
    assert_eq!(proposal_counts(&h, at()), expected(1, 0, 0));
}

#[test]
fn missing_activation_request_cannot_be_used_as_terminal_binding_evidence() {
    let h = fixture(at());
    h.state.store.with_conn_for_test(|conn| {
        conn.execute("DELETE FROM action_requests WHERE id = ?1", [h.request.id.to_string()]).unwrap();
    });
    assert_eq!(proposal_counts(&h, at()), expected(0, 0, 1));
}

#[test]
fn review_census_never_recovers_blob_markers_and_snapshot_stays_owned() {
    let h = fixture(at());
    let path = h.state.artifacts.blob_path_for_test(&h.review_ref);
    let marker = path.with_extension("upgrade-pending");
    std::fs::write(&marker, b"").unwrap();
    let before = std::fs::read(&path).unwrap();
    let snapshot = h.state.store.package_outstanding_work_with_reviews(at(), &h.state.artifacts).unwrap();
    assert!(snapshot.is_quiescent());
    assert!(marker.exists(), "review must not invoke artifact recovery");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::remove_file(path).unwrap();
    assert!(snapshot.is_quiescent(), "captured result cannot reread deleted evidence");
    assert_eq!(proposal_counts(&h, at()), expected(0, 0, 1));
}

fn replace_review(h: &ReviewHarness, review: &OwnerReviewRequest) {
    let artifact = h.state.artifacts.put(&serde_json::to_vec(review).unwrap()).unwrap();
    h.state.store.with_conn_for_test(|conn| {
        conn.execute(
            "UPDATE owner_reviews SET artifact_ref_digest = ?1 WHERE id = ?2",
            rusqlite::params![artifact.digest.as_str(), h.review.id.to_string()],
        ).unwrap();
    });
}

// Seed the separately evaluated replacement, then use the real atomic
// supersession operation. This tests census lifecycle accounting, not the
// miner's replay/judge evaluation, which has its own production-path test.
fn supersede_with_replacement(h: &ReviewHarness, expiry: Timestamp) -> OwnerReviewRequest {
    let mut manifest: StandingRuleManifest = serde_yaml::from_slice(
        &h.state.artifacts.get(h.request.payload_ref.as_ref().unwrap()).unwrap(),
    ).unwrap();
    manifest.id = "census-rule-narrowed".into();
    manifest.quota.max = 1;
    let payload = h.state.artifacts.put(serde_yaml::to_string(&manifest).unwrap().as_bytes()).unwrap();
    let mut request = h.request.clone();
    request.id = Ulid::new();
    request.payload_ref = Some(payload.clone());
    request.target_digest = Some(digest_of_bytes(b"census-rule-narrowed-v1"));
    h.state.store.insert_action_request(&request).unwrap();
    h.state.store.with_conn_for_test(|conn| {
        conn.execute(
            "INSERT INTO proposed_artifacts
             (id, kind, artifact_id, version, state, yaml_digest, task_grant_id, action_request_id, proposed_at)
             VALUES (?1, 'standing_rule', 'census-rule-narrowed', 1, 'review_required', ?2, ?3, ?4, ?5)",
            rusqlite::params![Ulid::new().to_string(), payload.digest.as_str(), request.task_grant_id.to_string(), request.id.to_string(), at().to_string()],
        ).unwrap();
    });
    let mut replacement = h.review.clone();
    replacement.id = Ulid::new();
    replacement.limits.quota.max = 1;
    replacement.proposal_digest = payload.digest.clone();
    let mut binding = replacement.evaluation_binding.clone().unwrap();
    binding.artifact_id = manifest.id;
    binding.action_request_id = request.id;
    binding.proposal_digest = payload.digest.clone();
    binding.epochs.proposal_digest = Some(payload.digest);
    replacement = replacement.with_evaluation_binding(binding);
    let artifact = h.state.artifacts.put(&serde_json::to_vec(&replacement).unwrap()).unwrap();
    h.state.store.insert_narrowed_owner_review(
        (h.review.id, h.review.binding_digest()), replacement.id, &artifact,
        h.state.owner.principal_id.as_ulid(), expiry, at(),
    ).unwrap();
    replacement
}

#[test]
fn narrowed_review_is_terminal_only_for_the_superseded_proposal() {
    let expiry = at() + Duration::from_secs(30 * 86400);
    let h = fixture(expiry);
    let replacement = supersede_with_replacement(&h, expiry);
    assert_eq!(h.state.store.owner_review_row(h.review.id).unwrap().unwrap().state, OwnerReviewState::Narrowed);
    assert_eq!(h.state.store.owner_review_row(replacement.id).unwrap().unwrap().state, OwnerReviewState::Pending);
    assert_eq!(proposal_counts(&h, at()), expected(1, 1, 0));
    assert!(!h.state.store.package_outstanding_work_with_reviews(at(), &h.state.artifacts).unwrap().is_quiescent());
}

#[test]
fn rejecting_the_narrowed_replacement_does_not_wait_for_original_review_expiry() {
    let expiry = at() + Duration::from_secs(30 * 86400);
    let h = fixture(expiry);
    let replacement = supersede_with_replacement(&h, expiry);
    let surface = OwnerSurfaceRef::authenticated_terminal(h.state.owner.principal_id.as_ulid());
    crate::pipeline::owner_review_decision::submit_owner_review_decision(
        &h.state, &surface, replacement.id, replacement.binding_digest(),
        DecisionIntent::Reject, None, at(),
    ).unwrap();
    assert_eq!(proposal_counts(&h, at()), expected(0, 2, 0));
    assert!(h.state.store.package_outstanding_work_with_reviews(at(), &h.state.artifacts).unwrap().is_quiescent());
    assert_eq!(h.state.store.owner_review_row(h.review.id).unwrap().unwrap().state, OwnerReviewState::Narrowed);
}

#[test]
fn narrowed_review_cannot_hide_another_live_review_of_the_original_proposal() {
    let expiry = at() + Duration::from_secs(30 * 86400);
    let h = fixture(expiry);
    supersede_with_replacement(&h, expiry);
    let mut competing = h.review.clone();
    competing.id = Ulid::new();
    let binding = competing.evaluation_binding.clone().unwrap();
    competing = competing.with_evaluation_binding(binding);
    persist(&h.state, &competing, expiry);
    assert_eq!(proposal_counts(&h, at()), expected(2, 0, 0));
}

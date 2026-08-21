use super::*;

#[tokio::test]
async fn asymmetric_rows_prove_context_grouping() {
    let harness = miner_tick_harness().await;
    let (first, first_target, first_scope, _) =
        resolved_email_context(&harness.state, "thread-1", 'a', '1', Ulid::from(11_u128));
    let (second, second_target, second_scope, _) =
        resolved_email_context(&harness.state, "thread-2", 'c', '2', Ulid::from(12_u128));
    append_approval(&harness, &first, &first_target, &first_scope, 'f');
    append_approval(&harness, &first, &first_target, &first_scope, '0');
    for payload in ['1', '2', '3'] {
        append_approval(&harness, &second, &second_target, &second_scope, payload);
    }
    let first_class = ReviewedActionScope::derive(&first)
        .unwrap()
        .context_class_digest()
        .clone();
    let second_class = ReviewedActionScope::derive(&second)
        .unwrap()
        .context_class_digest()
        .clone();
    let tick = reflection_miner_tick(&harness.state).await.unwrap();
    let description_for = |class: &Digest| {
        harness
            .state
            .store
            .find_proposed_artifact("standing_rule", class.as_str(), 1)
            .unwrap()
            .map(|row| {
                let artifact_ref = ArtifactRef {
                    digest: Digest::parse(row.yaml_digest).unwrap(),
                    schema_version: 1,
                };
                String::from_utf8(harness.state.artifacts.get(&artifact_ref).unwrap()).unwrap()
            })
            .and_then(|yaml| {
                yaml.lines()
                    .find(|line| line.starts_with("description:"))
                    .map(str::to_owned)
            })
    };
    let first_description = description_for(&first_class);
    let second_description = description_for(&second_class);
    assert_eq!(tick, 1);
    assert_eq!(
        first_description, None,
        "the larger second context class must be selected"
    );
    assert_eq!(
        second_description.as_deref(),
        Some("description: 3 matching owner approvals")
    );
}
/// Append an approval whose resolved request shape intentionally differs from
/// the reviewed binding while keeping the evidence grouping class fixed. The
/// request digest remains produced by `from_context`; only the grouping/binding
/// fields are held constant so a payload-derived request-key mutation is
/// observable.
fn append_approval_with_grouping_binding(
    harness: &MinerTickHarness,
    context: &ResolvedActionContext,
    target_ref: &ArtifactRef,
    reviewed_scope_ref: &ArtifactRef,
    binding: &ReviewedScopeBinding,
    payload_byte: char,
) -> AuditEvent {
    let action = ActionId::new("email.create_draft");
    let mut metadata =
        crate::store::OwnerApprovalAuditMetadata::from_context(context, reviewed_scope_ref.clone())
            .expect("resolved context must carry all approval evidence");
    metadata.context_class_digest = binding.scope.context_class_digest().clone();
    metadata.reviewed_scope_digest = binding.reviewed_scope_digest.clone();
    metadata.compatibility_digest = binding.compatibility_digest.clone();
    metadata.payload_digest =
        Digest::parse(format!("sha256:{}", payload_byte.to_string().repeat(64))).unwrap();
    let metadata_json = serde_json::to_string(&metadata).unwrap();
    harness
        .state
        .store
        .append_audit_with_payload_json(
            "action.gated",
            Some(&action),
            Some(&GateDecision::Allow),
            Some(crate::store::OWNER_APPROVAL_GATE_REASON),
            Some(harness.owner_grant.id),
            std::slice::from_ref(target_ref),
            std::slice::from_ref(target_ref),
            Some(&metadata_json),
        )
        .unwrap()
}
#[tokio::test]
async fn scheduled_reflection_miner_request_shape_mismatch_is_rejected() {
    let harness = miner_tick_harness().await;
    let (first, first_target, first_scope, first_binding) =
        resolved_email_context(&harness.state, "thread-1", 'a', '1', Ulid::from(11_u128));
    let (second, _, _, _) = resolved_email_context_with_task_shape(
        &harness.state,
        "thread-1",
        'a',
        '1',
        Ulid::from(11_u128),
        'd',
    );
    let first_shape = first.task_shape_digest().unwrap().clone();
    let second_shape = second.task_shape_digest().unwrap().clone();
    assert_ne!(first_shape, second_shape);

    let first_event = append_approval(&harness, &first, &first_target, &first_scope, 'f');
    let second_event = append_approval_with_grouping_binding(
        &harness,
        &second,
        &first_target,
        &first_scope,
        &first_binding,
        'f',
    );
    assert_ne!(first_event.id, second_event.id);
    assert_eq!(
        reflection_miner_tick(&harness.state).await.unwrap(),
        0,
        "different task-shape digests must not form one repeated-approval pattern"
    );
}
/// Insert another valid ledger row whose decision event id is the existing
/// event id. The production ledger enforces unique ids, so this test rebuilds
/// its private in-memory audit table without that constraint to model a
/// duplicated audit read.
fn append_duplicate_audit_row(state: &crate::pipeline::AppState, event: &AuditEvent) {
    state.store.with_conn_for_test(|conn| {
        conn.execute_batch(
            "DROP INDEX IF EXISTS idx_audit_id;
             DROP INDEX IF EXISTS idx_audit_aggregate;
             DROP INDEX IF EXISTS idx_audit_aggregate_seq_unique;
             ALTER TABLE audit_log RENAME TO audit_log_original;
             CREATE TABLE audit_log (
                 seq INTEGER PRIMARY KEY AUTOINCREMENT,
                 id TEXT NOT NULL,
                 ts TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 prev_hash TEXT NOT NULL,
                 hash TEXT NOT NULL,
                 meta_json TEXT NOT NULL,
                 event_json TEXT NOT NULL,
                 aggregate_id TEXT NOT NULL DEFAULT 'system',
                 aggregate_seq INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO audit_log
                 (seq, id, ts, kind, prev_hash, hash, meta_json, event_json,
                  aggregate_id, aggregate_seq)
             SELECT seq, id, ts, kind, prev_hash, hash, meta_json, event_json,
                    aggregate_id, aggregate_seq
             FROM audit_log_original
             ORDER BY seq;
             DROP TABLE audit_log_original;",
        )
        .unwrap();
        let prev_hash_text: String = conn
            .query_row(
                "SELECT hash FROM audit_log ORDER BY seq DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let prev_hash = Digest::parse(prev_hash_text).unwrap();
        let aggregate_seq: u64 = conn
            .query_row(
                "SELECT COALESCE(MAX(aggregate_seq), 0) + 1 FROM audit_log WHERE aggregate_id = ?1",
                params![event.aggregate_id.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
            .try_into()
            .unwrap();
        let ts = Timestamp::now();
        let metadata = json!({
            "id": event.id.to_string(),
            "ts": ts.to_string(),
            "kind": event.kind.as_str(),
            "action": event.action,
            "decision": event.decision,
            "reason": event.reason,
            "task_grant_id": event.task_grant_id.map(|id| id.to_string()),
            "target_refs": event.target_refs,
            "payload_refs": event.payload_refs,
            "aggregate_id": event.aggregate_id,
            "aggregate_seq": aggregate_seq,
            "payload_json": event.payload_json,
        });
        let canonical = canonical_json(&metadata);
        let mut hasher = Sha256::new();
        hasher.update(prev_hash.as_str().as_bytes());
        hasher.update(canonical.as_bytes());
        let hash = digest_from_hash(hasher.finalize().into());
        let mut duplicate = event.clone();
        duplicate.ts = ts;
        duplicate.aggregate_seq = aggregate_seq;
        duplicate.prev_hash = prev_hash.clone();
        duplicate.hash = hash.clone();
        conn.execute(
            "INSERT INTO audit_log \
             (id, ts, kind, prev_hash, hash, meta_json, event_json, aggregate_id, aggregate_seq) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                duplicate.id.to_string(),
                duplicate.ts.to_string(),
                duplicate.kind.as_str(),
                duplicate.prev_hash.as_str(),
                duplicate.hash.as_str(),
                canonical,
                serde_json::to_string(&duplicate).unwrap(),
                duplicate.aggregate_id,
                i64::try_from(duplicate.aggregate_seq).unwrap(),
            ],
        )
        .unwrap();
    });
}
#[tokio::test]
async fn scheduled_reflection_miner_duplicate_decision_event_is_not_repeated() {
    let harness = miner_tick_harness().await;
    let (context, target_ref, scope_ref, _) =
        resolved_email_context(&harness.state, "thread-1", 'a', '1', Ulid::from(11_u128));
    let event = append_approval(&harness, &context, &target_ref, &scope_ref, 'f');
    append_duplicate_audit_row(&harness.state, &event);
    let miner_grant = find_active_grant_by_route(&harness.state, REFLECTION_SCHEDULED_MINER_ROUTE)
        .unwrap()
        .expect("scheduled miner grant must exist")
        .0;
    let entries = harness
        .state
        .store
        .load_owner_miner_audit_slice(
            harness.state.owner_principal_id.into(),
            &crate::grant_hmac_key().unwrap(),
            &format!("reflection:{}", miner_grant.id),
            openspine_schemas::event::DataClassification::Private,
        )
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].event_id, entries[1].event_id);
    assert_eq!(reflection_miner_tick(&harness.state).await.unwrap(), 0);
}

#[tokio::test]
async fn duplicate_audit_row_does_not_poison_distinct_approval_pattern() {
    let harness = miner_tick_harness().await;
    let (context, target_ref, scope_ref, _) =
        resolved_email_context(&harness.state, "thread-1", 'a', '1', Ulid::from(11_u128));
    let first = append_approval(&harness, &context, &target_ref, &scope_ref, 'f');
    append_duplicate_audit_row(&harness.state, &first);
    append_approval(&harness, &context, &target_ref, &scope_ref, '0');

    assert_eq!(reflection_miner_tick(&harness.state).await.unwrap(), 1);
}
#[tokio::test]
async fn approval_does_not_reuse_the_proposal_task_grant() {
    let harness = miner_tick_harness().await;
    let (context, target_ref, scope_ref, _) =
        resolved_email_context(&harness.state, "thread-1", 'a', '1', Ulid::from(11_u128));
    append_approval(&harness, &context, &target_ref, &scope_ref, 'f');
    append_approval(&harness, &context, &target_ref, &scope_ref, '0');
    assert_eq!(reflection_miner_tick(&harness.state).await.unwrap(), 1);

    let review_id: Ulid = harness.state.store.with_conn_for_test(|conn| {
        conn.query_row(
            "SELECT id FROM owner_reviews ORDER BY created_at DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
        .parse()
        .unwrap()
    });
    let review_row = harness
        .state
        .store
        .owner_review_row(review_id)
        .unwrap()
        .expect("miner tick must persist an owner review");
    let review: openspine_schemas::owner_review::OwnerReviewRequest = serde_json::from_slice(
        &harness
            .state
            .artifacts
            .get(&review_row.artifact_ref)
            .unwrap(),
    )
    .unwrap();
    let binding = review
        .evaluation_binding
        .as_ref()
        .expect("miner review must carry evaluation identity");
    let proposal_request = harness
        .state
        .store
        .find_action_request(binding.action_request_id)
        .unwrap()
        .expect("proposal activation request must persist");
    let proposal_grant_id = proposal_request.task_grant_id;
    let binding_digest = review.binding_digest();
    let outcome = crate::pipeline::owner_review_decision::submit_owner_review_decision_async(
        &harness.state,
        &crate::test_support::owner_surface(&harness.state),
        review.id,
        binding_digest,
        openspine_schemas::owner_review::DecisionIntent::Approve,
        None,
        Timestamp::now(),
    )
    .await
    .expect("owner approval must activate the evaluated proposal");
    assert!(matches!(
        outcome,
        crate::pipeline::owner_review_decision::OwnerReviewDecisionOutcome::Committed {
            replayed: false,
            ..
        }
    ));

    let activation_event: AuditEvent = {
        let event_json: String = harness.state.store.with_conn_for_test(|conn| {
            conn.query_row(
                "SELECT event_json FROM audit_log \
                 WHERE kind = 'artifact.activation_gated' ORDER BY seq DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap()
        });
        serde_json::from_str(&event_json).unwrap()
    };
    let activation_grant_id = activation_event
        .task_grant_id
        .expect("activation gate must name its grant");
    assert_ne!(
        activation_grant_id, proposal_grant_id,
        "owner approval must not reuse the proposal task grant"
    );
    let (activation_grant, _, _) = harness
        .state
        .store
        .find_task_grant_by_id(activation_grant_id)
        .unwrap()
        .expect("fresh activation grant must be persisted");
    assert_eq!(
        activation_grant.purpose,
        "owner_approved_artifact_activation"
    );
    assert_eq!(activation_grant.event_id, review.id);
}
#[tokio::test]
async fn miner_approval_refuses_review_proposal_digest_mismatch() {
    let harness = miner_tick_harness().await;
    let (context, target_ref, scope_ref, _) =
        resolved_email_context(&harness.state, "thread-1", 'a', '1', Ulid::from(11_u128));
    append_approval(&harness, &context, &target_ref, &scope_ref, 'f');
    append_approval(&harness, &context, &target_ref, &scope_ref, '0');
    assert_eq!(reflection_miner_tick(&harness.state).await.unwrap(), 1);

    let review_id: Ulid = harness.state.store.with_conn_for_test(|conn| {
        conn.query_row(
            "SELECT id FROM owner_reviews ORDER BY created_at DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
        .parse()
        .unwrap()
    });
    let review_row = harness
        .state
        .store
        .owner_review_row(review_id)
        .unwrap()
        .expect("miner tick must persist an owner review");
    let mut review: openspine_schemas::owner_review::OwnerReviewRequest = serde_json::from_slice(
        &harness
            .state
            .artifacts
            .get(&review_row.artifact_ref)
            .unwrap(),
    )
    .unwrap();
    review.id = Ulid::new();
    review.proposal_digest = Digest::parse(format!("sha256:{}", "9".repeat(64))).unwrap();
    let mut review_json = serde_json::to_value(&review).unwrap();
    review_json
        .as_object_mut()
        .unwrap()
        .remove("binding_digest");
    let rebound = openspine_schemas::digest::digest_of(&review_json);
    review_json.as_object_mut().unwrap().insert(
        "binding_digest".into(),
        serde_json::to_value(rebound).unwrap(),
    );
    let review: openspine_schemas::owner_review::OwnerReviewRequest =
        serde_json::from_value(review_json).unwrap();
    crate::pipeline::owner_review_surface::persist_owner_review(
        &harness.state,
        &review,
        harness.state.owner_principal_id,
        Timestamp::now() + jiff::SignedDuration::from_secs(review.limits.expires_after_secs),
        Timestamp::now(),
        None,
    )
    .unwrap();
    let binding_digest = review.binding_digest();
    let error = crate::pipeline::owner_review_decision::submit_owner_review_decision_async(
        &harness.state,
        &crate::test_support::owner_surface(&harness.state),
        review.id,
        binding_digest,
        openspine_schemas::owner_review::DecisionIntent::Approve,
        None,
        Timestamp::now(),
    )
    .await
    .expect_err("a review for a different proposal must be refused");
    assert!(matches!(
        error,
        crate::pipeline::owner_review_decision::OwnerReviewDecisionError::EvaluationBindingRefused(
            reason
        ) if reason.contains("proposed artifact identity")
    ));
}

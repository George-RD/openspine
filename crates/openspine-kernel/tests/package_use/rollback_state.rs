//! Historical authority fixture; owner decisions use the real binary.
use super::*;
pub(super) fn seed_approved_responsibility(fixture: &Fixture) -> (OwnerReviewRequest, String) {
    let principal: String = db(fixture)
        .query_row("SELECT id FROM principals LIMIT 1", [], |row| row.get(0))
        .unwrap();
    let catalog = action_catalog::canonical_catalog();
    let action = ActionId::new("email.create_draft");
    let context = ResolvedActionContext::try_new(
        &catalog,
        &action,
        &ActionImplementationId::new("gmail.draft.v1"),
        ResolvedActionContextInput {
            connector_instance_id: "gmail-primary".into(),
            account_role: Some(AccountRole::OwnerMailbox),
            account_identity_digest: Some(digest_of_bytes(b"account")),
            target_refs: vec![TargetRef {
                kind: TargetRefKind::EmailThread,
                id: Some("thread-1".into()),
            }],
            counterparty: Some(CounterpartyRef::Bound {
                identity_id: Ulid::from(11_u128),
                relationship: RelationshipKind::Client,
            }),
            bound_parameters: Default::default(),
            target_digest: Some(digest_of_bytes(b"target")),
            payload_digest: Some(digest_of_bytes(b"payload")),
            workflow_id: Some("draft_reply_workflow".into()),
            task_shape_digest: Some(digest_of_bytes(b"shape")),
        },
    )
    .unwrap();
    let review = OwnerReviewRequest::try_new(
        OwnerReviewRequestInput {
            id: Ulid::new(),
            schema_version: 1,
            review_version: 1,
            proposal_kind: ProposalKind::Responsibility,
            evidence: DelegationEvidence::ExplicitOwnerRequest {
                schema_version: 1,
                decision_event_id: Ulid::new(),
                owner_principal_id: principal.parse().unwrap(),
                request_digest: digest_of_bytes(b"owner request"),
            },
            title: "Prepare replies".into(),
            description: "Existing approved operator responsibility".into(),
            reviewed_scope: ReviewedActionScope::derive(&context).unwrap(),
            automatic_effects: vec!["Create a Gmail draft".into()],
            remaining_boundaries: vec!["Sending stays denied".into()],
            limits: ReviewLimits {
                quota: BudgetWindow {
                    max: 5,
                    window_secs: 604800,
                },
                rate: BudgetWindow {
                    max: 1,
                    window_secs: 3600,
                },
                expires_after_secs: 604800,
            },
            fallback_behavior: ReviewFallbackBehavior {
                scope_mismatch: BoundaryBehavior::RequireApproval,
                compatibility_drift: BoundaryBehavior::RequireApproval,
                budget_exhaustion: BoundaryBehavior::RequireApproval,
                timeout: BoundaryBehavior::Deny,
            },
            proposal_digest: digest_of_bytes(b"proposal"),
            compatibility_digest: context.compatibility_digest().clone(),
            available_decisions: BTreeSet::from([
                OwnerReviewDecision::Approve,
                OwnerReviewDecision::Reject,
                OwnerReviewDecision::Narrow,
            ]),
            lifecycle_controls: BTreeSet::from([
                ResponsibilityLifecycleControl::Pause,
                ResponsibilityLifecycleControl::Resume,
                ResponsibilityLifecycleControl::Expire,
                ResponsibilityLifecycleControl::Revoke,
            ]),
            evaluation_binding: None,
        },
        catalog
            .delegation_descriptor_for(&action)
            .unwrap()
            .delegation_policy
            .as_ref()
            .unwrap(),
    )
    .unwrap();
    let blob = put_system_fixture(fixture, &serde_json::to_vec(&review).unwrap());
    let now = Timestamp::now();
    db(fixture).execute("INSERT INTO owner_reviews (id, artifact_ref_digest, artifact_ref_schema_version, state, owner_principal_id, expires_at, created_at) VALUES (?1, ?2, 1, 'approved', ?3, ?4, ?5)", rusqlite::params![review.id.to_string(), blob, principal, (now + Duration::from_secs(3600)).as_nanosecond() as i64, now.as_nanosecond() as i64]).unwrap();
    let manifest = openspine_schemas::standing_rule::StandingRuleManifest {
        id: "operator-rule".into(),
        schema_version: 1,
        version: 1,
        lifecycle_state: openspine_schemas::artifact::Lifecycle::Active,
        action_id: action,
        description: "Existing approved operator responsibility".into(),
        quota: review.limits.quota,
        rate: review.limits.rate,
        expires_after_secs: review.limits.expires_after_secs,
        dark_window: None,
        reviewed_scope: Some(
            openspine_schemas::standing_rule::ReviewedScopeBinding::derive_from(
                review.reviewed_scope.clone(),
                context.compatibility_digest().clone(),
            ),
        ),
    };
    db(fixture).execute("INSERT INTO standing_rules (rule_id, artifact_id, version, action_id, rule_json, quota_max, quota_window_secs, rate_max, rate_window_secs, expires_after_secs, status, activated_at) VALUES ('operator-rule', 'operator-rule', 1, 'email.create_draft', ?1, 5, 604800, 1, 3600, 604800, 'active', ?2)", rusqlite::params![serde_json::to_string(&manifest).unwrap(), now.as_second()]).unwrap();
    db(fixture).execute("INSERT INTO owner_review_standing_rules (review_id, rule_id, rule_version) VALUES (?1, 'operator-rule', 1)", [review.id.to_string()]).unwrap();
    db(fixture).execute("INSERT INTO standing_rule_usage (rule_id, version, kind, used_at, status, reservation_id) VALUES ('operator-rule', 1, 'quota', ?1, 'committed', 'historic-effect')", [now.as_second()]).unwrap();
    seed_historical_activity(fixture, principal.parse().unwrap());
    let command = format!("/review {} {}", review.id, review.binding_digest());
    (review, command)
}

/// Seed encrypted fixture state using the documented on-disk format. Production
/// owner decisions below still decrypt/validate it through the real blob store.
fn put_system_fixture(fixture: &Fixture, plaintext: &[u8]) -> String {
    use aes_gcm::aead::{Aead, Payload};
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
    use rand::Rng as _;
    let env = fs::read_to_string(fixture.root.path().join("openspine.env")).unwrap();
    let hex = env
        .lines()
        .find_map(|line| line.strip_prefix("OPENSPINE_ARTIFACT_KEY="))
        .unwrap()
        .trim_matches(['\'', '"']);
    let master: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    let scope = Ulid::nil();
    let wrapped = fs::read(
        fixture
            .root
            .path()
            .join("data/keys")
            .join(scope.to_string()),
    )
    .unwrap();
    assert!(wrapped.starts_with(b"OSK1"));
    let mut aad = b"openspine.counterparty_key.v1".to_vec();
    aad.extend_from_slice(&scope.to_bytes());
    let key = Aes256Gcm::new_from_slice(&master)
        .unwrap()
        .decrypt(
            &Nonce::try_from(&wrapped[4..16]).unwrap(),
            Payload {
                msg: &wrapped[16..],
                aad: &aad,
            },
        )
        .unwrap();
    let mut header = vec![3];
    header.extend_from_slice(&scope.to_bytes());
    let mut nonce = [0_u8; 12];
    rand::rng().fill_bytes(&mut nonce);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .unwrap()
        .encrypt(
            &Nonce::try_from(nonce.as_slice()).unwrap(),
            Payload {
                msg: plaintext,
                aad: &header,
            },
        )
        .unwrap();
    let mut blob = header;
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&cipher);
    let digest = digest_of_bytes(plaintext);
    let directory = fixture
        .root
        .path()
        .join("data/artifacts")
        .join(scope.to_string());
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join(digest.as_str().strip_prefix("sha256:").unwrap()),
        blob,
    )
    .unwrap();
    digest.to_string()
}

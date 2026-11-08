//! The ordinary pure gate still requires approval after responsibility revocation.
use super::*;
use openspine_gate::{gate, ActionOrigin, GateContext, NoEgress};
use openspine_schemas::action::{ActionRequest, GateDecision};
use openspine_schemas::approval::{ApprovalDecision, ApprovalRecord, TimeoutBehavior};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::selection::{
    SelectionScope, SelectionToken, SelectionTokenType, SelectionVerificationMethod,
};
use rusqlite::OptionalExtension as _;

pub(super) fn seed_historical_activity(fixture: &Fixture, principal: Ulid) {
    let now: Timestamp = "2020-01-01T00:00:00Z".parse().unwrap();
    let request = ActionRequest {
        id: "01ARZ3NDEKTSV4RRFFQ69G5FAZ".parse().unwrap(),
        task_grant_id: Ulid::new(),
        action: ActionId::new("email.create_draft"),
        target_ref: Some(TargetRef {
            kind: TargetRefKind::EmailThread,
            id: Some("thread-1".into()),
        }),
        payload_ref: Some(ArtifactRef {
            digest: digest_of_bytes(b"past draft"),
            schema_version: 1,
        }),
        target_digest: Some(digest_of_bytes(b"past target")),
        selection_token_id: None,
        params: Default::default(),
        skill_attribution: None,
        requested_at: now,
        schema_version: 1,
    };
    let approval = ApprovalRecord {
        id: Ulid::new(),
        schema_version: 1,
        action_request_id: request.id,
        approved_by: principal.into(),
        approved_at: now,
        approved_payload_digest: request.payload_ref.as_ref().unwrap().digest.clone(),
        approved_target_digest: request.target_digest.clone().unwrap(),
        expires_at: now + Duration::from_secs(600),
        decision: ApprovalDecision::Approved,
        timeout_behavior: TimeoutBehavior::DoNothing,
        approval_channel: "terminal".into(),
    };
    db(fixture)
        .execute(
            "INSERT INTO action_requests (id, request_json, used) VALUES (?1, ?2, 1)",
            rusqlite::params![
                request.id.to_string(),
                serde_json::to_string(&request).unwrap()
            ],
        )
        .unwrap();
    db(fixture)
        .execute(
            "INSERT INTO approvals (id, action_request_id, approval_json) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                approval.id.to_string(),
                request.id.to_string(),
                serde_json::to_string(&approval).unwrap()
            ],
        )
        .unwrap();
    db(fixture).execute("INSERT INTO daily_spend (day, model_calls, connector_calls, alert_state) VALUES ('2020-01-01', 7, 2, 0)", []).unwrap();
}

struct Context<'a> {
    fixture: &'a Fixture,
    token: SelectionToken,
    key: Vec<u8>,
}

impl GateContext for Context<'_> {
    fn approval_for_request(&self, id: Ulid) -> Option<ApprovalRecord> {
        let json: Option<String> = db(self.fixture)
            .query_row(
                "SELECT approval_json FROM approvals WHERE action_request_id = ?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .unwrap();
        json.map(|s| serde_json::from_str(&s).unwrap())
    }
    fn find_selection_token(&self, id: Ulid) -> Option<SelectionToken> {
        (id == self.token.id).then(|| self.token.clone())
    }
    fn grant_hmac_key(&self) -> Option<Vec<u8>> {
        Some(self.key.clone())
    }
}

pub(super) fn assert_effect_still_requires_approval(fixture: &Fixture) {
    assert_eq!(count(fixture, "SELECT COUNT(*) FROM standing_rules WHERE action_id = 'email.create_draft' AND status = 'active'"), 0);
    let json: String = db(fixture)
        .query_row(
            "SELECT grant_json FROM task_grants ORDER BY rowid DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut grant: TaskGrant = serde_json::from_str(&json).unwrap();
    let now = Timestamp::now();
    // A new, correctly authenticated request, not a stale/expired grant denial.
    grant.id = Ulid::new();
    grant.root_grant_id = grant.id;
    grant.issued_at = now;
    grant.expires_at = now + Duration::from_secs(60);
    let action = ActionId::new("email.create_draft");
    grant.allowed_actions = vec![action.clone()];
    grant.approval_required_actions = vec![action.clone()];
    grant.denied_actions.clear();
    let catalog = action_catalog::canonical_catalog();
    let egress = catalog.egress_decl_for(&action).unwrap();
    grant.allowed_egress_classes = egress.egress_class.into_iter().collect();
    grant.output_channels = egress.output_channels.clone().unwrap_or_default();
    let token = SelectionToken {
        id: Ulid::new(),
        schema_version: 1,
        token_type: SelectionTokenType::email_thread_selection(),
        user: grant.user,
        target_id: "thread-1".into(),
        selected_by: grant.user,
        selected_at: now,
        issued_by: "kernel".into(),
        expires_at: grant.expires_at,
        verified_source: true,
        verification_method: SelectionVerificationMethod::KernelUiSelection,
        connector: Some(openspine_schemas::event::Connector::GmailPrimaryConnector),
        account_role: Some(AccountRole::OwnerMailbox),
        scope: SelectionScope {
            read_thread: true,
            attachments_allowed: false,
            max_messages: 20,
            include_headers: true,
            include_recipients: true,
            include_body: true,
        },
        single_use: true,
    };
    grant.selection_tokens = vec![token.id];
    let env = fs::read_to_string(fixture.root.path().join("openspine.env")).unwrap();
    let key = env
        .lines()
        .find_map(|s| s.strip_prefix("OPENSPINE_GRANT_HMAC_KEY="))
        .unwrap()
        .trim_matches(['\'', '"'])
        .as_bytes()
        .to_vec();
    grant.seal_root(&key);
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action,
        target_ref: Some(TargetRef {
            kind: TargetRefKind::EmailThread,
            id: Some("thread-1".into()),
        }),
        payload_ref: Some(ArtifactRef {
            digest: digest_of_bytes(b"past draft"),
            schema_version: 1,
        }),
        target_digest: Some(digest_of_bytes(b"past target")),
        selection_token_id: Some(token.id),
        params: Default::default(),
        skill_attribution: None,
        requested_at: now,
        schema_version: 1,
    };
    let context = Context {
        fixture,
        token,
        key,
    };
    let outcome = gate(
        &grant,
        &request,
        ActionOrigin::Shell,
        &context,
        &catalog,
        &NoEgress,
        now,
    );
    assert!(matches!(outcome.decision, GateDecision::ApprovalRequired { .. }), "revoked reusable authority and historical consumed approval must not admit a new effect: {:?}", outcome.decision);
}

//! Shared harness for the #128 scope-matched admission acceptance suite:
//! a mocked Gmail provider, a draft-capable grant with a briefcase, and the
//! durable assertions the tests read. Split from the test modules so every
//! file stays under the 500-line gate.

use jiff::Timestamp;
use openspine_schemas::action::{ActionId, GateDecision};
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::briefcase::{
    Briefcase, CounterpartyRef, RelationshipTier, TaskClass, TaskShape,
};
use openspine_schemas::digest::{canonical_json, Digest};
use openspine_schemas::event::{AccountRole, Connector};
use openspine_schemas::grant::{GrantLimits, TaskGrant};
use openspine_schemas::identity::RelationshipKind;
use openspine_schemas::resolved_context::ResolvedActionContext;
use openspine_schemas::reviewed_scope::ReviewedActionScope;
use openspine_schemas::selection::{
    SelectionScope, SelectionToken, SelectionTokenType, SelectionVerificationMethod,
};
use openspine_schemas::standing_rule::{BudgetWindow, ReviewedScopeBinding, StandingRuleManifest};
use rusqlite::params;
use serde_json::{json, Value};
use std::time::Duration;
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{resolve_scoped_admission, ScopedAdmission};
use crate::api::actions::{mediate_and_dispatch_action, FailureSurface};
use crate::gmail::GmailConnector;
use crate::pipeline::AppState;
use crate::store::standing_rules_tests::manifest;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::test_state_with_gmail_and_telegram;

pub(super) const CHAT_ID: i64 = 555;
pub(super) const OWNER_MAILBOX: &str = "owner@example.com";

/// A live provider environment: mocked Gmail OAuth + API and a mocked
/// Telegram endpoint (the executor's owner notification is best-effort but
/// must not reach the real network).
pub(crate) struct DraftEnv {
    pub(crate) state: AppState,
    pub(crate) api_server: MockServer,
    _token_server: MockServer,
    _telegram_server: MockServer,
}

pub(super) fn thread_body(from: &str) -> Value {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    json!({
        "messages": [{
            "payload": {
                "mimeType": "multipart/mixed",
                "headers": [
                    {"name": "From", "value": from},
                    {"name": "Subject", "value": "Re: invoice"},
                ],
                "parts": [{
                    "mimeType": "text/plain",
                    "body": {"data": URL_SAFE_NO_PAD.encode(b"hello owner")},
                }],
            },
        }],
    })
}

pub(super) async fn draft_env_with_mailbox(mailbox: &str, threads: &[&str]) -> DraftEnv {
    let token_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "test-access-token",
            "expires_in": 3600,
        })))
        .mount(&token_server)
        .await;
    let api_server = MockServer::start().await;
    for thread_id in threads {
        Mock::given(method("GET"))
            .and(path(format!("/gmail/v1/users/me/threads/{thread_id}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(thread_body("alice@example.com")),
            )
            .mount(&api_server)
            .await;
    }
    let telegram_server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": CHAT_ID, "type": "private"},
                "text": "ok"
            }
        })))
        .mount(&telegram_server)
        .await;
    let gmail = GmailConnector::new(
        "client-id".to_string(),
        "client-secret".to_string(),
        "refresh-token".to_string(),
        mailbox.to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), api_server.uri());
    let state = test_state_with_gmail_and_telegram(
        gmail,
        TelegramConnector::with_api_url(
            "test-token".into(),
            telegram_server.uri().parse().unwrap(),
        ),
    );
    DraftEnv {
        state,
        api_server,
        _token_server: token_server,
        _telegram_server: telegram_server,
    }
}

pub(crate) async fn draft_env(threads: &[&str]) -> DraftEnv {
    draft_env_with_mailbox(OWNER_MAILBOX, threads).await
}

/// Mount the Gmail drafts endpoint. `body` is the provider response.
pub(super) async fn mount_drafts(server: &MockServer, status: u16, body: Value) {
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}

pub(super) async fn drafts_written(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|req| req.url.path() == "/gmail/v1/users/me/drafts")
        .count()
}

/// A grant that needs owner approval for `email.create_draft`, carrying a
/// kernel-authored selection token for `thread_id` and a briefcase whose task
/// shape binds an identity-resolved counterparty.
pub(crate) fn mint_draft_grant(state: &AppState, thread_id: &str) -> TaskGrant {
    let now = Timestamp::now();
    let user: openspine_schemas::ids::PrincipalId = state.owner.principal_id;
    let token = SelectionToken {
        id: Ulid::new(),
        schema_version: 1,
        token_type: SelectionTokenType::email_thread_selection(),
        user,
        target_id: thread_id.to_string(),
        selected_by: user,
        selected_at: now,
        issued_by: "kernel".to_string(),
        expires_at: now + Duration::from_secs(300),
        verified_source: true,
        verification_method: SelectionVerificationMethod::ApprovedOwnerControlSelection,
        connector: Some(Connector::GmailPrimaryConnector),
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
    state.store.insert_selection_token(&token).unwrap();

    let mut grant = TaskGrant {
        id: Ulid::new(),
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user,
        purpose: "selected_thread_email_reply_draft".to_string(),
        issued_by: "kernel".to_string(),
        issued_at: now,
        expires_at: now + Duration::from_secs(300),
        event_id: Ulid::new(),
        route_id: "owner_email_selected_thread".to_string(),
        agent_id: "email_reply_drafter".to_string(),
        workflow_id: "selected_thread_email_reply_draft".to_string(),
        capability_pack_id: "selected_thread_email_draft_pack".to_string(),
        authority_sources: vec![],
        selection_tokens: vec![token.id],
        allowed_actions: vec![],
        approval_required_actions: vec![ActionId::new("email.create_draft")],
        denied_actions: vec![],
        allowed_egress_classes: vec![],
        output_channels: vec!["telegram.owner.reply".to_string()],
        limits: GrantLimits {
            max_model_calls: 8,
            max_artifacts: 20,
            max_runtime_seconds: 300,
        },
        task_token: Ulid::new().to_string(),
        root_grant_id: Ulid::nil(),
        parent_grant_id: None,
        mode: openspine_schemas::grant::GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
        persona_id: None,
    };
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    let pending_ref = state.artifacts.put(b"test pending".as_slice()).unwrap();
    state
        .store
        .insert_task_grant(
            &grant,
            &pending_ref,
            &crate::test_support::owner_surface_for(state, CHAT_ID),
        )
        .unwrap();
    let briefcase = Briefcase {
        schema_version: 1,
        task_shape: TaskShape {
            route_id: grant.route_id.clone(),
            workflow_id: grant.workflow_id.clone(),
            counterparty: CounterpartyRef::Bound {
                identity_id: Ulid::from(11_u128),
                relationship: RelationshipKind::Client,
            },
        },
        source_snapshot_id: Digest::parse(format!("sha256:{}", "0".repeat(64))).unwrap(),
        depth: 1,
        tier: RelationshipTier::Known,
        class: TaskClass::Conversation,
        sections: vec![],
        top_up_log: vec![],
    };
    state.store.insert_briefcase(grant.id, &briefcase).unwrap();
    grant
}

pub(super) fn draft_payload() -> Value {
    json!({"subject": "Re: invoice", "body": "Thanks — attached."})
}

/// Resolve the context the production path will resolve for this grant. Used
/// to mint a rule bound to exactly that reviewed scope; the request under
/// test then re-resolves it independently.
pub(crate) async fn resolved_context(state: &AppState, grant: &TaskGrant) -> ResolvedActionContext {
    let payload_ref = state
        .artifacts
        .put(canonical_json(&draft_payload()).as_bytes())
        .unwrap();
    let admission = resolve_scoped_admission(
        state,
        grant,
        &ActionId::new("email.create_draft"),
        Some(&payload_ref),
        Timestamp::now(),
    )
    .await
    .expect("resolution must not error");
    match admission {
        ScopedAdmission::Resolved(resolved) => resolved.context.clone(),
        ScopedAdmission::NotApplicable => panic!("email.create_draft is scope-eligible"),
        ScopedAdmission::Unresolvable => panic!("context must resolve for a complete grant"),
    }
}

pub(crate) fn scoped_manifest(
    rule_id: &str,
    context: &ResolvedActionContext,
) -> StandingRuleManifest {
    let scope = ReviewedActionScope::derive(context).expect("required dimensions are all valued");
    let mut m = manifest(
        rule_id,
        "email.create_draft",
        7 * 24 * 3600,
        BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 3,
            window_secs: 3600,
        },
        None,
    );
    m.reviewed_scope = Some(ReviewedScopeBinding::derive_from(
        scope,
        context.compatibility_digest().clone(),
    ));
    m
}

pub(crate) fn usage_count(state: &AppState, rule_id: &str, status: &str) -> i64 {
    state.store.with_conn_for_test(|conn| {
        conn.query_row(
            "SELECT COUNT(DISTINCT reservation_id) FROM standing_rule_usage \
             WHERE rule_id = ?1 AND status = ?2",
            params![rule_id, status],
            |row| row.get(0),
        )
        .unwrap()
    })
}

pub(super) fn scheduled_timer_count(state: &AppState) -> i64 {
    state.store.with_conn_for_test(|conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM standing_rule_pending_actions",
            [],
            |row| row.get(0),
        )
        .unwrap()
    })
}

pub(super) fn audit_count(state: &AppState, event: &str) -> i64 {
    state.store.with_conn_for_test(|conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM audit_log WHERE kind = ?1",
            params![event],
            |row| row.get(0),
        )
        .unwrap()
    })
}

pub(super) fn digest_count(state: &AppState) -> usize {
    state.store.owner_digest_items().unwrap().len()
}

pub(super) async fn dispatch(state: &AppState, grant: &TaskGrant) -> (GateDecision, Option<u32>) {
    let result = mediate_and_dispatch_action(
        state,
        grant,
        ActionId::new("email.create_draft"),
        &crate::test_support::owner_surface_for(state, CHAT_ID),
        Some(&draft_payload()),
        FailureSurface::DirectResponse,
        None,
    )
    .await;
    match result {
        Ok((decision, _, _, budget)) => (decision, budget.map(|b| b.quota_remaining)),
        Err(err) => panic!("mediation returned a transport error: {err:?}"),
    }
}

pub(super) fn mint_draft_grant_with_counterparty(
    state: &AppState,
    thread_id: &str,
    identity_id: Ulid,
) -> TaskGrant {
    let grant = mint_draft_grant(state, thread_id);
    let briefcase = Briefcase {
        schema_version: 1,
        task_shape: TaskShape {
            route_id: grant.route_id.clone(),
            workflow_id: grant.workflow_id.clone(),
            counterparty: CounterpartyRef::Bound {
                identity_id,
                relationship: RelationshipKind::Client,
            },
        },
        source_snapshot_id: Digest::parse(format!("sha256:{}", "0".repeat(64))).unwrap(),
        depth: 1,
        tier: RelationshipTier::Known,
        class: TaskClass::Conversation,
        sections: vec![],
        top_up_log: vec![],
    };
    state.store.with_conn_for_test(|conn| {
        conn.execute(
            "UPDATE briefcases SET briefcase_json = ?2 WHERE task_grant_id = ?1",
            params![
                grant.id.to_string(),
                serde_json::to_string(&briefcase).unwrap()
            ],
        )
        .unwrap();
    });
    grant
}

/// Insert a standing-rule row exactly as a pre-#128 database would hold it:
/// active, action-keyed, and carrying no reviewed-scope binding in either the
/// manifest JSON or the two fast-path digest columns. Activation refuses such
/// a rule now, so this is the only way to reach the legacy state the matcher
/// still has to refuse.
pub(super) fn insert_legacy_unbounded_rule(state: &AppState, manifest: &StandingRuleManifest) {
    let activated = Timestamp::now()
        .as_nanosecond()
        .try_into()
        .map(|nanos: i64| nanos)
        .expect("timestamp fits i64 nanoseconds");
    state.store.with_conn_for_test(|conn| {
        conn.execute(
            "INSERT INTO standing_rules (
                rule_id, artifact_id, version, action_id, rule_json,
                quota_max, quota_window_secs, rate_max, rate_window_secs,
                expires_after_secs, dark_window_timeout_secs, dark_window_default,
                status, activated_at, last_used_at, revoked_at, needs_review_since,
                reviewed_scope_digest, compatibility_digest
             ) VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL,
                       'active', ?10, NULL, NULL, NULL, NULL, NULL)",
            rusqlite::params![
                manifest.id,
                manifest.version as i64,
                manifest.action_id.to_string(),
                serde_json::to_string(manifest).unwrap(),
                manifest.quota.max as i64,
                manifest.quota.window_secs,
                manifest.rate.max as i64,
                manifest.rate.window_secs,
                manifest.expires_after_secs,
                activated,
            ],
        )
        .unwrap();
    });
}
/// A thread whose messages come from two distinct senders — the newest (and
/// therefore the reply recipient) is `newest`, with `older` also a participant.
pub(super) fn two_party_thread_body(newest: &str, older: &str) -> Value {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    let message = |from: &str| {
        json!({
            "payload": {
                "mimeType": "multipart/mixed",
                "headers": [
                    {"name": "From", "value": from},
                    {"name": "Subject", "value": "Re: invoice"},
                ],
                "parts": [{
                    "mimeType": "text/plain",
                    "body": {"data": URL_SAFE_NO_PAD.encode(b"hello owner")},
                }],
            },
        })
    };
    json!({ "messages": [message(newest), message(older)] })
}

// openspine:allow-large-module reason: persona grant field update kept dispatch coverage together
//! End-to-end tests for `email.read_thread:selected_no_attachments`
//! (build plan Step 5). These exercises go through the real axum router and
//! the real SQLite store to prove the single-use selection token, grant
//! binding, expiry, and payload validation all surface as HTTP-level 400s.
//! `lyra.ui.preview`'s tests live in the sibling `preview_tests` module —
//! split out purely to keep both files under the 500-line gate — and reuse
//! [`mint_grant_with_selection_token`] and [`OWNER_CHAT_ID`] from here.

use crate::api::actions::DispatchError;
use crate::connector_reality::BreakerState;
use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::briefcase::{
    Briefcase, BriefcaseSection, CounterpartyRef, RelationshipTier, TaskClass, TaskShape,
};
use openspine_schemas::digest::Digest;
use openspine_schemas::egress::EgressClass;
use openspine_schemas::event::{AccountRole, Connector};
use openspine_schemas::grant::{GrantLimits, TaskGrant};
use openspine_schemas::identity::RelationshipKind;
use openspine_schemas::selection::{
    SelectionScope, SelectionToken, SelectionTokenType, SelectionVerificationMethod,
};
use serde_json::{json, Value};
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::tests::{post_action, start_server};
use crate::gmail::GmailConnector;
use crate::test_support::fixtures::{test_state, test_state_with_gmail};

pub(crate) const OWNER_CHAT_ID: i64 = 555;

fn sample_gmail_thread_json() -> Value {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    json!({
        "messages": [{
            "payload": {
                "mimeType": "multipart/mixed",
                "headers": [
                    {"name": "From", "value": "alice@example.com"},
                    {"name": "Subject", "value": "Re: invoice"},
                ],
                "parts": [
                    {
                        "mimeType": "text/plain",
                        "body": {"data": URL_SAFE_NO_PAD.encode(b"hello owner")},
                    },
                    {
                        "mimeType": "application/pdf",
                        "filename": "invoice.pdf",
                        "body": {"data": URL_SAFE_NO_PAD.encode(b"not-a-real-pdf")},
                    },
                ],
            },
        }],
    })
}

async fn mount_gmail_token_endpoint(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "test-access-token",
            "expires_in": 3600,
        })))
        .mount(server)
        .await;
}

async fn mount_gmail_thread_endpoint(server: &MockServer, thread_id: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/gmail/v1/users/me/threads/{}", thread_id)))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_gmail_thread_json()))
        .mount(server)
        .await;
}

fn gmail_connector(token_server: &MockServer, api_server: &MockServer) -> GmailConnector {
    GmailConnector::new(
        "client-id".to_string(),
        "client-secret".to_string(),
        "refresh-token".to_string(),
        "owner@example.com".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), api_server.uri())
}

pub(crate) fn mint_grant_with_selection_token(
    state: &crate::pipeline::AppState,
    allowed_actions: &[&str],
    token_expires_at: Timestamp,
) -> (TaskGrant, SelectionToken) {
    mint_grant_with_selection_token_egress(state, allowed_actions, &[], token_expires_at)
}

/// Variant that seals the grant with a specific set of allowed egress classes,
/// so a test can admit a rated egress action (e.g. `email.send`, rated
/// `DirectMessage`) past the gate's egress-class coverage check (gate.rs:226).
pub(crate) fn mint_grant_with_selection_token_egress(
    state: &crate::pipeline::AppState,
    allowed_actions: &[&str],
    allowed_egress_classes: &[EgressClass],
    token_expires_at: Timestamp,
) -> (TaskGrant, SelectionToken) {
    let now = Timestamp::now();
    let user = state.owner_user_id.to_string();
    let token = SelectionToken {
        id: Ulid::new(),
        schema_version: 1,
        token_type: SelectionTokenType::email_thread_selection(),
        user: user.clone(),
        target_id: "thread-1".to_string(),
        selected_by: user.clone(),
        selected_at: now,
        issued_by: "kernel".to_string(),
        expires_at: token_expires_at,
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
        expires_at: now + std::time::Duration::from_secs(120),
        event_id: Ulid::new(),
        route_id: "owner_email_selected_thread".to_string(),
        agent_id: "email_reply_drafter".to_string(),
        workflow_id: "selected_thread_email_reply_draft".to_string(),
        capability_pack_id: "selected_thread_email_draft_pack".to_string(),
        authority_sources: vec![],
        selection_tokens: vec![token.id],
        allowed_actions: allowed_actions.iter().map(|a| ActionId::new(*a)).collect(),
        approval_required_actions: vec![],
        denied_actions: vec![],
        allowed_egress_classes: allowed_egress_classes.to_vec(),
        output_channels: vec!["telegram.owner.reply".to_string()],
        limits: GrantLimits {
            max_model_calls: 8,
            max_artifacts: 20,
            max_runtime_seconds: 120,
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
            &crate::test_support::telegram_surface(OWNER_CHAT_ID),
        )
        .unwrap();

    (grant, token)
}

/// Insert a briefcase with a bound counterparty and NO classified (non-public)
/// sections, so a rated egress action's disclosure hook derives empty
/// provenance and passes trivially — leaving the post-gate executor lookup as
/// the next boundary. Used to admit `email.send` fully in the no-executor
/// tests without seeding any disclosure policy.
pub(crate) fn insert_bound_briefcase_without_classified_sections(
    state: &crate::pipeline::AppState,
    grant: &TaskGrant,
    relationship: RelationshipKind,
) {
    insert_bound_briefcase_with_sections(state, grant, relationship, vec![]);
}

/// Insert a briefcase with a bound counterparty and the given sections, so a
/// rated egress action's disclosure hook derives provenance from exactly those
/// classified sections.
pub(crate) fn insert_bound_briefcase_with_sections(
    state: &crate::pipeline::AppState,
    grant: &TaskGrant,
    relationship: RelationshipKind,
    sections: Vec<BriefcaseSection>,
) {
    let briefcase = Briefcase {
        schema_version: 1,
        task_shape: TaskShape {
            route_id: grant.route_id.clone(),
            workflow_id: grant.workflow_id.clone(),
            counterparty: CounterpartyRef::Bound {
                identity_id: Ulid::from(11_u128),
                relationship,
            },
        },
        source_snapshot_id: Digest::parse(format!("sha256:{}", "0".repeat(64))).unwrap(),
        depth: 1,
        tier: RelationshipTier::Known,
        class: TaskClass::Conversation,
        sections,
        top_up_log: vec![],
    };
    state.store.insert_briefcase(grant.id, &briefcase).unwrap();
}

#[tokio::test]
async fn email_read_selected_thread_returns_thread_via_mocked_gmail() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    mount_gmail_token_endpoint(&token_server).await;
    mount_gmail_thread_endpoint(&api_server, "thread-1").await;

    let state = test_state_with_gmail(gmail_connector(&token_server, &api_server));
    let (grant, token) = mint_grant_with_selection_token(
        &state,
        &["email.read_thread:selected_no_attachments"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );

    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({ "selection_token_id": token.id.to_string() })),
    )
    .await;
    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "allow");
    assert_eq!(body["result"]["thread_id"], "thread-1");
    let messages = body["result"]["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["from"], "alice@example.com");
    assert_eq!(messages[0]["subject"], "Re: invoice");
    assert_eq!(messages[0]["body_text"], "hello owner");
    assert!(!messages[0]["body_text"]
        .as_str()
        .unwrap()
        .contains("not-a-real-pdf"));

    handle.abort();
}

#[tokio::test]
async fn email_read_selected_thread_rejects_second_use() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    mount_gmail_token_endpoint(&token_server).await;
    mount_gmail_thread_endpoint(&api_server, "thread-1").await;

    let state = test_state_with_gmail(gmail_connector(&token_server, &api_server));
    let (grant, token) = mint_grant_with_selection_token(
        &state,
        &["email.read_thread:selected_no_attachments"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );

    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({ "selection_token_id": token.id.to_string() })),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "allow");

    let resp = post_action(
        addr,
        &grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({ "selection_token_id": token.id.to_string() })),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "selection token has already been used");

    handle.abort();
}

#[tokio::test]
async fn email_read_selected_thread_rejects_foreign_grant() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    mount_gmail_token_endpoint(&token_server).await;
    mount_gmail_thread_endpoint(&api_server, "thread-1").await;

    let state = test_state_with_gmail(gmail_connector(&token_server, &api_server));
    let (grant, token) = mint_grant_with_selection_token(
        &state,
        &["email.read_thread:selected_no_attachments"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );

    let mut foreign_grant = grant.clone();
    foreign_grant.id = Ulid::new();
    foreign_grant.root_grant_id = foreign_grant.id;
    foreign_grant.parent_grant_id = None;
    foreign_grant.task_token = Ulid::new().to_string();
    foreign_grant.selection_tokens = vec![];
    // Re-seal so chain_valid passes; denial must come from the unbound token.
    foreign_grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    let pending_ref = state.artifacts.put(b"foreign pending".as_slice()).unwrap();
    state
        .store
        .insert_task_grant(
            &foreign_grant,
            &pending_ref,
            &crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID),
        )
        .unwrap();

    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &foreign_grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({ "selection_token_id": token.id.to_string() })),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "deny");
    assert_eq!(body["decision"]["reason"], "selection_token_invalid");

    handle.abort();
}

#[tokio::test]
async fn email_read_selected_thread_rejects_expired_token() {
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    mount_gmail_token_endpoint(&token_server).await;
    mount_gmail_thread_endpoint(&api_server, "thread-1").await;

    let state = test_state_with_gmail(gmail_connector(&token_server, &api_server));
    let (grant, token) = mint_grant_with_selection_token(
        &state,
        &["email.read_thread:selected_no_attachments"],
        Timestamp::now() - std::time::Duration::from_secs(1),
    );

    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({ "selection_token_id": token.id.to_string() })),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "deny");
    assert_eq!(body["decision"]["reason"], "selection_token_invalid");

    handle.abort();
}

#[tokio::test]
async fn email_read_selected_thread_rejects_malformed_payload() {
    let state = test_state_with_gmail(GmailConnector::new(
        "client-id".to_string(),
        "client-secret".to_string(),
        "refresh-token".to_string(),
        "owner@example.com".to_string(),
    ));
    let (grant, _token) = mint_grant_with_selection_token(
        &state,
        &["email.read_thread:selected_no_attachments"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );

    let (addr, handle) = start_server(state).await;

    // An empty payload presents no selection token. `email.read_thread:
    // selected_no_attachments` is token-requiring, so the pure gate denies
    // it (D-055.1) — a `200` with `GateDecision::Deny`, not a `400`.
    let resp = post_action(
        addr,
        &grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({})),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "deny");
    assert_eq!(body["decision"]["reason"], "selection_token_invalid");

    // A non-ULID `selection_token_id` cannot be parsed, so no token is
    // presented to the gate — same deny as a missing token.
    let resp = post_action(
        addr,
        &grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({ "selection_token_id": "not-a-ulid" })),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "deny");
    assert_eq!(body["decision"]["reason"], "selection_token_invalid");

    // D-036: the shell has no way to name a thread directly — only a
    // selection token it was handed. A `thread_id` field presents no
    // `selection_token_id`, so the gate denies it the same way as any other
    // payload missing the required token, never silently ignored.
    let resp = post_action(
        addr,
        &grant.task_token,
        "email.read_thread:selected_no_attachments",
        Some(json!({ "thread_id": "thread-1" })),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "deny");
    assert_eq!(body["decision"]["reason"], "selection_token_invalid");

    handle.abort();
}

#[tokio::test]
async fn unregistered_known_action_returns_stub_shape() {
    // `memory.read:owner_preferences_limited` is a known catalog action (so a
    // grant may authorize it and the gate allows it) but has no Step 4
    // kernel-side handler registered. The dispatcher must return the honest
    // stub shape rather than erroring.
    let state = test_state();
    let (grant, _token) = mint_grant_with_selection_token(
        &state,
        &["memory.read:owner_preferences_limited"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );

    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &grant.task_token,
        "memory.read:owner_preferences_limited",
        None,
    )
    .await;
    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "allow");
    assert!(body["result"]["stub"].as_bool().unwrap());
    assert_eq!(
        body["result"]["note"].as_str().unwrap(),
        "memory.read:owner_preferences_limited has no Step 4 kernel-side implementation yet"
    );

    handle.abort();
}

/// Every catalogued effect id outside the explicit non-effect READ allowlist
/// must fail closed after the grant has allowed it. These six actions all use
/// the post-gate dispatch boundary (not a gate denial): the custom grant
/// explicitly allows each one, so a registry miss is surfaced as HTTP 500
/// `internal_error`, never as the old successful stub.
#[tokio::test]
async fn unregistered_effect_actions_fail_closed_without_stub() {
    for action in [
        // Counterparty-facing AND egress-rated (DirectMessage). Admitted fully
        // below (egress class granted + an empty-provenance briefcase so the
        // disclosure hook passes) so the test still pins the post-gate
        // NoExecutor boundary rather than an egress-class denial.
        "email.send",
        // These mutations are also explicitly allowed and have no egress or
        // selection-token gate constraint; each must reach NoExecutor.
        "coolify.deploy",
        "briefcase.topup",
        "artifact.write:task_scratch",
        // These formerly hit successful placeholder handlers. Their grants
        // prove gate() returns Allow before the registry miss is exercised.
        "workflow.invoke:approved",
        "setup.workflow.start",
    ] {
        let state = test_state();
        let store = state.store.clone();
        let (grant, _token) = if action == "email.send" {
            let minted = mint_grant_with_selection_token_egress(
                &state,
                &[action],
                &[EgressClass::DirectMessage],
                Timestamp::now() + std::time::Duration::from_secs(120),
            );
            insert_bound_briefcase_without_classified_sections(
                &state,
                &minted.0,
                RelationshipKind::Client,
            );
            minted
        } else {
            mint_grant_with_selection_token(
                &state,
                &[action],
                Timestamp::now() + std::time::Duration::from_secs(120),
            )
        };

        let (addr, handle) = start_server(state).await;
        let resp = post_action(addr, &grant.task_token, action, None).await;
        assert_eq!(
            resp.status(),
            500,
            "{action} should fail at post-gate dispatch, not return a stub"
        );

        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["error"], "internal_error", "{action}");
        assert!(
            body.get("result")
                .and_then(|result| result.get("stub"))
                .is_none(),
            "{action} response must not contain result.stub: {body}"
        );

        let gated_allows = store
            .all_audit_event_jsons()
            .unwrap()
            .into_iter()
            .filter_map(|raw| serde_json::from_str::<Value>(&raw).ok())
            .any(|event| {
                event["kind"] == "action.gated"
                    && event["action"] == action
                    && event["decision"]["outcome"] == "allow"
            });
        assert!(
            gated_allows,
            "{action} must reach dispatch only after action.gated Allow"
        );

        handle.abort();
    }
}

#[tokio::test]
async fn unknown_action_is_not_stub_eligible() {
    let state = test_state();
    let (grant, _token) = mint_grant_with_selection_token(
        &state,
        &["unknown.action:not_catalogued"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );
    let action = ActionId::new("unknown.action:not_catalogued");

    let result = super::connector_breaker::dispatch_allowed_action(
        &state,
        &grant,
        &action,
        &crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID),
        None,
    )
    .await;
    assert_eq!(
        result.disposition,
        crate::api::effect_executors::EffectDisposition::NotAttempted,
        "an unknown action never polls a write future: NotAttempted"
    );
    let error = match result.result {
        Err(error) => error,
        Ok(value) => panic!("unknown action must not return a result or stub: {value}"),
    };
    assert!(
        matches!(&error, DispatchError::NoExecutor(id) if id == &action),
        "unknown action must fail closed with NoExecutor, got {error:?}"
    );
}

/// AD-103/AD-141 acceptance: an Open gmail circuit breaker blocks the effect
/// AFTER `gate()` has authorized it, emitting the distinct
/// `connector_unavailable` audit event — operational failure, never a policy
/// denial. No Gmail network call is made.
#[tokio::test]
async fn open_gmail_breaker_blocks_effect_with_connector_unavailable_event() {
    use crate::api::actions::{mediate_and_dispatch_action, DispatchError, FailureSurface};

    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    // No mocks are mounted on either server: the Open breaker must block the
    // effect BEFORE any Gmail network call is ever attempted.
    let state = test_state_with_gmail(gmail_connector(&token_server, &api_server));
    let (grant, token) = mint_grant_with_selection_token(
        &state,
        &["email.read_thread:selected_no_attachments"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );
    // Trip the gmail circuit breaker (default failure threshold is 3).
    for _ in 0..3 {
        state.connectors.record_connector_outcome("gmail", false);
    }

    let payload = json!({ "selection_token_id": token.id.to_string() });
    let result = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("email.read_thread:selected_no_attachments"),
        &crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID),
        Some(&payload),
        FailureSurface::Detached,
        None,
    )
    .await;

    assert!(
        matches!(result, Err(DispatchError::ConnectorUnavailable(_))),
        "an Open breaker must block the effect as a ConnectorUnavailable dispatch error, got {result:?}"
    );
    // The distinct connector_unavailable audit event is appended exactly once.
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("connector_unavailable")
            .unwrap(),
        1,
    );
    // The operational block is never double-batched as a normal dispatch
    // failure — ConnectorUnavailable is the distinct, already-audited outcome.
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("action.dispatch_failed")
            .unwrap(),
        0,
    );
    // The block is operational, not a policy denial: `gate()` Authorized the
    // action (action.gated carries an Allow), and the distinct
    // connector_unavailable event records the health block on top of it.
    let gated_allows = state
        .store
        .all_audit_event_jsons()
        .unwrap()
        .into_iter()
        .filter_map(|raw| serde_json::from_str::<Value>(&raw).ok())
        .any(|event| {
            event["kind"] == "action.gated"
                && event["action"] == "email.read_thread:selected_no_attachments"
                && event["decision"]["outcome"] == "allow"
        });
    assert!(
        gated_allows,
        "the breaker block must be post-gate (action.gated Allow), not a policy denial"
    );
    // R6: an Open gmail breaker must not block a *different* connector's
    // admission — telegram stays Closed and still acquires.
    assert_eq!(
        state.connectors.breaker_state("telegram"),
        Some(BreakerState::Closed)
    );
    assert!(state.connectors.acquire_connector("telegram").is_ok());
}

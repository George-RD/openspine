use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::briefcase::{
    Briefcase, CounterpartyRef, LearnedSource, RelationshipTier, TaskClass, TaskShape, TopUpRequest,
};
use openspine_schemas::digest::Digest;
use openspine_schemas::event::Lane;
use openspine_schemas::grant::{GrantLimits, GrantMode, TaskGrant};
use openspine_schemas::identity::{
    EntityType, Identifier, IdentifierKind, IdentifierVerificationMethod, Identity, Relationship,
    RelationshipKind,
};
use serde_json::{json, Value};
use sha2::Digest as _;
use ulid::Ulid;

use super::tests::start_server;
use crate::pipeline::AppState;
use crate::telegram::VerifiedOwnerContext;
use crate::test_support::fixtures::test_state;
const OWNER_CHAT_ID: i64 = 555;

fn mint_topup_grant(state: &AppState, allow_topup: bool) -> TaskGrant {
    let now = Timestamp::now();
    let mut grant = TaskGrant {
        id: Ulid::new(),
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user: state.owner_user_id.to_string(),
        purpose: "briefcase_topup_test".to_string(),
        issued_by: "kernel".to_string(),
        issued_at: now,
        expires_at: now + std::time::Duration::from_secs(120),
        event_id: Ulid::new(),
        route_id: "owner_telegram_main_assistant".to_string(),
        agent_id: "main_assistant_agent".to_string(),
        workflow_id: "owner_control_conversation".to_string(),
        capability_pack_id: "owner_control_basic_pack".to_string(),
        authority_sources: vec![],
        selection_tokens: vec![],
        allowed_actions: if allow_topup {
            vec![ActionId::new("briefcase.topup")]
        } else {
            vec![]
        },
        approval_required_actions: vec![],
        denied_actions: vec![],
        allowed_egress_classes: vec![],
        output_channels: vec![],
        limits: GrantLimits {
            max_model_calls: 8,
            max_artifacts: 20,
            max_runtime_seconds: 120,
        },
        task_token: Ulid::new().to_string(),
        root_grant_id: Ulid::nil(),
        parent_grant_id: None,
        mode: GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
    };
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    let pending_ref = state
        .artifacts
        .put(b"briefcase topup pending".as_slice())
        .unwrap();
    state
        .store
        .insert_task_grant(&grant, &pending_ref, OWNER_CHAT_ID)
        .unwrap();

    let briefcase = Briefcase {
        schema_version: 1,
        task_shape: TaskShape {
            route_id: "owner_telegram_main_assistant".to_string(),
            workflow_id: "owner_control_conversation".to_string(),
            counterparty: CounterpartyRef::Unresolved {
                channel: "email".to_string(),
                identifier: "thread-1".to_string(),
            },
        },
        source_snapshot_id: Digest::parse(format!("sha256:{}", "0".repeat(64))).unwrap(),
        depth: 1,
        tier: RelationshipTier::Stranger,
        class: TaskClass::Conversation,
        sections: vec![],
        top_up_log: vec![],
    };
    state.store.insert_briefcase(grant.id, &briefcase).unwrap();
    grant
}

async fn post_topup(
    addr: std::net::SocketAddr,
    token: &str,
    briefcase_id: Ulid,
    body: Value,
) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!(
            "http://{}/v1/briefcase/{}/topup",
            addr, briefcase_id
        ))
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn topup_granted_grant_mutates_briefcase_atomically() {
    let state = test_state();
    // Clone the store handle so we can observe post-request mutations even
    // though `start_server` consumes `state`.
    let store = state.store.clone();
    let first_source = LearnedSource {
        key: "a-packed".to_string(),
        kind: openspine_schemas::briefcase::SectionKind::Preference,
        payload: json!({"source": "packed"}),
        applicable_tiers: vec![],
        applicable_workflows: vec![],
    };
    let source = LearnedSource {
        key: "calendar".to_string(),
        kind: openspine_schemas::briefcase::SectionKind::Preference,
        payload: json!({"source": "omitted-at-pack"}),
        applicable_tiers: vec![],
        applicable_workflows: vec![],
    };
    let source_digest = openspine_schemas::digest::digest_of(&source.payload);
    store.insert_learned_source(&first_source).unwrap();
    store.insert_learned_source(&source).unwrap();
    let grant = mint_topup_grant(&state, true);
    let pool = crate::briefcase::SourcePool {
        learned: store.list_learned_sources().unwrap(),
    };
    let packed = crate::briefcase::pack_for_task(
        &grant,
        CounterpartyRef::Unresolved {
            channel: "email".to_string(),
            identifier: "thread-digest".to_string(),
        },
        json!({}),
        TaskClass::Conversation,
        &pool,
    )
    .unwrap();
    assert!(packed
        .sections
        .iter()
        .any(|section| section.key == "preference:a-packed"));
    assert!(!packed
        .sections
        .iter()
        .any(|section| section.key == "preference:calendar"));
    store
        .mutate_briefcase(grant.id, |briefcase| {
            *briefcase = packed;
            Ok::<(), crate::briefcase::BriefcaseKernelError>(())
        })
        .unwrap();
    let (addr, handle) = start_server(state).await;

    let body = json!({
        "request_id": Ulid::new().to_string(),
        "section_key": "calendar",
        "kind": "preference",
        "requested_depth": 2,
        "justification": "need more scheduling context"
    });
    let resp = post_topup(addr, &grant.task_token, grant.id, body).await;
    assert_eq!(resp.status(), 200);
    let decision: Value = resp.json().await.unwrap();
    assert_eq!(decision["outcome"]["outcome"], "allowed");
    assert_eq!(decision["source_digest"], source_digest.as_str());

    // The previously omitted learned source was authorized and appended.
    let loaded = store.find_briefcase(grant.id).unwrap().unwrap();
    assert_eq!(loaded.top_up_log.len(), 1);
    let calendar = loaded
        .sections
        .iter()
        .find(|section| section.key == "preference:calendar")
        .expect("rank-2 source should be appended");
    assert_eq!(calendar.payload, json!({"source": "omitted-at-pack"}));

    handle.abort();
}

#[tokio::test]
async fn topup_oversized_section_key_is_rejected_without_persistence() {
    let state = test_state();
    let store = state.store.clone();
    let grant = mint_topup_grant(&state, true);
    let (addr, handle) = start_server(state).await;
    let body = json!({
        "request_id": Ulid::new().to_string(),
        "section_key": "x".repeat(TopUpRequest::MAX_SECTION_KEY_BYTES + 1),
        "kind": "preference",
        "requested_depth": 1,
        "justification": "bounded key test"
    });
    let resp = post_topup(addr, &grant.task_token, grant.id, body).await;
    assert_eq!(resp.status(), 400);
    assert!(store
        .find_briefcase(grant.id)
        .unwrap()
        .unwrap()
        .top_up_log
        .is_empty());
    handle.abort();
}

#[tokio::test]
async fn topup_ungranted_grant_leaves_briefcase_unchanged() {
    let state = test_state();
    let store = state.store.clone();
    let grant = mint_topup_grant(&state, false);
    let (addr, handle) = start_server(state).await;

    let body = json!({
        "request_id": Ulid::new().to_string(),
        "section_key": "calendar",
        "kind": "preference",
        "requested_depth": 1,
        "justification": "need more scheduling context"
    });
    let resp = post_topup(addr, &grant.task_token, grant.id, body).await;
    assert_eq!(resp.status(), 200);
    let decision: Value = resp.json().await.unwrap();
    // Denied by the gate — the outcome must surface the gate decision.
    assert_eq!(decision["outcome"]["outcome"], "denied");

    // Critically, one denied decision is durably recorded for replay safety.
    let loaded = store.find_briefcase(grant.id).unwrap().unwrap();
    assert_eq!(loaded.top_up_log.len(), 1);
    handle.abort();
}

/// Bind an email address to a fresh identity with a relationship to the
/// owner, exactly as owner `/bind` would, so packing can resolve it.
fn bind_email_identity(state: &AppState, email: &str, relationship: RelationshipKind) -> Ulid {
    let owner = state.store.owner_principal().unwrap().unwrap();
    let identity_id = Ulid::new();
    let mut hasher = sha2::Sha256::new();
    hasher.update(email.as_bytes());
    let hash = openspine_schemas::digest::digest_from_hash(hasher.finalize().into());
    let identity = Identity {
        id: identity_id,
        display_name: email.to_string(),
        entity_type: EntityType::Person,
        identifiers: vec![Identifier {
            kind: IdentifierKind::Email,
            value_hash: hash,
            verified: true,
            verification_method: IdentifierVerificationMethod::UserConfirmed,
        }],
        relationships: vec![Relationship {
            kind: relationship,
            target_id: owner.identity_id,
            confidence: 1.0,
            notes_ref: None,
        }],
        schema_version: 1,
    };
    state
        .store
        .owner_assert_identity_binding(owner.id, &VerifiedOwnerContext::test_new(), &identity)
        .unwrap();
    identity_id
}

/// A minimal grant sufficient for `pack_for_pipeline` (which only needs the
/// structural fields for its semantic projection).
fn minimal_grant() -> TaskGrant {
    serde_json::from_value(serde_json::json!({
        "id": Ulid::new().to_string(),
        "schema_version": 1,
        "lifecycle_state": "active",
        "user": "owner",
        "purpose": "test",
        "issued_by": "kernel",
        "issued_at": "2026-01-01T00:00:00Z",
        "expires_at": "2030-01-01T00:00:00Z",
        "event_id": Ulid::new().to_string(),
        "route_id": "route",
        "agent_id": "agent",
        "workflow_id": "workflow",
        "capability_pack_id": "pack",
        "authority_sources": [],
        "selection_tokens": [],
        "allowed_actions": [],
        "approval_required_actions": [],
        "denied_actions": [],
        "allowed_egress_classes": [],
        "output_channels": [],
        "limits": {"max_model_calls": 1, "max_artifacts": 1, "max_runtime_seconds": 1},
        "task_token": "secret",
        "root_grant_id": Ulid::new().to_string(),
        "parent_grant_id": null,
        "mode": "live",
        "chain": [],
        "caveat_mac": ""
    }))
    .unwrap()
}

#[tokio::test]
async fn email_counterparty_resolves_to_bound_identity_when_address_is_bound() {
    let state = test_state();
    let identity_id = bind_email_identity(&state, "alice@example.com", RelationshipKind::Client);
    let grant = minimal_grant();
    let briefcase = crate::briefcase::pack_for_pipeline(
        &state,
        Some("thread-1"),
        Lane::ExternalCommunication,
        &grant,
        Some("alice@example.com"),
    )
    .await
    .unwrap();
    match briefcase.task_shape.counterparty {
        CounterpartyRef::Bound {
            identity_id: id,
            relationship,
        } => {
            assert_eq!(id, identity_id);
            assert_eq!(relationship, RelationshipKind::Client);
        }
        other => panic!("expected Bound counterparty, got {other:?}"),
    }
}

#[tokio::test]
async fn email_counterparty_stays_unresolved_when_address_is_unbound() {
    let state = test_state();
    let grant = minimal_grant();
    let briefcase = crate::briefcase::pack_for_pipeline(
        &state,
        Some("thread-1"),
        Lane::ExternalCommunication,
        &grant,
        Some("bob@example.com"),
    )
    .await
    .unwrap();
    match briefcase.task_shape.counterparty {
        CounterpartyRef::Unresolved {
            channel,
            identifier,
        } => {
            assert_eq!(channel, "email");
            assert_eq!(
                identifier,
                "email:5ff860bf1190596c7188ab851db691f0f3169c453936e9e1eba2f9a47f7a0018"
            );
        }
        other => panic!("expected Unresolved counterparty, got {other:?}"),
    }
}

#[tokio::test]
async fn topup_replayed_denied_request_records_once() {
    let state = test_state();
    let store = state.store.clone();
    let grant = mint_topup_grant(&state, false);
    let (addr, handle) = start_server(state).await;

    let request_id = Ulid::new().to_string();
    let body = json!({
        "request_id": request_id,
        "section_key": "calendar",
        "kind": "preference",
        "requested_depth": 1,
        "justification": "need more scheduling context"
    });
    // First (denied) request is recorded durably.
    let first = post_topup(addr, &grant.task_token, grant.id, body.clone()).await;
    assert_eq!(first.status(), 200);
    // Replaying the same request id MUST be rejected and must NOT append a
    // second decision (spec: "Replayed top-up" → no second decision).
    let second = post_topup(addr, &grant.task_token, grant.id, body.clone()).await;
    assert!(
        !second.status().is_success(),
        "replayed denied request must be rejected"
    );

    let loaded = store.find_briefcase(grant.id).unwrap().unwrap();
    assert_eq!(loaded.top_up_log.len(), 1);
    handle.abort();
}

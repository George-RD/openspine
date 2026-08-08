//! Scheduled-miner grant resolution cases, split from
//! `artifact_propose_miner_tests.rs` for the 500-line gate.

use super::*;
use crate::reflection_miner_runtime::{
    find_active_grant_by_route, reflection_miner_tick, REFLECTION_SCHEDULED_MINER_ROUTE,
    REFLECTION_SCHEDULED_SUBMITTER_ROUTE,
};

use jiff::Timestamp;
use openspine_schemas::action::{ActionId, ActionImplementationId, GateDecision};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::audit::AuditEvent;
use openspine_schemas::briefcase::CounterpartyRef;
use openspine_schemas::digest::{canonical_json, digest_from_hash, Digest};
use openspine_schemas::event::{AccountRole, TargetRef, TargetRefKind};
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::identity::RelationshipKind;
use openspine_schemas::resolved_context::{ResolvedActionContext, ResolvedActionContextInput};
use openspine_schemas::reviewed_scope::ReviewedActionScope;
use openspine_schemas::standing_rule::ReviewedScopeBinding;
use rusqlite::params;
use serde_json::json;
use sha2::{Digest as ShaDigest, Sha256};
use std::collections::BTreeMap;
use ulid::Ulid;

#[tokio::test]
async fn scheduled_reflection_miner_tick_mines_repeated_approval() {
    use openspine_schemas::action::{ActionId, ActionImplementationId, GateDecision};
    use openspine_schemas::artifact::ArtifactRef;
    use openspine_schemas::briefcase::CounterpartyRef;
    use openspine_schemas::digest::Digest;
    use openspine_schemas::event::{AccountRole, TargetRef, TargetRefKind};
    use openspine_schemas::identity::RelationshipKind;
    use openspine_schemas::resolved_context::{ResolvedActionContext, ResolvedActionContextInput};
    use std::collections::BTreeMap;
    use ulid::Ulid;
    let server = MockServer::start().await;
    let token = "driver-test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{token}/SendMessage")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": OWNER_CHAT_ID, "type": "private"},
                "text": "sent",
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let connector =
        TelegramConnector::with_api_url(token.to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);

    // First tick composes the scheduled grants from active artifacts; nothing
    // is learnable from an empty audit ledger.
    let dispatched_first = reflection_miner_tick(&state).await.unwrap();
    assert_eq!(dispatched_first, 0);

    let miner_grant = find_active_grant_by_route(&state, REFLECTION_SCHEDULED_MINER_ROUTE)
        .unwrap()
        .expect("miner grant must have been composed on first tick")
        .0;
    let submitter_grant = find_active_grant_by_route(&state, REFLECTION_SCHEDULED_SUBMITTER_ROUTE)
        .unwrap()
        .expect("submitter grant must have been composed on first tick")
        .0;
    assert_eq!(miner_grant.agent_id, "reflection_miner_agent");
    assert_eq!(miner_grant.workflow_id, "reflection_miner_scheduled");
    assert_eq!(miner_grant.capability_pack_id, "reflection_miner_pack");
    assert!(miner_grant.output_channels.is_empty());
    assert!(!miner_grant.authority_sources.is_empty());
    assert_eq!(submitter_grant.agent_id, "reflection_submitter_agent");
    assert_eq!(
        submitter_grant.workflow_id,
        "reflection_submitter_scheduled"
    );
    assert_eq!(
        submitter_grant.capability_pack_id,
        "reflection_submitter_pack"
    );
    let owner_grant = handle_owner_update(&state, &owner_update("capture owner history"))
        .await
        .unwrap()
        .expect("owner update must compose an owner-control grant");
    seed_owner_history(&state, &owner_grant);

    // Seed two kernel-verifiable email.create_draft approvals. Each row
    // carries the resolved-context evidence payload that the scheduled miner
    // can trust; the payload digest varies to prove it is not the request key.
    let target_ref = ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
        schema_version: 1,
    };
    let payload_ref = ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
        schema_version: 1,
    };
    let action = ActionId::new("email.create_draft");
    let decision = GateDecision::Allow;
    let counterparty_scope_id = Ulid::from(11_u128);
    let context = ResolvedActionContext::try_new(
        &state.action_catalog,
        &action,
        &ActionImplementationId::new("gmail.draft.v1"),
        ResolvedActionContextInput {
            connector_instance_id: "gmail-primary".into(),
            account_role: Some(AccountRole::OwnerMailbox),
            account_identity_digest: Some(
                Digest::parse(format!("sha256:{}", "1".repeat(64))).unwrap(),
            ),
            target_refs: vec![TargetRef {
                kind: TargetRefKind::EmailThread,
                id: Some("thread-1".into()),
            }],
            counterparty: Some(CounterpartyRef::Bound {
                identity_id: counterparty_scope_id,
                relationship: RelationshipKind::Client,
            }),
            bound_parameters: BTreeMap::from([(
                "thread_participants".into(),
                "alice@example.com".into(),
            )]),
            target_digest: Some(target_ref.digest.clone()),
            payload_digest: Some(payload_ref.digest.clone()),
            workflow_id: Some("draft_reply_workflow".into()),
            task_shape_digest: Some(Digest::parse(format!("sha256:{}", "e".repeat(64))).unwrap()),
        },
    )
    .unwrap();
    let reviewed_scope =
        openspine_schemas::reviewed_scope::ReviewedActionScope::derive(&context).unwrap();
    let binding = openspine_schemas::standing_rule::ReviewedScopeBinding::derive_from(
        reviewed_scope.clone(),
        context.compatibility_digest().clone(),
    );
    let scope_artifact = crate::store::OwnerApprovalScopeArtifact {
        scope: reviewed_scope,
        compatibility_digest: context.compatibility_digest().clone(),
    };
    let scope_ref = state
        .artifacts
        .put_scoped(
            counterparty_scope_id,
            &serde_json::to_vec(&scope_artifact).unwrap(),
        )
        .unwrap();
    let context_class_digest = binding.scope.context_class_digest().clone();
    let reviewed_scope_digest = binding.reviewed_scope_digest.clone();
    let request_digest = context.task_shape_digest().unwrap().clone();
    for payload_byte in ['f', '0'] {
        let metadata = crate::store::OwnerApprovalAuditMetadata {
            schema_version: 1,
            context_class_digest: context_class_digest.clone(),
            reviewed_scope_digest: reviewed_scope_digest.clone(),
            request_digest: request_digest.clone(),
            target_digest: target_ref.digest.clone(),
            payload_digest: Digest::parse(format!(
                "sha256:{}",
                payload_byte.to_string().repeat(64)
            ))
            .unwrap(),
            compatibility_digest: context.compatibility_digest().clone(),
            counterparty_scope_id,
            reviewed_scope_ref: scope_ref.clone(),
        };
        let metadata_json = serde_json::to_string(&metadata).unwrap();
        state
            .store
            .append_audit_with_payload_json(
                "action.gated",
                Some(&action),
                Some(&decision),
                Some(crate::store::OWNER_APPROVAL_GATE_REASON),
                Some(owner_grant.id),
                std::slice::from_ref(&target_ref),
                std::slice::from_ref(&payload_ref),
                Some(&metadata_json),
            )
            .unwrap();
    }

    // Second tick derives one typed repeated-approval observation and
    // dispatches one standing_rule proposal through the normal lifecycle.
    let dispatched = reflection_miner_tick(&state).await.unwrap();
    assert_eq!(dispatched, 1);
    assert!(
        state
            .store
            .proposed_artifact_exists("standing_rule", context_class_digest.as_str(), 1)
            .unwrap(),
        "a standing_rule proposal must be persisted through the lifecycle"
    );
    assert!(
        state
            .store
            .count_audit_events_of_kind("reflection.miner.provenance")
            .unwrap()
            >= 1
    );
}

struct MinerTickHarness {
    state: crate::pipeline::AppState,
    owner_grant: TaskGrant,
    _server: MockServer,
}

async fn miner_tick_harness() -> MinerTickHarness {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": OWNER_CHAT_ID, "type": "private"},
                "text": "sent",
            }
        })))
        .mount(&server)
        .await;
    let connector = TelegramConnector::with_api_url(
        "miner-test-token".to_string(),
        server.uri().parse().unwrap(),
    );
    let state = test_state_with_telegram(connector);
    assert_eq!(reflection_miner_tick(&state).await.unwrap(), 0);
    let owner_grant = handle_owner_update(&state, &owner_update("capture owner history"))
        .await
        .unwrap()
        .expect("owner update must compose an owner-control grant");
    seed_owner_history(&state, &owner_grant);
    MinerTickHarness {
        state,
        owner_grant,
        _server: server,
    }
}

fn resolved_email_context(
    state: &crate::pipeline::AppState,
    target_id: &str,
    target_byte: char,
    account_byte: char,
    counterparty_scope_id: Ulid,
) -> (
    ResolvedActionContext,
    ArtifactRef,
    ArtifactRef,
    ReviewedScopeBinding,
) {
    resolved_email_context_with_task_shape(
        state,
        target_id,
        target_byte,
        account_byte,
        counterparty_scope_id,
        'e',
    )
}

fn resolved_email_context_with_task_shape(
    state: &crate::pipeline::AppState,
    target_id: &str,
    target_byte: char,
    account_byte: char,
    counterparty_scope_id: Ulid,
    task_shape_byte: char,
) -> (
    ResolvedActionContext,
    ArtifactRef,
    ArtifactRef,
    ReviewedScopeBinding,
) {
    let target_ref = ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", target_byte.to_string().repeat(64))).unwrap(),
        schema_version: 1,
    };
    let payload_ref = ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
        schema_version: 1,
    };
    let action = ActionId::new("email.create_draft");
    let context = ResolvedActionContext::try_new(
        &state.action_catalog,
        &action,
        &ActionImplementationId::new("gmail.draft.v1"),
        ResolvedActionContextInput {
            connector_instance_id: "gmail-primary".into(),
            account_role: Some(AccountRole::OwnerMailbox),
            account_identity_digest: Some(
                Digest::parse(format!("sha256:{}", account_byte.to_string().repeat(64))).unwrap(),
            ),
            target_refs: vec![TargetRef {
                kind: TargetRefKind::EmailThread,
                id: Some(target_id.into()),
            }],
            counterparty: Some(CounterpartyRef::Bound {
                identity_id: counterparty_scope_id,
                relationship: RelationshipKind::Client,
            }),
            bound_parameters: BTreeMap::from([(
                "thread_participants".into(),
                "alice@example.com".into(),
            )]),
            target_digest: Some(target_ref.digest.clone()),
            payload_digest: Some(payload_ref.digest.clone()),
            workflow_id: Some("draft_reply_workflow".into()),
            task_shape_digest: Some(
                Digest::parse(format!("sha256:{}", task_shape_byte.to_string().repeat(64)))
                    .unwrap(),
            ),
        },
    )
    .unwrap();
    let scope = ReviewedActionScope::derive(&context).unwrap();
    let binding =
        ReviewedScopeBinding::derive_from(scope.clone(), context.compatibility_digest().clone());
    let scope_artifact = crate::store::OwnerApprovalScopeArtifact {
        scope,
        compatibility_digest: context.compatibility_digest().clone(),
    };
    let reviewed_scope_ref = state
        .artifacts
        .put_scoped(
            counterparty_scope_id,
            &serde_json::to_vec(&scope_artifact).unwrap(),
        )
        .unwrap();
    (context, target_ref, reviewed_scope_ref, binding)
}

fn append_approval(
    harness: &MinerTickHarness,
    context: &ResolvedActionContext,
    target_ref: &ArtifactRef,
    reviewed_scope_ref: &ArtifactRef,
    payload_byte: char,
) -> AuditEvent {
    let action = ActionId::new("email.create_draft");
    let mut metadata =
        crate::store::OwnerApprovalAuditMetadata::from_context(context, reviewed_scope_ref.clone())
            .expect("resolved context must carry all approval evidence");
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
async fn scheduled_reflection_miner_two_context_classes_do_not_form_one_pattern() {
    let harness = miner_tick_harness().await;
    let (first, first_target, first_scope, _) =
        resolved_email_context(&harness.state, "thread-1", 'a', '1', Ulid::from(11_u128));
    let (second, second_target, second_scope, _) =
        resolved_email_context(&harness.state, "thread-2", 'c', '2', Ulid::from(12_u128));
    append_approval(&harness, &first, &first_target, &first_scope, 'f');
    append_approval(&harness, &second, &second_target, &second_scope, '0');
    assert_eq!(reflection_miner_tick(&harness.state).await.unwrap(), 0);
}

#[path = "artifact_propose_miner_binding_tests.rs"]
mod binding_tests;
#[path = "artifact_propose_miner_grouping_tests.rs"]
mod grouping_tests;

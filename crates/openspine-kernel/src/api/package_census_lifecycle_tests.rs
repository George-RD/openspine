use std::time::Duration;

use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::{ProposalApprovalEvidence, ProposalApprovalPath};
use openspine_schemas::audit::{AuditEvent, AuditKind};
use openspine_schemas::digest::Digest;
use openspine_schemas::event_bus::EventSubscriptionFilter;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::owner_surface::OwnerSurfaceRef;
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::pipeline::{handle_owner_update, AppState};
use crate::store::package_install::outstanding_work::{
    OutstandingWorkCounts, OutstandingWorkSource,
};
use crate::store::Store;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::{owner_update, seed_owner_history, test_state_with_telegram};

const OWNER_CHAT_ID: i64 = 555;
const TOKEN: &str = "test-token";

fn ordinary_payload(id: &str) -> serde_json::Value {
    serde_json::json!({
        "kind": "route",
        "yaml": format!(
            "id: {id}\nschema_version: 1\nlifecycle_state: proposed\npriority: 100\nagent: main_assistant_agent\nworkflow: owner_control_conversation\ncapability_pack: owner_control_basic_pack\n"
        ),
    })
}

async fn dispatch_proposal(
    state: &AppState,
    grant: &TaskGrant,
    surface: &OwnerSurfaceRef,
    payload: &Value,
) -> Result<Value, super::actions::DispatchError> {
    let action = ActionId::new("artifact.propose");
    let handler = state
        .action_handlers
        .lookup(action.as_str())
        .expect("artifact.propose production handler must be registered");
    handler(state, grant, &action, surface, Some(payload)).await
}

async fn ordinary_proposal() -> (AppState, TaskGrant, ulid::Ulid, MockServer) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/bot{TOKEN}/SendMessage")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": OWNER_CHAT_ID, "type": "private"},
                "text": "sent"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        TOKEN.into(),
        server.uri().parse().unwrap(),
    ));
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .unwrap();
    seed_owner_history(&state, &grant);
    let (_, _, surface) = state
        .store
        .find_task_grant_by_id(grant.id)
        .unwrap()
        .unwrap();
    let result = dispatch_proposal(
        &state,
        &grant,
        &surface,
        &ordinary_payload("census-ordinary-route"),
    )
    .await
    .unwrap();
    let request_id = result["action_request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    state.store.with_conn_for_test(|conn| {
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM owner_reviews", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            count, 0,
            "ordinary proposal uses only its digest-bound callback"
        );
    });
    (state, grant, request_id, server)
}

fn recorded_proof(store: &Store, kind: &'static str) -> (AuditEvent, ProposalApprovalEvidence) {
    let filter = EventSubscriptionFilter::kinds([AuditKind::from_static(kind)]);
    let entries = store.replay_audit(&filter, 0).unwrap();
    assert_eq!(entries.len(), 1, "expected one {kind} event");
    let event = entries.into_iter().next().unwrap().event;
    let proof = serde_json::from_str(event.payload_json.as_deref().unwrap()).unwrap();
    (event, proof)
}

#[tokio::test]
async fn ordinary_proposal_becomes_terminal_when_its_delivered_callback_grant_expires() {
    let (state, grant, request, _server) = ordinary_proposal().await;
    let before_expiry = state
        .store
        .package_outstanding_work_with_reviews(
            grant.expires_at - Duration::from_nanos(1),
            &state.artifacts,
        )
        .unwrap();
    assert_eq!(
        before_expiry.source(OutstandingWorkSource::ProposedArtifacts),
        OutstandingWorkCounts {
            outstanding: 1,
            terminal: 0,
            unknown: 0,
        }
    );
    let audit = state.store.all_audit_event_jsons().unwrap();
    let request_before = state.store.find_action_request(request).unwrap();
    let after = state
        .store
        .package_outstanding_work_with_reviews(grant.expires_at, &state.artifacts)
        .unwrap();
    assert_eq!(
        after.source(OutstandingWorkSource::ActionRequests),
        OutstandingWorkCounts {
            outstanding: 0,
            terminal: 1,
            unknown: 0,
        }
    );
    assert_eq!(
        after.source(OutstandingWorkSource::ProposedArtifacts),
        OutstandingWorkCounts {
            outstanding: 0,
            terminal: 1,
            unknown: 0,
        }
    );
    assert_eq!(
        state.store.find_action_request(request).unwrap(),
        request_before
    );
    assert_eq!(state.store.all_audit_event_jsons().unwrap(), audit);
    assert_eq!(
        state
            .store
            .find_proposed_artifact_by_action_request(request)
            .unwrap()
            .unwrap()
            .state,
        openspine_schemas::artifact::Lifecycle::ReviewRequired
    );
}

#[tokio::test]
async fn ordinary_proposal_approval_path_survives_the_grant_sweep() {
    let (state, grant, request, _server) = ordinary_proposal().await;
    let as_of = grant.expires_at + Duration::from_secs(172800);
    state.store.sweep_expired_grants(as_of).unwrap();
    assert!(state
        .store
        .find_task_grant_by_id(grant.id)
        .unwrap()
        .is_none());
    let audit = state.store.all_audit_event_jsons().unwrap();
    let request_before = state.store.find_action_request(request).unwrap();
    let snapshot = state
        .store
        .package_outstanding_work_with_reviews(as_of, &state.artifacts)
        .unwrap();
    assert_eq!(
        snapshot.source(OutstandingWorkSource::ProposedArtifacts),
        OutstandingWorkCounts {
            outstanding: 0,
            terminal: 1,
            unknown: 0,
        }
    );
    assert_eq!(
        state.store.find_action_request(request).unwrap(),
        request_before
    );
    assert_eq!(state.store.all_audit_event_jsons().unwrap(), audit);
}

#[tokio::test]
async fn ordinary_proposal_evidence_binds_proposal_and_successful_callback_delivery() {
    let (state, grant, request_id, _server) = ordinary_proposal().await;
    let (proposed_event, proposed_proof) = recorded_proof(&state.store, "artifact.proposed");
    let (delivery_event, delivery_proof) =
        recorded_proof(&state.store, "artifact.proposal_callback_delivered");
    let proposal = state
        .store
        .find_proposed_artifact_by_action_request(request_id)
        .unwrap()
        .unwrap();
    let expected = ProposalApprovalEvidence {
        schema_version: 1,
        proposal_id: proposal.id,
        artifact_kind: proposal.kind,
        artifact_id: proposal.artifact_id,
        artifact_version: proposal.version,
        task_grant_id: grant.id,
        action_request_id: request_id,
        proposal_digest: Digest::parse(proposal.yaml_digest).unwrap(),
        approval_path: ProposalApprovalPath::GrantBoundCallback,
    };
    assert_eq!(proposed_proof, expected);
    assert_eq!(delivery_proof, expected);
    for event in [proposed_event, delivery_event] {
        assert_eq!(event.task_grant_id, Some(grant.id));
        assert_eq!(event.payload_refs.len(), 1);
        assert_eq!(event.payload_refs[0].digest, expected.proposal_digest);
    }
    assert!(state.store.verify_audit_chain().unwrap());
}

#[tokio::test]
async fn failed_owner_notification_cannot_be_cleared_by_callback_grant_expiry() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/bot{TOKEN}/SendMessage")))
        .respond_with(ResponseTemplate::new(500).set_body_string("failed"))
        .expect(1)
        .mount(&server)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        TOKEN.into(),
        server.uri().parse().unwrap(),
    ));
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .unwrap();
    seed_owner_history(&state, &grant);
    let (_, _, surface) = state
        .store
        .find_task_grant_by_id(grant.id)
        .unwrap()
        .unwrap();
    let result = dispatch_proposal(
        &state,
        &grant,
        &surface,
        &ordinary_payload("census-failed-notify-route"),
    )
    .await;
    assert!(result.is_err(), "failed owner notification must surface");
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("artifact.proposal_callback_delivered")
            .unwrap(),
        0
    );
    let snapshot = state
        .store
        .package_outstanding_work_with_reviews(grant.expires_at, &state.artifacts)
        .unwrap();
    assert_eq!(
        snapshot.source(OutstandingWorkSource::ActionRequests),
        OutstandingWorkCounts {
            outstanding: 0,
            terminal: 1,
            unknown: 0,
        }
    );
    assert_eq!(
        snapshot.source(OutstandingWorkSource::ProposedArtifacts),
        OutstandingWorkCounts {
            outstanding: 0,
            terminal: 0,
            unknown: 1,
        }
    );
    assert!(state.store.verify_audit_chain().unwrap());
}

#[tokio::test]
async fn malformed_or_conflicting_approval_evidence_cannot_clear_a_proposal() {
    for axis in [
        "duplicate",
        "schema",
        "proposal",
        "request",
        "grant",
        "kind",
        "artifact",
        "version",
        "digest",
        "path",
        "malformed",
    ] {
        let (state, grant, _, _server) = ordinary_proposal().await;
        let (event, mut proof) = recorded_proof(&state.store, "artifact.proposed");
        match axis {
            "schema" => proof.schema_version = 2,
            "proposal" => proof.proposal_id = ulid::Ulid::new(),
            "request" => proof.action_request_id = ulid::Ulid::new(),
            "grant" => proof.task_grant_id = ulid::Ulid::new(),
            "kind" => proof.artifact_kind = "workflow".into(),
            "artifact" => proof.artifact_id = "another-artifact".into(),
            "version" => proof.artifact_version = 2,
            "digest" => {
                proof.proposal_digest = openspine_schemas::digest::digest_of_bytes(b"different")
            }
            // This appends a second, conflicting path receipt to the original
            // callback receipt; it is intentionally not a standalone evaluated
            // proposal and must therefore remain unknown.
            "path" => proof.approval_path = ProposalApprovalPath::EvaluatedOwnerReview,
            _ => {}
        }
        let json = if axis == "malformed" {
            "{}".into()
        } else {
            serde_json::to_string(&proof).unwrap()
        };
        state
            .store
            .append_audit_with_payload_json(
                "artifact.proposed",
                event.action.as_ref(),
                None,
                None,
                event.task_grant_id,
                &[],
                &event.payload_refs,
                Some(&json),
            )
            .unwrap();
        assert!(
            state.store.verify_audit_chain().unwrap(),
            "fixture retains a valid chain"
        );
        let before = state.store.all_audit_event_jsons().unwrap();
        let snapshot = state
            .store
            .package_outstanding_work_with_reviews(grant.expires_at, &state.artifacts)
            .unwrap();
        assert_eq!(
            snapshot.source(OutstandingWorkSource::ProposedArtifacts),
            OutstandingWorkCounts {
                outstanding: 0,
                terminal: 0,
                unknown: 1,
            },
            "{axis}"
        );
        assert!(!snapshot.is_quiescent());
        assert_eq!(state.store.all_audit_event_jsons().unwrap(), before);
    }
}

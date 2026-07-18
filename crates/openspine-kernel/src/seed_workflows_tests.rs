//! Acceptance tests for the AD-153 minimal seed workflow set: every seed must
//! parse/validate as a state machine, render Mermaid, load as an overlay
//! namespace artifact on a fresh install, and the email-draft seed's
//! approval-required state must resolve through the digest-bound approval
//! contract (D-087/D-088).

use super::*;

use crate::artifact_store::ArtifactStore;
use crate::store::Store;
use crate::workflow_state_machine::{WorkflowStateMachine, WorkflowStateMachineError};

use openspine_schemas::action::{ActionId, ActionRequest};
use openspine_schemas::approval::{ApprovalDecision, ApprovalRecord, TimeoutBehavior};
use openspine_schemas::artifact::{ArtifactNamespace, ArtifactRef};
use openspine_schemas::digest::Digest;
use openspine_schemas::workflow::{ApprovalSemantics, WorkflowManifest};

use tempfile::tempdir;
use ulid::Ulid;

fn named(id: &str) -> WorkflowManifest {
    parsed()
        .unwrap_or_else(|err| panic!("seed {id} must parse and validate: {err}"))
        .into_iter()
        .find(|manifest| manifest.id == id)
        .unwrap_or_else(|| panic!("seed {id} present"))
}

#[test]
fn all_seeds_parse_and_validate_as_state_machines() {
    let manifests = parsed().expect("all seed manifests parse and validate");
    assert_eq!(manifests.len(), 4, "exactly four seed workflows ship");

    let mut ids: Vec<&str> = manifests.iter().map(|m| m.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 4, "seed ids must be unique");

    for manifest in &manifests {
        assert!(
            manifest.initial_state.is_some(),
            "seed {} must declare an initial state",
            manifest.id
        );
        assert!(
            !manifest.states.is_empty(),
            "seed {} must declare states",
            manifest.id
        );
        assert!(
            !manifest.transitions.is_empty(),
            "seed {} must declare transitions",
            manifest.id
        );
        let state_ids: Vec<&str> = manifest.states.iter().map(|s| s.id.as_str()).collect();
        for transition in &manifest.transitions {
            assert!(
                state_ids.iter().any(|s| *s == transition.from),
                "seed {} transition from undeclared state {}",
                manifest.id,
                transition.from
            );
            assert!(
                state_ids.iter().any(|s| *s == transition.to),
                "seed {} transition to undeclared state {}",
                manifest.id,
                transition.to
            );
        }
    }
}

#[test]
fn seeds_render_mermaid_flowcharts() {
    for manifest in parsed().unwrap() {
        let mermaid = manifest.to_mermaid();
        assert!(
            mermaid.starts_with("flowchart TD\n"),
            "seed {} must render a Mermaid flowchart",
            manifest.id
        );
        let edge_lines = mermaid.lines().filter(|line| line.contains("-->|")).count();
        assert_eq!(
            edge_lines,
            manifest.transitions.len(),
            "seed {} must render one edge per transition",
            manifest.id
        );
    }
}

#[test]
fn email_draft_seed_declares_digest_bound_approval_state() {
    let manifest = named("email_draft_with_approval_seed");
    let approval_state = manifest
        .states
        .iter()
        .find(|state| state.approval == ApprovalSemantics::Required)
        .expect("email seed must declare an approval-required state");
    assert_eq!(approval_state.id, "awaiting_approval");
    assert_eq!(
        approval_state.approval_action,
        Some(ActionId::new("email.create_draft")),
        "approval-required state must bind the digest-bound create_draft action"
    );
}

#[test]
fn materialize_is_idempotent_and_preserves_edits() {
    let store = Store::open_in_memory().unwrap();
    let dir = tempdir().unwrap().keep();
    let overlay = dir.join("artifacts.d");

    let first = materialize_missing(&store, &overlay).unwrap();
    assert_eq!(first, 4, "first boot materializes all four seeds");

    let second = materialize_missing(&store, &overlay).unwrap();
    assert_eq!(second, 0, "second boot writes nothing");

    // An owner edit to an existing seed file must survive a third boot.
    let target = overlay
        .join("workflows")
        .join(crate::artifact_loader::overlay_filename(
            "owner_control_conversation_seed",
            1,
        ));
    std::fs::write(&target, "owner-edited-marker").unwrap();
    let third = materialize_missing(&store, &overlay).unwrap();
    assert_eq!(third, 0, "seed must not overwrite an edited existing file");
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "owner-edited-marker",
        "owner edit preserved"
    );
}

#[test]
fn seeds_load_as_overlay_namespace_artifacts_on_fresh_install() {
    let overlay_dir = tempdir().unwrap().keep().join("artifacts.d");

    // First boot materializes the seed files into the overlay workflows dir.
    let written = write_seed_files(&overlay_dir).unwrap();
    assert_eq!(written, 4, "first boot writes all four seed files");

    let workflow_files = std::fs::read_dir(overlay_dir.join("workflows"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .count();
    assert_eq!(
        workflow_files, 4,
        "exactly four seed workflow files materialized"
    );

    // The overlay loader parses them as workflow artifacts (the same loader the
    // kernel uses to register overlay-namespace artifacts at startup).
    let registry = crate::artifact_loader::load_registry(&overlay_dir).unwrap();
    let seed_ids: Vec<&str> = registry.workflows.keys().map(|id| id.as_str()).collect();
    assert_eq!(
        seed_ids.len(),
        4,
        "four workflow seeds loaded by the overlay loader"
    );
    for want in [
        "owner_control_conversation_seed",
        "email_draft_with_approval_seed",
        "research_and_brief_seed",
        "customer_service_intake_seed",
    ] {
        assert!(
            seed_ids.contains(&want),
            "missing overlay seed artifact {want}"
        );
    }
}

#[test]
fn email_seed_approval_state_requires_digest_bound_approval() {
    let store = Store::open_in_memory().unwrap();
    let manifest = named("email_draft_with_approval_seed");
    let mut machine = WorkflowStateMachine::new(&store, "seed-email-run", manifest).unwrap();
    assert_eq!(machine.current_state(), Some("selected"));

    // Advance to the approval-required state. Entering binds the action request
    // id that the digest-bound approval is later checked against (the kernel
    // presents the draft, the owner approves, then the run advances into the
    // approval-required state carrying that request id).
    machine.transition_to("drafted", None).unwrap();
    assert_eq!(machine.current_state(), Some("drafted"));

    let request_id = insert_request_and_approval(&store, "email.create_draft");
    machine
        .transition_to("awaiting_approval", Some(request_id))
        .unwrap();
    assert_eq!(machine.current_state(), Some("awaiting_approval"));

    // Leaving the approval-required state without the matching bound request is
    // rejected.
    assert!(matches!(
        machine.transition_to("approved", None),
        Err(WorkflowStateMachineError::ApprovalRequired(_))
    ));

    // With the bound request (whose approval is digest-matched), departure
    // succeeds.
    machine.transition_to("approved", Some(request_id)).unwrap();
    assert_eq!(machine.current_state(), Some("approved"));
}

fn digest(ch: char) -> Digest {
    Digest::parse(format!("sha256:{}", ch.to_string().repeat(64))).unwrap()
}

fn insert_request_and_approval(store: &Store, action: &str) -> Ulid {
    let request_id = Ulid::new();
    let payload_digest = digest('a');
    let target_digest = digest('b');
    let request = ActionRequest {
        id: request_id,
        task_grant_id: Ulid::new(),
        action: ActionId::new(action),
        target_ref: None,
        payload_ref: Some(ArtifactRef {
            digest: payload_digest.clone(),
            schema_version: 1,
        }),
        target_digest: Some(target_digest.clone()),
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        requested_at: jiff::Timestamp::now(),
        schema_version: 1,
    };
    store.insert_action_request(&request).unwrap();
    store
        .insert_approval(&ApprovalRecord {
            id: Ulid::new(),
            schema_version: 1,
            action_request_id: request_id,
            approved_by: "owner".to_string(),
            approved_at: jiff::Timestamp::now(),
            approved_payload_digest: payload_digest,
            approved_target_digest: target_digest,
            expires_at: jiff::Timestamp::now() + std::time::Duration::from_secs(900),
            decision: ApprovalDecision::Approved,
            timeout_behavior: TimeoutBehavior::DoNothing,
            approval_channel: "test".to_string(),
        })
        .unwrap();
    request_id
}

#[test]
fn seeds_register_as_overlay_namespace_learned_artifacts() {
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(tempdir().unwrap().keep(), [3u8; 32]).unwrap();
    let data_dir = tempdir().unwrap().keep();
    let overlay_dir = data_dir.join("artifacts.d");
    let lyra_dir = tempdir().unwrap().keep();

    // Drive the real startup path: materialize the seeds, then run the overlay
    // startup loader, which discovers the orphaned overlay files and records
    // each as a learned artifact.
    let written = crate::seed_workflows::materialize_missing(&store, &overlay_dir).unwrap();
    assert_eq!(
        written, 4,
        "materialize writes all four seed files on first boot"
    );
    let _startup = crate::overlay_startup::load(&lyra_dir, &data_dir, &store, &artifacts)
        .expect("overlay startup must register the seed artifacts");

    // Each seed is recorded as a workflow in the Overlay namespace.
    let learned = store.list_learned_artifacts().unwrap();
    let seed_kinds: Vec<&str> = learned
        .iter()
        .filter(|artifact| {
            artifact.kind == "workflow" && artifact.namespace == ArtifactNamespace::Overlay
        })
        .map(|artifact| artifact.artifact_id.as_str())
        .collect();
    assert_eq!(
        seed_kinds.len(),
        4,
        "four workflow seeds recorded as overlay artifacts"
    );
    for want in [
        "owner_control_conversation_seed",
        "email_draft_with_approval_seed",
        "research_and_brief_seed",
        "customer_service_intake_seed",
    ] {
        assert!(
            seed_kinds.contains(&want),
            "missing overlay seed artifact {want}"
        );
    }
}

#[test]
fn materialize_runs_once_per_fresh_install() {
    let store = Store::open_in_memory().unwrap();
    let data_dir = tempdir().unwrap().keep();
    let overlay_dir = data_dir.join("artifacts.d");

    // First boot materializes all four; a persisted marker prevents a second
    // boot from re-materializing (so a seed the owner deletes is not re-created).
    assert_eq!(
        crate::seed_workflows::materialize_missing(&store, &overlay_dir).unwrap(),
        4
    );
    assert_eq!(
        crate::seed_workflows::materialize_missing(&store, &overlay_dir).unwrap(),
        0,
        "second boot must not re-materialize"
    );

    // An owner-deleted seed stays deleted (marker wins over the shipped file).
    let target = overlay_dir
        .join("workflows")
        .join(crate::artifact_loader::overlay_filename(
            "owner_control_conversation_seed",
            1,
        ));
    std::fs::remove_file(&target).unwrap();
    assert_eq!(
        crate::seed_workflows::materialize_missing(&store, &overlay_dir).unwrap(),
        0,
        "deleted seed must not be re-created"
    );
    assert!(
        !target.exists(),
        "deleted seed file must remain absent after a later boot"
    );
}

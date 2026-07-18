use super::*;
use openspine_schemas::action::ActionRequest;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use openspine_schemas::workflow::{
    ApprovalSemantics, ReasoningTier, WorkflowManifest, WorkflowState, WorkflowStep,
    WorkflowStepKind, WorkflowTransition,
};
use ulid::Ulid;

fn digest(ch: char) -> Digest {
    Digest::parse(format!("sha256:{}", ch.to_string().repeat(64))).unwrap()
}

fn manifest() -> WorkflowManifest {
    WorkflowManifest {
        id: "approval_flow".to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: openspine_schemas::artifact::Lifecycle::Active,
        purpose: "test workflow".to_string(),
        required_agent: "agent".to_string(),
        required_capability_pack: "pack".to_string(),
        steps: vec![],
        candidate_allowed_actions: vec![],
        approval_required: vec![],
        denied_actions: vec![],
        initial_state: Some("read".to_string()),
        states: vec![
            WorkflowState {
                id: "read".to_string(),
                steps: vec![WorkflowStep {
                    id: "gather".to_string(),
                    kind: WorkflowStepKind::Deterministic,
                    reasoning_tier: ReasoningTier::Low,
                }],
                approval: ApprovalSemantics::None,
                approval_action: None,
                escalation: None,
            },
            WorkflowState {
                id: "drafted".to_string(),
                steps: vec![WorkflowStep {
                    id: "compose".to_string(),
                    kind: WorkflowStepKind::Agentic,
                    reasoning_tier: ReasoningTier::High,
                }],
                approval: ApprovalSemantics::Required,
                approval_action: Some(ActionId::new("email.create_draft")),
                escalation: None,
            },
            WorkflowState {
                id: "approved".to_string(),
                steps: vec![],
                approval: ApprovalSemantics::None,
                approval_action: None,
                escalation: None,
            },
            WorkflowState {
                id: "done".to_string(),
                steps: vec![],
                approval: ApprovalSemantics::None,
                approval_action: None,
                escalation: None,
            },
        ],
        transitions: vec![
            WorkflowTransition {
                from: "read".to_string(),
                to: "drafted".to_string(),
                event: Some("drafted".to_string()),
            },
            WorkflowTransition {
                from: "drafted".to_string(),
                to: "approved".to_string(),
                event: Some("approved".to_string()),
            },
            WorkflowTransition {
                from: "approved".to_string(),
                to: "done".to_string(),
                event: Some("done".to_string()),
            },
        ],
    }
}

fn insert_request_and_approval(store: &Store, action: &str) -> Ulid {
    insert_request_and_approval_with_expiry(
        store,
        action,
        jiff::Timestamp::now() + std::time::Duration::from_secs(900),
    )
}

fn insert_request_and_approval_with_expiry(
    store: &Store,
    action: &str,
    expires_at: jiff::Timestamp,
) -> Ulid {
    insert_request_and_approval_with_digests(store, action, expires_at, 'a', 'b')
}

fn insert_request_and_approval_with_digests(
    store: &Store,
    action: &str,
    expires_at: jiff::Timestamp,
    payload_char: char,
    target_char: char,
) -> Ulid {
    let request_id = Ulid::new();
    let payload_digest = digest(payload_char);
    let target_digest = digest(target_char);
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
        .insert_approval(&openspine_schemas::approval::ApprovalRecord {
            id: Ulid::new(),
            schema_version: 1,
            action_request_id: request_id,
            approved_by: "owner".to_string(),
            approved_at: jiff::Timestamp::now(),
            approved_payload_digest: payload_digest,
            approved_target_digest: target_digest,
            expires_at,
            decision: openspine_schemas::approval::ApprovalDecision::Approved,
            timeout_behavior: openspine_schemas::approval::TimeoutBehavior::DoNothing,
            approval_channel: "test".to_string(),
        })
        .unwrap();
    request_id
}

fn insert_request_without_approval(store: &Store, action: &str) -> Ulid {
    let request_id = Ulid::new();
    let request = ActionRequest {
        id: request_id,
        task_grant_id: Ulid::new(),
        action: ActionId::new(action),
        target_ref: None,
        payload_ref: Some(ArtifactRef {
            digest: digest('a'),
            schema_version: 1,
        }),
        target_digest: Some(digest('b')),
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        requested_at: jiff::Timestamp::now(),
        schema_version: 1,
    };
    store.insert_action_request(&request).unwrap();
    request_id
}

#[test]
fn entering_approval_state_without_request_is_rejected() {
    let store = Store::open_in_memory().unwrap();
    let mut machine = WorkflowStateMachine::new(&store, "entry-missing", manifest()).unwrap();
    machine.transition_to("drafted", None).unwrap_err();
    assert_eq!(machine.current_state(), Some("read"));
}

#[test]
fn entry_binds_request_and_departure_requires_exact_match() {
    let store = Store::open_in_memory().unwrap();

    let request_id = insert_request_and_approval(&store, "email.create_draft");
    let other_id = insert_request_and_approval(&store, "email.create_draft");

    let mut machine = WorkflowStateMachine::new(&store, "binding", manifest()).unwrap();
    machine.transition_to("drafted", Some(request_id)).unwrap();
    assert_eq!(machine.current_state(), Some("drafted"));

    // Departure without the request id is rejected and writes nothing.
    assert!(matches!(
        machine.transition_to("approved", None),
        Err(WorkflowStateMachineError::ApprovalRequired(_))
    ));
    // A different valid approval does not match the entry binding.
    assert!(matches!(
        machine.transition_to("approved", Some(other_id)),
        Err(WorkflowStateMachineError::ApprovalBindingMismatch)
    ));
    assert_eq!(machine.current_state(), Some("drafted"));
}

#[test]
fn request_without_approval_blocks_departure_without_advancing() {
    let store = Store::open_in_memory().unwrap();
    let request_id = insert_request_without_approval(&store, "email.create_draft");
    let mut machine = WorkflowStateMachine::new(&store, "missing-approval", manifest()).unwrap();
    machine.transition_to("drafted", Some(request_id)).unwrap();
    let error = machine
        .transition_to("approved", Some(request_id))
        .unwrap_err();
    assert!(matches!(error, WorkflowStateMachineError::ApprovalMissing));
    assert_eq!(machine.current_state(), Some("drafted"));
}

#[test]
fn valid_bound_approval_permits_departure_and_rehydrate_next_transition() {
    let store = Store::open_in_memory().unwrap();
    let request_id = insert_request_and_approval(&store, "email.create_draft");
    let mut machine = WorkflowStateMachine::new(&store, "valid", manifest()).unwrap();
    machine.transition_to("drafted", Some(request_id)).unwrap();
    machine.transition_to("approved", Some(request_id)).unwrap();
    assert_eq!(machine.current_state(), Some("approved"));

    let mut recovered = WorkflowStateMachine::new(&store, "valid", manifest()).unwrap();
    recovered.transition_to("done", None).unwrap();
    assert_eq!(recovered.current_state(), Some("done"));
}

#[test]
fn cyclic_approval_replay_uses_each_visit_binding() {
    let store = Store::open_in_memory().unwrap();
    let first = insert_request_and_approval(&store, "email.create_draft");
    let second = insert_request_and_approval_with_digests(
        &store,
        "email.create_draft",
        jiff::Timestamp::now() + std::time::Duration::from_secs(900),
        'c',
        'd',
    );
    let mut workflow = manifest();
    workflow.transitions.push(WorkflowTransition {
        from: "approved".to_string(),
        to: "drafted".to_string(),
        event: Some("revise".to_string()),
    });
    let mut machine = WorkflowStateMachine::new(&store, "cycle", workflow.clone()).unwrap();
    machine.transition_to("drafted", Some(first)).unwrap();
    machine.transition_to("approved", Some(first)).unwrap();
    machine.transition_to("drafted", Some(second)).unwrap();
    machine.transition_to("approved", Some(second)).unwrap();

    // A later mutable decision must not invalidate already committed history.
    store
        .insert_approval(&openspine_schemas::approval::ApprovalRecord {
            id: Ulid::new(),
            schema_version: 1,
            action_request_id: second,
            approved_by: "owner".to_string(),
            approved_at: jiff::Timestamp::now(),
            approved_payload_digest: digest('c'),
            approved_target_digest: digest('d'),
            expires_at: jiff::Timestamp::now() + std::time::Duration::from_secs(900),
            decision: openspine_schemas::approval::ApprovalDecision::Rejected,
            timeout_behavior: openspine_schemas::approval::TimeoutBehavior::DoNothing,
            approval_channel: "test-revocation".to_string(),
        })
        .unwrap();
    let recovered = WorkflowStateMachine::new(&store, "cycle", workflow).unwrap();
    assert_eq!(recovered.current_state(), Some("approved"));
}

#[test]
fn entry_rejects_request_whose_action_differs_from_state() {
    let store = Store::open_in_memory().unwrap();
    let request_id = insert_request_and_approval(&store, "telegram.reply:owner_channel");
    let mut machine = WorkflowStateMachine::new(&store, "wrong-action", manifest()).unwrap();
    assert!(matches!(
        machine.transition_to("drafted", Some(request_id)),
        Err(WorkflowStateMachineError::ApprovalActionMismatch)
    ));
}

#[test]
fn manifest_change_on_resume_fails_closed() {
    let store = Store::open_in_memory().unwrap();
    let request_id = insert_request_and_approval(&store, "email.create_draft");
    let mut machine = WorkflowStateMachine::new(&store, "drift", manifest()).unwrap();
    machine.transition_to("drafted", Some(request_id)).unwrap();
    machine.transition_to("approved", Some(request_id)).unwrap();

    let mut drifted = manifest();
    drifted.purpose = "tampered purpose changes the digest".to_string();
    let result = WorkflowStateMachine::new(&store, "drift", drifted);
    match result {
        Err(WorkflowStateMachineError::InvalidDefinition(_)) => {}
        Err(other) => panic!("unexpected error: {other}"),
        Ok(_) => panic!("manifest drift unexpectedly resumed"),
    }
}

#[test]
fn declared_step_tier_routes_through_gateway_map() {
    let state = crate::test_support::fixtures::test_state();
    let machine = WorkflowStateMachine::new(&state.store, "tier", manifest()).unwrap();
    assert_eq!(
        machine.step("compose").unwrap().reasoning_tier,
        ReasoningTier::High
    );
    let active_provider = state
        .active_model_providers
        .read()
        .get(&openspine_schemas::model_swap::ModelRole::Base)
        .cloned()
        .unwrap();
    let selected = machine
        .provider_for_step(
            "compose",
            &state.gateway_tier_map,
            &active_provider,
            &state.provider_pool,
        )
        .expect("the active model provider must be a usable gateway fallback");
    assert!(std::ptr::eq(
        selected,
        state.provider_pool.get(&active_provider).unwrap()
    ));
}

#[test]
fn pending_entry_binding_rehydrates_source_and_completes_atomically() {
    let store = Store::open_in_memory().unwrap();
    let request_id = insert_request_and_approval(&store, "email.create_draft");
    let alternate_request_id = insert_request_and_approval(&store, "email.create_draft");
    let workflow = manifest();
    let mut ctx = WorkflowCtx::new_with_definition(
        &store,
        "pending-entry",
        workflow.id.clone(),
        workflow.version.to_string(),
    )
    .unwrap();
    let definition = super::manifest_digest(&workflow).unwrap();
    let definition_step = match ctx.begin_definition_step::<Digest>(&definition).unwrap() {
        StepState::Fresh { handle, .. } => handle,
        _ => panic!("new run definition must be fresh"),
    };
    ctx.complete_step(&definition_step, Ok::<_, String>(definition))
        .unwrap();
    let binding = EntryBindingInputs {
        target_state: "drafted".to_string(),
        request_id: request_id.to_string(),
        action: "email.create_draft".to_string(),
        payload_digest: digest('a'),
        target_digest: digest('b'),
    };
    let _pending = ctx.begin_entry_binding_step("read", &binding).unwrap();
    let mut recovered =
        WorkflowStateMachine::new(&store, "pending-entry", workflow.clone()).unwrap();
    assert_eq!(recovered.current_state(), Some("read"));
    let wrong = recovered.transition_to("drafted", Some(alternate_request_id));
    assert!(matches!(
        wrong,
        Err(WorkflowStateMachineError::Workflow(
            WorkflowError::Divergence { .. }
        ))
    ));
    assert_eq!(recovered.current_state(), Some("read"));

    let mut recovered = WorkflowStateMachine::new(&store, "pending-entry", workflow).unwrap();
    assert_eq!(recovered.current_state(), Some("read"));
    recovered
        .transition_to("drafted", Some(request_id))
        .unwrap();
    assert_eq!(recovered.current_state(), Some("drafted"));
}

#[test]
fn failed_reserved_step_does_not_reset_reconstructed_target() {
    let store = Store::open_in_memory().unwrap();
    let request_id = insert_request_without_approval(&store, "email.create_draft");
    let mut ctx = WorkflowCtx::new_with_definition(&store, "scan", "approval_flow", "1").unwrap();
    let transition = match ctx.begin_transition_step("read", "drafted").unwrap() {
        StepState::Fresh { handle, .. } => handle,
        _ => panic!("transition must be fresh"),
    };
    ctx.complete_step(
        &transition,
        Ok::<_, String>(TransitionOutcome {
            target: "drafted".to_string(),
        }),
    )
    .unwrap();
    let binding = EntryBindingInputs {
        target_state: "drafted".to_string(),
        request_id: request_id.to_string(),
        action: "email.create_draft".to_string(),
        payload_digest: digest('a'),
        target_digest: digest('b'),
    };
    assert!(ctx
        .begin_authorized_gated_departure_step("read", "drafted", &binding)
        .is_err());
    assert_eq!(
        ctx.last_completed_transition_target().unwrap().as_deref(),
        Some("drafted")
    );
}

#[test]
fn forged_transition_outcome_is_rejected_on_rehydrate() {
    let store = Store::open_in_memory().unwrap();
    let workflow = manifest();
    let mut ctx =
        WorkflowCtx::new_with_definition(&store, "forged", workflow.id.clone(), "1").unwrap();
    let forged = match ctx.begin_transition_step("read", "drafted").unwrap() {
        StepState::Fresh { handle, .. } => handle,
        _ => panic!("transition must be fresh"),
    };

    ctx.complete_step(
        &forged,
        Ok::<_, String>(TransitionOutcome {
            target: "approved".to_string(),
        }),
    )
    .unwrap();
    let result = WorkflowStateMachine::new(&store, "forged", workflow);
    assert!(result.is_err(), "forged transition unexpectedly rehydrated");
}

#[test]
fn expired_completed_approval_rehydrates_from_immutable_step_proof() {
    let store = Store::open_in_memory().unwrap();
    let request_id = insert_request_and_approval_with_expiry(
        &store,
        "email.create_draft",
        jiff::Timestamp::now() + std::time::Duration::from_secs(1),
    );
    let workflow = manifest();
    let mut machine =
        WorkflowStateMachine::new(&store, "expired-completed", workflow.clone()).unwrap();
    machine.transition_to("drafted", Some(request_id)).unwrap();
    machine.transition_to("approved", Some(request_id)).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1_100));

    let recovered = WorkflowStateMachine::new(&store, "expired-completed", workflow).unwrap();
    assert_eq!(recovered.current_state(), Some("approved"));
}

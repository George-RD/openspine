use crate::api::actions::{mediate_and_dispatch_action, FailureSurface};
use crate::pipeline::{dispatch_task_timer_event, TimerDispatchOutcome};
use crate::test_support::fixtures::test_state;
use jiff::Timestamp;
use openspine_schemas::action::{ActionId, GateDecision};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use openspine_schemas::principal::Principal;
use openspine_schemas::task::{Task, TaskProvenance, TaskStatus, TaskTimerKind};
use ulid::Ulid;
fn ref_of(byte: char) -> ArtifactRef {
    ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).unwrap(),
        schema_version: 1,
    }
}
fn task(state: &crate::pipeline::AppState, timer_kind: TaskTimerKind) -> Task {
    let id = Ulid::new();
    Task {
        schema_version: 1,
        id,
        owner_principal_id: state.owner_principal_id,
        status: TaskStatus::Open,
        owning_worker: Some(
            openspine_schemas::task::WorkerId::new("main_assistant_agent").unwrap(),
        ),
        owning_grant_id: None,
        due_at: Some(Timestamp::from_second(10).unwrap()),
        reminder_at: (timer_kind == TaskTimerKind::Reminder)
            .then(|| Timestamp::from_second(10).unwrap()),
        due_timer_id: None,
        reminder_timer_id: None,
        dependencies: vec![],
        provenance: TaskProvenance::AskedAbout {
            reference: ref_of('a'),
            asked_at: Timestamp::from_second(1).unwrap(),
        },
        title_ref: ref_of('b'),
        created_at: Timestamp::from_second(1).unwrap(),
    }
}
async fn fires_task_timer_and_reaches_worker_gate(kind: TaskTimerKind) {
    let state = test_state();
    let row = task(&state, kind);
    let timer_id = row.id.to_string();
    state.store.insert_task(&row).unwrap();
    state
        .store
        .schedule_task_timer(
            &timer_id,
            &row.id.to_string(),
            kind,
            Timestamp::from_second(10).unwrap(),
        )
        .unwrap();
    let fired = state
        .store
        .fire_due_timers(Timestamp::from_second(10).unwrap())
        .unwrap();
    assert_eq!(fired.len(), 1);
    let grant = match dispatch_task_timer_event(&state, &fired[0]).await.unwrap() {
        TimerDispatchOutcome::Delivered { grant } => *grant,
        other => panic!("task timer must produce a worker grant, got {other:?}"),
    };
    assert_eq!(
        grant.route_id,
        match kind {
            TaskTimerKind::Deadline => "task_deadline_fired",
            TaskTimerKind::Reminder => "task_reminder_fired",
        }
    );
    assert!(grant
        .allowed_actions
        .contains(&ActionId::new("telegram.reply:owner_channel")));
    assert!(grant
        .allowed_actions
        .contains(&ActionId::new("openspine.status.read")));
    assert!(grant
        .denied_actions
        .contains(&ActionId::new("artifact.propose")));
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("workflow.timer_fired")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("event.received")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("authority.granted")
            .unwrap(),
        1
    );
    let (_, pending_ref, _) = state
        .store
        .find_task_grant_by_token(&grant.task_token)
        .unwrap()
        .expect("scheduled worker grant must be persisted");
    let pending = state.artifacts.get(&pending_ref).unwrap();
    let pending_json = String::from_utf8(pending).unwrap();
    assert!(pending_json.len() < 4096);
    assert!(!pending_json.contains("dependencies"));
    assert!(!pending_json.contains("provenance"));
    assert!(!pending_json.contains("owning_grant_id"));
    let (decision, _, _) = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("openspine.status.read"),
        state.owner_user_id,
        None,
        FailureSurface::Detached,
    )
    .await
    .unwrap();
    assert_eq!(decision, GateDecision::Allow);
    let (proposal_decision, _, _) = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("artifact.propose"),
        state.owner_user_id,
        None,
        FailureSurface::Detached,
    )
    .await
    .unwrap();
    assert!(matches!(proposal_decision, GateDecision::Deny { .. }));
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("action.gated")
            .unwrap(),
        2
    );
}
#[tokio::test]
async fn timer_retry_does_not_advance_consumer_checkpoint() {
    let state = test_state();
    let dependency = task(&state, TaskTimerKind::Deadline);
    state.store.insert_task(&dependency).unwrap();
    let mut row = task(&state, TaskTimerKind::Deadline);
    row.dependencies = vec![dependency.id];
    state.store.insert_task(&row).unwrap();
    let before = state
        .store
        .load_consumer_checkpoint("task_board_timer_consumer")
        .unwrap();
    let wake = crate::store::task_board::DependencyWake {
        task_id: row.id,
        timer_id: row.id.to_string(),
        dependency_id: dependency.id,
        wake_key: format!("wake:{}", row.id),
    };
    let outcome = super::super::dispatch_task_wake(&state, &wake)
        .await
        .unwrap();
    assert!(matches!(outcome, TimerDispatchOutcome::Retry));
    let after = state
        .store
        .load_consumer_checkpoint("task_board_timer_consumer")
        .unwrap();
    assert_eq!(after, before);
}
#[tokio::test]
async fn linked_fired_event_after_task_done_ack_skips_without_grant() {
    let state = test_state();
    let row = task(&state, TaskTimerKind::Deadline);
    let timer_id = row.id.to_string();
    state.store.insert_task(&row).unwrap();
    state
        .store
        .schedule_task_timer(
            &timer_id,
            &row.id.to_string(),
            TaskTimerKind::Deadline,
            Timestamp::from_second(10).unwrap(),
        )
        .unwrap();
    let fired = state
        .store
        .fire_due_timers(Timestamp::from_second(10).unwrap())
        .unwrap();
    state.store.mark_task_done_and_poll(row.id).unwrap();
    let outcome = dispatch_task_timer_event(&state, &fired[0]).await.unwrap();
    assert!(matches!(outcome, TimerDispatchOutcome::AckSkip));
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
}
#[tokio::test]
async fn linked_fired_event_after_task_cancelled_ack_skips_without_grant() {
    let state = test_state();
    let row = task(&state, TaskTimerKind::Deadline);
    let timer_id = row.id.to_string();
    state.store.insert_task(&row).unwrap();
    state
        .store
        .schedule_task_timer(
            &timer_id,
            &row.id.to_string(),
            TaskTimerKind::Deadline,
            Timestamp::from_second(10).unwrap(),
        )
        .unwrap();
    let fired = state
        .store
        .fire_due_timers(Timestamp::from_second(10).unwrap())
        .unwrap();
    let mut cancelled = state.store.get_task(row.id).unwrap().unwrap();
    cancelled.status = TaskStatus::Cancelled;
    state
        .store
        .conn
        .lock()
        .execute(
            "UPDATE task_board SET status = 'cancelled', task_json = ?1 WHERE id = ?2",
            rusqlite::params![
                serde_json::to_string(&cancelled).unwrap(),
                row.id.to_string()
            ],
        )
        .unwrap();
    let outcome = dispatch_task_timer_event(&state, &fired[0]).await.unwrap();
    assert!(matches!(outcome, TimerDispatchOutcome::AckSkip));
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
}
#[tokio::test]
async fn unknown_task_owner_ack_skips_without_grant() {
    let state = test_state();
    let mut row = task(&state, TaskTimerKind::Deadline);
    row.owner_principal_id = Ulid::new();
    let timer_id = row.id.to_string();
    state.store.insert_task(&row).unwrap();
    state
        .store
        .schedule_task_timer(
            &timer_id,
            &row.id.to_string(),
            TaskTimerKind::Deadline,
            Timestamp::from_second(10).unwrap(),
        )
        .unwrap();
    let fired = state
        .store
        .fire_due_timers(Timestamp::from_second(10).unwrap())
        .unwrap();
    let outcome = dispatch_task_timer_event(&state, &fired[0]).await.unwrap();
    assert!(matches!(outcome, TimerDispatchOutcome::AckSkip));
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
}
#[tokio::test]
async fn timer_event_is_idempotent_in_grant_transaction() {
    let state = test_state();
    let row = task(&state, TaskTimerKind::Deadline);
    let timer_id = row.id.to_string();
    state.store.insert_task(&row).unwrap();
    state
        .store
        .schedule_task_timer(
            &timer_id,
            &row.id.to_string(),
            TaskTimerKind::Deadline,
            Timestamp::from_second(10).unwrap(),
        )
        .unwrap();
    let fired = state
        .store
        .fire_due_timers(Timestamp::from_second(10).unwrap())
        .unwrap();
    let first = dispatch_task_timer_event(&state, &fired[0]).await.unwrap();
    assert!(matches!(first, TimerDispatchOutcome::Delivered { .. }));
    let second = dispatch_task_timer_event(&state, &fired[0]).await.unwrap();
    assert!(matches!(second, TimerDispatchOutcome::AckSkip));
    assert_eq!(state.store.count_task_grants().unwrap(), 1);
}
#[tokio::test]
async fn unmet_dependency_blocks_task_and_ack_skips_timer() {
    let state = test_state();
    let mut row = task(&state, TaskTimerKind::Deadline);
    row.dependencies = vec![Ulid::new()];
    let timer_id = row.id.to_string();
    state.store.insert_task(&row).unwrap();
    state
        .store
        .schedule_task_timer(
            &timer_id,
            &row.id.to_string(),
            TaskTimerKind::Deadline,
            Timestamp::from_second(10).unwrap(),
        )
        .unwrap();
    let fired = state
        .store
        .fire_due_timers(Timestamp::from_second(10).unwrap())
        .unwrap();
    let outcome = dispatch_task_timer_event(&state, &fired[0]).await.unwrap();
    assert!(matches!(outcome, TimerDispatchOutcome::AckSkip));
    assert_eq!(
        state.store.get_task(row.id).unwrap().unwrap().status,
        TaskStatus::Blocked
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("task.blocked")
            .unwrap(),
        1
    );
}
#[tokio::test]
async fn timer_rejects_non_owner_task_principal() {
    let state = test_state();
    let other_owner = Ulid::new();
    state
        .store
        .insert_raw_principal_for_test(&Principal {
            id: other_owner,
            identity_id: Ulid::new(),
            is_owner: false,
            schema_version: 1,
        })
        .unwrap();
    let mut row = task(&state, TaskTimerKind::Deadline);
    row.owner_principal_id = other_owner;
    let timer_id = row.id.to_string();
    state.store.insert_task(&row).unwrap();
    state
        .store
        .schedule_task_timer(
            &timer_id,
            &row.id.to_string(),
            TaskTimerKind::Deadline,
            Timestamp::from_second(10).unwrap(),
        )
        .unwrap();
    let fired = state
        .store
        .fire_due_timers(Timestamp::from_second(10).unwrap())
        .unwrap();
    let outcome = dispatch_task_timer_event(&state, &fired[0]).await.unwrap();
    assert!(matches!(outcome, TimerDispatchOutcome::AckSkip));
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
}
#[tokio::test]
async fn deadline_timer_reaches_routed_granted_and_worker_gate() {
    fires_task_timer_and_reaches_worker_gate(TaskTimerKind::Deadline).await;
}
#[tokio::test]
async fn reminder_timer_uses_precise_route_and_worker_gate() {
    fires_task_timer_and_reaches_worker_gate(TaskTimerKind::Reminder).await;
}
#[tokio::test]
async fn authority_granted_audit_is_atomic_with_grant_persist() {
    let state = test_state();
    let row = task(&state, TaskTimerKind::Deadline);
    state.store.insert_task(&row).unwrap();
    let timer_id = row.id.to_string();
    state
        .store
        .schedule_task_timer(
            &timer_id,
            &row.id.to_string(),
            TaskTimerKind::Deadline,
            Timestamp::from_second(10).unwrap(),
        )
        .unwrap();
    let fired = state
        .store
        .fire_due_timers(Timestamp::from_second(10).unwrap())
        .unwrap();
    let outcome = dispatch_task_timer_event(&state, &fired[0]).await.unwrap();
    assert!(matches!(outcome, TimerDispatchOutcome::Delivered { .. }));
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("authority.granted")
            .unwrap(),
        1
    );
}
#[tokio::test]
async fn recovery_refuses_receiptless_handed_off_redispatch() {
    let state = test_state();
    let row = task(&state, TaskTimerKind::Deadline);
    state.store.insert_task(&row).unwrap();
    let timer_id = row.id.to_string();
    state
        .store
        .schedule_task_timer(
            &timer_id,
            &row.id.to_string(),
            TaskTimerKind::Deadline,
            Timestamp::from_second(10).unwrap(),
        )
        .unwrap();
    let fired = state
        .store
        .fire_due_timers(Timestamp::from_second(10).unwrap())
        .unwrap();
    let key = fired[0].id.to_string();
    let mut grant = openspine_schemas::grant::TaskGrant {
        id: Ulid::new(),
        schema_version: 1,
        lifecycle_state: openspine_schemas::artifact::Lifecycle::Active,
        user: state.owner_principal_id.to_string(),
        purpose: "timer-task".to_string(),
        issued_by: "timer".to_string(),
        issued_at: Timestamp::now(),
        expires_at: Timestamp::now() + std::time::Duration::from_secs(60),
        event_id: Ulid::from_string(&key).unwrap(),
        route_id: "scheduled_timer".to_string(),
        agent_id: "main_assistant_agent".to_string(),
        workflow_id: "task_board_scheduled".to_string(),
        capability_pack_id: "scheduled_timer_pack".to_string(),
        authority_sources: vec![],
        selection_tokens: vec![],
        allowed_actions: vec![],
        approval_required_actions: vec![],
        denied_actions: vec![],
        allowed_egress_classes: vec![],
        output_channels: vec![],
        limits: openspine_schemas::grant::GrantLimits {
            max_model_calls: 5,
            max_artifacts: 5,
            max_runtime_seconds: 60,
        },
        task_token: "a".repeat(64),
        root_grant_id: Ulid::nil(),
        parent_grant_id: None,
        mode: openspine_schemas::grant::GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
    };
    grant.seal_root(crate::grant_hmac_key().as_ref().unwrap());
    let token_ref = state.artifacts.put(grant.task_token.as_bytes()).unwrap();
    let pending_ref = openspine_schemas::artifact::ArtifactRef {
        digest: openspine_schemas::digest::Digest::parse(format!("sha256:{}", "c".repeat(64)))
            .unwrap(),
        schema_version: 1,
    };
    state
        .store
        .persist_grant_with_handoff(
            &key,
            &grant,
            &pending_ref,
            state.owner_user_id,
            &token_ref,
            &timer_id,
            Some(row.id),
        )
        .unwrap();
    crate::pipeline::recover_timer_dispatches(&state)
        .await
        .unwrap();
    let dispatches = state.store.incomplete_timer_dispatches().unwrap();
    assert!(
        dispatches.is_empty(),
        "dispatch should be terminal, not incomplete"
    );
    let record = state.store.dispatch_state_for_key(&key).unwrap().unwrap();
    assert_eq!(
        record.state,
        crate::store::task_board::TimerDispatchState::Terminal
    );
    assert_eq!(
        record.terminal_reason.as_deref(),
        Some("receiptless_handoff_refused")
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("task.dispatch_refused")
            .unwrap(),
        1
    );
}

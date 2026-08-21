//! Split from `task_board.rs` to keep that file under the 500-line gate.
//! Submodule of `task_board`, so `super::*` brings its fixtures (`task`,
//! `dispatch_task_timer_event`, `mediate_and_dispatch_action`, ...).
use super::*;

/// #202 (spec #197 testing decision 7, Task -> grant half): a typed `Task`'s
/// `owner_principal_id` composes a task grant whose `user` is exactly that
/// owner `PrincipalId`, and the composed grant passes `gate()` for an allowed
/// action. This is the "typed Task -> composed grant.user -> gate" seam; the
/// grant -> approval-callback -> approval/audit-actor half of the chain is
/// covered by the plan-approval E2E in `plan.rs`.
#[tokio::test]
async fn typed_task_owner_principal_composes_grant_user_through_gate() {
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
    assert_eq!(fired.len(), 1);
    let grant = match dispatch_task_timer_event(&state, &fired[0]).await.unwrap() {
        TimerDispatchOutcome::Delivered { grant } => *grant,
        other => panic!("task timer must produce a worker grant, got {other:?}"),
    };
    assert_eq!(
        grant.user,
        openspine_schemas::ids::PrincipalId::from(state.owner_principal_id),
        "composed grant.user is the Task's owner principal id (AD-146)"
    );
    let (decision, _, _, _) = mediate_and_dispatch_action(
        &state,
        &grant,
        ActionId::new("openspine.status.read"),
        &state.telegram_owner_surface(),
        None,
        FailureSurface::Detached,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        decision,
        GateDecision::Allow,
        "the composed grant is the live authority and passes gate (D-007)"
    );
}

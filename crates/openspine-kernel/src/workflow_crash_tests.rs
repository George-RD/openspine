use super::*;
use tempfile::tempdir;

#[test]
fn crash_child_pending_then_parent_recovers_resuming() {
    let phase = std::env::var("OPENSPINE_CRASH_PHASE").ok();
    let dir = tempdir().unwrap();
    let db = std::env::var_os("OPENSPINE_CRASH_DB")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| dir.path().join("crash-pending.db"));
    if phase.as_deref() == Some("pending") {
        let store = Store::open(&db).unwrap();
        let mut workflow = WorkflowCtx::new(&store, "crash-pending").unwrap();
        let _ = workflow
            .begin_step::<u32>("workflow.connector_call", &"pending")
            .unwrap();
        std::process::abort();
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .env("OPENSPINE_CRASH_PHASE", "pending")
        .arg("--nocapture")
        .arg("crash_child_pending_then_parent_recovers_resuming")
        .env("OPENSPINE_CRASH_DB", &db)
        .status()
        .unwrap();
    assert!(!status.success(), "child must terminate by abort");
    let store = Store::open(&db).unwrap();
    let mut recovered = WorkflowCtx::new(&store, "crash-pending").unwrap();
    assert!(matches!(
        recovered
            .begin_step::<u32>("workflow.connector_call", &"pending")
            .unwrap(),
        StepState::Resuming { .. }
    ));
    assert!(store.verify_audit_chain().unwrap());
}

#[test]
fn crash_child_resuming_then_parent_recovers_without_redispatch() {
    let phase = std::env::var("OPENSPINE_CRASH_PHASE").ok();
    let dir = tempdir().unwrap();
    let db = std::env::var_os("OPENSPINE_CRASH_DB")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| dir.path().join("crash-resuming.db"));
    if phase.as_deref() == Some("resuming") {
        let store = Store::open(&db).unwrap();
        let mut workflow = WorkflowCtx::new(&store, "crash-resuming").unwrap();
        let first = match workflow
            .begin_step::<u32>("workflow.connector_call", &"completed")
            .unwrap()
        {
            StepState::Fresh { handle, .. } => handle,
            _ => panic!("child expected Fresh"),
        };
        workflow
            .complete_step(&first, Ok::<_, String>(1u32))
            .unwrap();
        let _ = workflow
            .begin_step::<u32>("workflow.connector_call", &"resuming")
            .unwrap();
        std::process::abort();
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .env("OPENSPINE_CRASH_PHASE", "resuming")
        .arg("--nocapture")
        .arg("crash_child_resuming_then_parent_recovers_without_redispatch")
        .env("OPENSPINE_CRASH_DB", &db)
        .status()
        .unwrap();
    assert!(!status.success(), "child must terminate by abort");
    let store = Store::open(&db).unwrap();
    let mut recovered = WorkflowCtx::new(&store, "crash-resuming").unwrap();
    assert!(matches!(
        recovered
            .begin_step::<u32>("workflow.connector_call", &"completed")
            .unwrap(),
        StepState::Replayed { outcome: Ok(1), .. }
    ));
    assert!(matches!(
        recovered
            .begin_step::<u32>("workflow.connector_call", &"resuming")
            .unwrap(),
        StepState::Resuming { .. }
    ));
    assert!(store.verify_audit_chain().unwrap());
}

#[test]
fn timer_effect_path_is_internal_and_emits_one_terminal_audit() {
    let catalog = crate::action_catalog::canonical_catalog();
    let timer_path = catalog
        .effect_paths()
        .iter()
        .find(|path| path.name == "fire_due_workflow_timers")
        .expect("timer effect path must be cataloged");
    assert_eq!(
        timer_path.classification,
        openspine_schemas::action::EffectPathClass::InternalMaintenanceNonEffect
    );
    let store = Store::open_in_memory().unwrap();
    let due = Timestamp::now() + std::time::Duration::from_secs(60);
    let mut workflow = WorkflowCtx::new(&store, "timer-catalog").unwrap();
    let _timer = workflow.schedule_timer(due).unwrap();
    let fire_at = due + std::time::Duration::from_secs(60);
    assert_eq!(store.due_timers(fire_at).unwrap().len(), 1);
    assert_eq!(store.fire_due_timers(fire_at).unwrap().len(), 1);
    assert_eq!(
        store
            .count_audit_events_of_kind("workflow.timer_fired")
            .unwrap(),
        1
    );
    assert_eq!(store.count_audit_events_of_kind("action.gated").unwrap(), 0);
}
/// Production-path recovery contract for a persisted Pending gated step.
///
/// This is the blocker fix: the prior crash test proved `begin_step`
/// idempotency at the test layer, but never exercised the production
/// `run_gated_step` recovery path. Here the first attempt records the exact
/// `Pending` outbox intent `run_gated_step` would write (same
/// `GatedStepDigest`, same `begin_step`), then "crashes" by dropping the
/// in-memory workflow context. A fresh `AppState` reopens the ledger and
/// invokes the REAL production `run_gated_step` for the exact handle.
///
/// The production path MUST fail closed: the Resuming branch has no durable
/// receipt, so it refuses to redispatch. We assert the dispatcher was never
/// invoked (`action.gated` stays 0), the sole action-kind ledger row is
/// still the original `Pending` (no fabricated receipt/completion), and the
/// audit chain still verifies. If the Resuming branch were to redispatch,
/// `action.gated` would increment and a completion row would be fabricated —
/// both assertions would fail.
#[tokio::test]
async fn gated_step_persisted_pending_recovers_without_redispatch() {
    use crate::pipeline::approval_fixture_grant;
    use crate::test_support::fixtures::test_state_with_store;

    let dir = tempdir().unwrap();
    let db = dir.path().join("gated-pending.db");
    let run_id = "gated-pending";
    let action = ActionId::new("openspine.status.read");
    let bound_chat_id = 555i64;
    let grant = approval_fixture_grant();

    // Phase 1 — first attempt: durably record the gated step's `Pending`
    // intent (exactly what production `run_gated_step` persists before the
    // effect) and then "crash" by dropping the workflow context.
    let seeded_handle = {
        let store = Store::open(&db).unwrap();
        let state = test_state_with_store(store);
        let mut workflow = WorkflowCtx::new(&state.store, run_id).unwrap();
        let gated = GatedStepDigest {
            action: action.to_string(),
            grant_id: grant.id.to_string(),
            bound_chat_id,
            inputs_digest: digest_inputs(&()).unwrap(),
            payload_digest: None,
        };
        let seeded = workflow
            .begin_step::<ArtifactRef>(&action.to_string(), &gated)
            .unwrap();
        // Crash boundary: `workflow` and its `AppState`/store drop here; only
        // the on-disk `Pending` ledger row survives, exactly as after a real
        // process abort.
        match seeded {
            StepState::Fresh { handle, .. } => handle,
            _ => panic!("seeded gated step must be fresh"),
        }
    };

    // Phase 2 — restart: reopen the ledger and invoke the PRODUCTION
    // recovery path for the exact same handle.
    let store = Store::open(&db).unwrap();
    let state = test_state_with_store(store);
    let mut recovered = WorkflowCtx::new(&state.store, run_id).unwrap();
    let result = recovered
        .run_gated_step(
            &state,
            &grant,
            &state.artifacts,
            action.clone(),
            bound_chat_id,
            None,
            &(),
        )
        .await;

    // Fail-closed: a Pending step with no durable receipt is refused, never
    // redispatched or completed with a fabricated outcome.
    assert!(
        matches!(&result, Err(WorkflowError::Step(msg)) if msg.contains("refusing to re-dispatch")),
        "expected fail-closed refusal, got a non-error result"
    );
    // The exact seeded handle is what recovery inspected: still Pending, no
    // receipt, no fabricated completion.
    assert_eq!(
        recovered.steps.len(),
        1,
        "exactly one step must be rehydrated"
    );
    assert!(
        recovered.steps[0].receipt.is_none(),
        "no receipt may be recorded for a Pending step on recovery"
    );
    assert!(
        recovered.steps[0].completed.is_none(),
        "recovery must not fabricate a completion for the Pending step"
    );
    assert_eq!(
        recovered.steps[0].step_id, seeded_handle.step_id,
        "recovery must target the exact seeded gated handle"
    );
    // The gate/dispatcher was never invoked on recovery.
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("action.gated")
            .unwrap(),
        0,
        "recovery must not redispatch the gated step"
    );
    // The only action-kind ledger row is the original `Pending`; no receipt
    // or completion was fabricated by recovery.
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind(&action.to_string())
            .unwrap(),
        1,
        "recovery must not fabricate a completion for the Pending step"
    );
    assert!(state.store.verify_audit_chain().unwrap());
}

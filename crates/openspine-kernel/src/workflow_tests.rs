use super::*;
use crate::artifact_store::ArtifactStore;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tempfile::tempdir;

fn complete_u32(workflow: &mut WorkflowCtx<'_>, kind: &str, input: &str, value: u32) {
    match workflow.begin_step::<u32>(kind, &input).unwrap() {
        StepState::Fresh { handle, .. } | StepState::Resuming { handle, .. } => workflow
            .complete_step(&handle, Ok::<_, String>(value))
            .unwrap(),
        StepState::Replayed { .. } => {}
    }
}

#[test]
fn kill_and_recover_replays_without_rerunning_steps() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("kernel.db");
    let calls = Arc::new(AtomicUsize::new(0));
    {
        let store = Store::open(&db).unwrap();
        let mut workflow = WorkflowCtx::new(&store, "run").unwrap();
        let handle = match workflow
            .begin_step::<u32>("workflow.connector_call", &"thread")
            .unwrap()
        {
            StepState::Fresh { handle, .. } => handle,
            _ => panic!(),
        };
        calls.fetch_add(1, Ordering::SeqCst);
        workflow
            .complete_step(&handle, Ok::<_, String>(7u32))
            .unwrap();
        let handle = match workflow
            .begin_step::<u64>("workflow.model_call", &"prompt")
            .unwrap()
        {
            StepState::Fresh { handle, .. } => handle,
            _ => panic!(),
        };
        calls.fetch_add(1, Ordering::SeqCst);
        workflow
            .complete_step(&handle, Ok::<_, String>(42u64))
            .unwrap();
    }
    let store = Store::open(&db).unwrap();
    let mut recovered = WorkflowCtx::new(&store, "run").unwrap();
    assert!(matches!(
        recovered
            .begin_step::<u32>("workflow.connector_call", &"thread")
            .unwrap(),
        StepState::Replayed { outcome: Ok(7), .. }
    ));
    assert!(matches!(
        recovered
            .begin_step::<u64>("workflow.model_call", &"prompt")
            .unwrap(),
        StepState::Replayed {
            outcome: Ok(42),
            ..
        }
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(store.verify_audit_chain().unwrap());
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct ModelOutput {
    body: String,
}

#[test]
fn private_outcome_replays_and_missing_artifact_fails_closed() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("kernel.db");
    let root = dir.path().join("artifacts");
    let secret = ModelOutput {
        body: "secret".into(),
    };
    {
        let store = Store::open(&db).unwrap();
        let artifacts = ArtifactStore::open(root.clone(), [1u8; 32]).unwrap();
        let mut workflow = WorkflowCtx::new(&store, "private").unwrap();
        let handle = match workflow
            .begin_private_step::<ModelOutput>("workflow.model_call", &"prompt", &artifacts)
            .unwrap()
        {
            StepState::Fresh { handle, .. } => handle,
            _ => panic!(),
        };
        workflow
            .complete_private_step(&handle, Ok::<_, String>(secret.clone()), &artifacts)
            .unwrap();
    }
    let store = Store::open(&db).unwrap();
    let artifacts = ArtifactStore::open(root.clone(), [1u8; 32]).unwrap();
    let mut workflow = WorkflowCtx::new(&store, "private").unwrap();
    assert!(
        matches!(workflow.begin_private_step::<ModelOutput>("workflow.model_call", &"prompt", &artifacts).unwrap(), StepState::Replayed { outcome: Ok(value), .. } if value == secret)
    );
    std::fs::remove_dir_all(&root).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let artifacts = ArtifactStore::open(root, [9u8; 32]).unwrap();
    let mut workflow = WorkflowCtx::new(&store, "private").unwrap();
    assert!(matches!(
        workflow.begin_private_step::<ModelOutput>("workflow.model_call", &"prompt", &artifacts),
        Err(WorkflowError::Step(_))
    ));
}

#[test]
fn timer_driver_fires_once_across_recovery_and_stale_contexts() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("timers.db");
    let due = Timestamp::from_second(10).unwrap();
    {
        let store = Store::open(&db).unwrap();
        let mut workflow = WorkflowCtx::new(&store, "timer-run").unwrap();
        let timer = workflow.schedule_timer(due).unwrap();
        assert!(!workflow.poll_timer(&timer).unwrap());
    }
    let store = Store::open(&db).unwrap();
    assert_eq!(store.fire_due_timers(due).unwrap().len(), 1);
    assert!(store
        .fire_due_timers(Timestamp::from_second(11).unwrap())
        .unwrap()
        .is_empty());
    let mut recovered = WorkflowCtx::new(&store, "timer-run").unwrap();
    let timer = recovered.schedule_timer(due).unwrap();
    assert!(recovered.poll_timer(&timer).unwrap());
}

#[test]
fn invalid_run_id_is_rejected() {
    let store = Store::open_in_memory().unwrap();
    assert!(matches!(
        WorkflowCtx::new(&store, "x:timers"),
        Err(WorkflowError::InvalidRunId(_))
    ));
}

#[test]
fn receipt_reconciles_after_crash_before_completion() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("receipt.db");
    let root = dir.path().join("artifacts");
    let secret = ModelOutput {
        body: "receipt-only".into(),
    };
    {
        let store = Store::open(&db).unwrap();
        let artifacts = ArtifactStore::open(root.clone(), [3u8; 32]).unwrap();
        let mut workflow = WorkflowCtx::new(&store, "receipt").unwrap();
        let handle = match workflow
            .begin_private_step::<ModelOutput>("workflow.model_call", &"prompt", &artifacts)
            .unwrap()
        {
            StepState::Fresh { handle, .. } => handle,
            _ => panic!(),
        };
        let receipt = artifacts
            .put(&serde_json::to_vec(&secret).unwrap())
            .unwrap();
        workflow.record_receipt(&handle, receipt).unwrap();
    }
    let store = Store::open(&db).unwrap();
    let artifacts = ArtifactStore::open(root, [3u8; 32]).unwrap();
    let mut recovered = WorkflowCtx::new(&store, "receipt").unwrap();
    assert!(
        matches!(recovered.begin_private_step::<ModelOutput>("workflow.model_call", &"prompt", &artifacts).unwrap(), StepState::Replayed { outcome: Ok(value), .. } if value == secret)
    );
}

#[test]
fn divergent_inputs_fail_closed() {
    let store = Store::open_in_memory().unwrap();
    let mut workflow = WorkflowCtx::new(&store, "div").unwrap();
    complete_u32(&mut workflow, "workflow.connector_call", "a", 7);
    let mut recovered = WorkflowCtx::new(&store, "div").unwrap();
    assert!(matches!(
        recovered.begin_step::<u32>("workflow.connector_call", &"b"),
        Err(WorkflowError::Divergence { .. })
    ));
}

#[test]
fn identical_steps_complete_reverse_order_by_exact_handle() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("kernel.db");
    {
        let store = Store::open(&db).unwrap();
        let mut workflow = WorkflowCtx::new(&store, "same").unwrap();
        let a = match workflow
            .begin_step::<u64>("workflow.same", &"digest")
            .unwrap()
        {
            StepState::Fresh { handle, .. } => handle,
            _ => panic!(),
        };
        let b = match workflow
            .begin_step::<u64>("workflow.same", &"digest")
            .unwrap()
        {
            StepState::Fresh { handle, .. } => handle,
            _ => panic!(),
        };
        assert_ne!(a.step_id, b.step_id);
        workflow.complete_step(&b, Ok::<_, String>(22u64)).unwrap();
        workflow.complete_step(&a, Ok::<_, String>(11u64)).unwrap();
    }
    let store = Store::open(&db).unwrap();
    let mut recovered = WorkflowCtx::new(&store, "same").unwrap();
    assert!(matches!(
        recovered
            .begin_step::<u64>("workflow.same", &"digest")
            .unwrap(),
        StepState::Replayed {
            outcome: Ok(11),
            ..
        }
    ));
    assert!(matches!(
        recovered
            .begin_step::<u64>("workflow.same", &"digest")
            .unwrap(),
        StepState::Replayed {
            outcome: Ok(22),
            ..
        }
    ));
    assert!(store.verify_audit_chain().unwrap());
}

#[test]
fn timer_and_deterministic_reads_replay() {
    let store = Store::open_in_memory().unwrap();
    let due = Timestamp::from_second(20).unwrap();
    let mut first = WorkflowCtx::new_with_definition(&store, "defined", "mail", "7").unwrap();
    let timer = first.schedule_timer(due).unwrap();
    assert!(!first.poll_timer(&timer).unwrap());
    let recorded_now = first.now().unwrap();
    let mut recovered = WorkflowCtx::new_with_definition(&store, "defined", "mail", "7").unwrap();
    assert_eq!(recovered.schedule_timer(due).unwrap(), timer);
    assert_eq!(recovered.now().unwrap(), recorded_now);
    let mut wrong = WorkflowCtx::new_with_definition(&store, "defined", "mail", "8").unwrap();
    assert!(matches!(
        wrong.schedule_timer(due),
        Err(WorkflowError::Divergence { .. })
    ));
}

#[test]
fn ledger_corruption_fails_closed() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("kernel.db");
    let store = Store::open(&db).unwrap();
    let mut workflow = WorkflowCtx::new(&store, "corrupt").unwrap();
    complete_u32(&mut workflow, "workflow.connector_call", "x", 1);
    drop(store);
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute("UPDATE audit_log SET hash = 'sha256:deadbeef' WHERE seq = (SELECT MIN(seq) FROM audit_log)", []).unwrap();
    drop(conn);
    let store = Store::open(&db).unwrap();
    assert!(matches!(
        WorkflowCtx::new(&store, "corrupt"),
        Err(WorkflowError::LedgerCorrupted)
    ));
}

#[test]
fn timer_schedule_converges_across_contexts() {
    let store = Store::open_in_memory().unwrap();
    let due = Timestamp::from_second(10).unwrap();
    let mut workflow_a = WorkflowCtx::new(&store, "timer-converge").unwrap();
    let timer_a = workflow_a.schedule_timer(due).unwrap();
    let mut workflow_b = WorkflowCtx::new(&store, "timer-converge").unwrap();
    let timer_b = workflow_b.schedule_timer(due).unwrap();
    assert_eq!(timer_a, timer_b);
    assert!(!workflow_b.poll_timer(&timer_b).unwrap());
    assert!(store
        .fire_due_timers(Timestamp::from_second(9).unwrap())
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .fire_due_timers(Timestamp::from_second(11).unwrap())
            .unwrap()
            .len(),
        1
    );
    assert!(workflow_b.poll_timer(&timer_b).unwrap());
    assert!(workflow_b.poll_timer(&timer_b).unwrap());
    assert_eq!(
        store
            .count_audit_events_of_kind("workflow.timer_fired")
            .unwrap(),
        1
    );
}

#[test]
fn gated_step_failure_persists_closed_code_and_replays() {
    let store = Store::open_in_memory().unwrap();
    let code = "workflow.gated_step_failed";
    {
        let mut workflow = WorkflowCtx::new(&store, "gated-failure").unwrap();
        let handle = match workflow
            .begin_step::<u32>("workflow.connector_call", &"draft")
            .unwrap()
        {
            StepState::Fresh { handle, .. } => handle,
            _ => panic!("first gated step must be fresh"),
        };
        workflow
            .complete_step(&handle, Err::<u32, _>(code.to_string()))
            .unwrap();
    }
    let mut recovered = WorkflowCtx::new(&store, "gated-failure").unwrap();
    assert!(matches!(
        recovered
            .begin_step::<u32>("workflow.connector_call", &"draft")
            .unwrap(),
        StepState::Replayed {
            outcome: Err(message),
            ..
        } if message == code
    ));
}

#[test]
fn kill_during_pending_yields_resuming() {
    let store = Store::open_in_memory().unwrap();
    {
        let mut workflow = WorkflowCtx::new(&store, "pending-recovery").unwrap();
        assert!(matches!(
            workflow
                .begin_step::<u32>("workflow.connector_call", &"thread")
                .unwrap(),
            StepState::Fresh {
                idempotency_key,
                ..
            } if !idempotency_key.is_empty()
        ));
    }
    let mut recovered = WorkflowCtx::new(&store, "pending-recovery").unwrap();
    assert!(matches!(
        recovered
            .begin_step::<u32>("workflow.connector_call", &"thread")
            .unwrap(),
        StepState::Resuming {
            idempotency_key,
            ..
        } if !idempotency_key.is_empty()
    ));
}

#[test]
fn recover_from_resuming_does_not_redispatch() {
    let store = Store::open_in_memory().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    {
        let mut workflow = WorkflowCtx::new(&store, "completed-effect").unwrap();
        let handle = match workflow
            .begin_step::<u32>("workflow.connector_call", &"once")
            .unwrap()
        {
            StepState::Fresh { handle, .. } => handle,
            _ => panic!("first effect attempt must be fresh"),
        };
        calls.fetch_add(1, Ordering::SeqCst);
        workflow
            .complete_step(&handle, Ok::<_, String>(7u32))
            .unwrap();
    }
    {
        let mut recovered = WorkflowCtx::new(&store, "completed-effect").unwrap();
        assert!(matches!(
            recovered
                .begin_step::<u32>("workflow.connector_call", &"once")
                .unwrap(),
            StepState::Replayed { outcome: Ok(7), .. }
        ));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    {
        let mut workflow = WorkflowCtx::new(&store, "pending-effect").unwrap();
        match workflow
            .begin_step::<u32>("workflow.connector_call", &"once")
            .unwrap()
        {
            StepState::Fresh { .. } => {
                calls.fetch_add(1, Ordering::SeqCst);
            }
            _ => panic!("second effect attempt must be fresh"),
        }
    }
    let handle = {
        let mut recovered = WorkflowCtx::new(&store, "pending-effect").unwrap();
        match recovered
            .begin_step::<u32>("workflow.connector_call", &"once")
            .unwrap()
        {
            StepState::Resuming { handle, .. } => handle,
            _ => panic!("crashed effect must resume"),
        }
    };
    let mut recovered = WorkflowCtx::new(&store, "pending-effect").unwrap();
    recovered
        .complete_step(&handle, Ok::<_, String>(9u32))
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let mut replayed = WorkflowCtx::new(&store, "pending-effect").unwrap();
    assert!(matches!(
        replayed
            .begin_step::<u32>("workflow.connector_call", &"once")
            .unwrap(),
        StepState::Replayed { outcome: Ok(9), .. }
    ));
}

#[test]
fn timer_fires_once_across_recovery_trusted_now() {
    let store = Store::open_in_memory().unwrap();
    let due = Timestamp::from_second(10).unwrap();
    let timer = {
        let mut workflow = WorkflowCtx::new(&store, "timer-recovery").unwrap();
        let timer = workflow.schedule_timer(due).unwrap();
        assert!(!workflow.poll_timer(&timer).unwrap());
        assert_eq!(
            store
                .count_audit_events_of_kind("workflow.timer_fired")
                .unwrap(),
            0
        );
        timer
    };
    let mut recovered = WorkflowCtx::new(&store, "timer-recovery").unwrap();
    assert_eq!(recovered.schedule_timer(due).unwrap(), timer);
    assert!(!recovered.poll_timer(&timer).unwrap());
    assert!(store
        .fire_due_timers(Timestamp::from_second(9).unwrap())
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .fire_due_timers(Timestamp::from_second(11).unwrap())
            .unwrap()
            .len(),
        1
    );
    assert!(recovered.poll_timer(&timer).unwrap());
    assert!(recovered.poll_timer(&timer).unwrap());
    assert_eq!(
        store
            .count_audit_events_of_kind("workflow.timer_fired")
            .unwrap(),
        1
    );
}

#[test]
fn approval_adapter_requires_digests_and_generic_rejects_kind() {
    let store = Store::open_in_memory().unwrap();
    let target_digest =
        openspine_schemas::digest::Digest::parse(format!("sha256:{}", "0".repeat(64))).unwrap();
    let payload_digest =
        openspine_schemas::digest::Digest::parse(format!("sha256:{}", "1".repeat(64))).unwrap();
    let mut workflow = WorkflowCtx::new(&store, "approval").unwrap();
    assert!(matches!(
        workflow.begin_approval_step::<()>("email.create_draft", target_digest, payload_digest),
        Ok(StepState::Fresh { .. }) | Ok(StepState::Replayed { .. })
    ));
    assert!(matches!(
        workflow.begin_step::<()>("workflow.approval", &()),
        Err(WorkflowError::Step(_))
    ));
}

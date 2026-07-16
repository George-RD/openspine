//! Driver / sequence characterization (Wave 2 refactor).
//!
//! These pins guard the executable stage plan itself: the `PipelineStage`
//! enum is the single source of truth for ordering, the driver's synchronous
//! prefix stops before `Gate`, and BOTH lanes execute that exact prefix on the
//! happy path. They do not modify any Wave-1 pin above.

use super::gmail_state_with_real_thread;
use crate::pipeline::driver::{
    email_preview_lane, owner_control_lane, run_pipeline, EventInputs, PipelineStage,
};
use crate::test_support::fixtures::test_state;
use jiff::Timestamp;

#[test]
fn pipeline_stage_sequence_is_declared_once_and_pinned() {
    // The complete sequence is declared in exactly one place with nine stages
    // in canonical order; the driver iterates only its synchronous prefix.
    assert_eq!(PipelineStage::SEQUENCE.len(), 9);
    assert_eq!(
        PipelineStage::SEQUENCE,
        [
            PipelineStage::Event,
            PipelineStage::Verify,
            PipelineStage::Identify,
            PipelineStage::Route,
            PipelineStage::Compose,
            PipelineStage::Grant,
            PipelineStage::Run,
            PipelineStage::Gate,
            PipelineStage::Audit,
        ]
    );
    assert_eq!(PipelineStage::SYNC_PREFIX.len(), 7);
    assert_eq!(PipelineStage::SYNC_PREFIX[6], PipelineStage::Run);
}

#[test]
fn driver_sync_prefix_excludes_gate_and_audit_stages() {
    // Structural guard: gate is a distributed runtime stage, not part of this
    // driver's synchronous prefix (see driver.rs module doc — it must never
    // import or call `gate()`). `SYNC_PREFIX` therefore stops at `Run`.
    assert!(!PipelineStage::SYNC_PREFIX.contains(&PipelineStage::Gate));
    assert!(!PipelineStage::SYNC_PREFIX.contains(&PipelineStage::Audit));
    assert_eq!(PipelineStage::SYNC_PREFIX.last(), Some(&PipelineStage::Run));
}

#[tokio::test]
async fn owner_lane_executed_stage_trace_matches_sync_prefix() {
    let state = test_state();
    let inputs = EventInputs {
        chat_id: 555,
        text: "hello lyra".to_string(),
        thread_id: None,
        owner_verified: Some(crate::telegram::VerifiedOwnerContext::test_new()),
    };
    let mut trace = Vec::new();
    let result = run_pipeline(
        &state,
        owner_control_lane(),
        &inputs,
        Timestamp::now(),
        &mut trace,
    )
    .await
    .unwrap();
    assert!(result.is_some(), "owner-control lane must compose a grant");
    let grant = result.unwrap();
    assert_eq!(
        grant.user,
        state.owner_principal_id.to_string(),
        "composition must consume principal_id, not the Telegram owner config string"
    );
    assert_ne!(
        grant.user,
        state.owner_user_id.to_string(),
        "grant.user must not be the raw Telegram owner user id"
    );
    assert_eq!(trace, PipelineStage::SYNC_PREFIX.to_vec());
}

#[tokio::test]
async fn email_lane_executed_stage_trace_matches_sync_prefix() {
    let (state, _token_server, _api_server) = gmail_state_with_real_thread().await;
    let inputs = EventInputs {
        chat_id: 555,
        text: "/draft thread-1".to_string(),
        thread_id: Some("thread-1".to_string()),
        owner_verified: Some(crate::telegram::VerifiedOwnerContext::test_new()),
    };
    let mut trace = Vec::new();
    let result = run_pipeline(
        &state,
        email_preview_lane(),
        &inputs,
        Timestamp::now(),
        &mut trace,
    )
    .await
    .unwrap();
    assert!(result.is_some(), "email-preview lane must compose a grant");
    assert_eq!(trace, PipelineStage::SYNC_PREFIX.to_vec());
}

#[tokio::test]
async fn owner_lane_without_verified_context_fails_closed_before_grant() {
    // Absent VerifiedOwnerContext, the resolver never yields a principal_id
    // (and never claims Owner relationship). The pipeline fails closed with
    // no grant — either via route denial (Ok(None)) or composition error.
    // The load-bearing contract is: no grant is persisted (AD-146).
    let state = test_state();
    let inputs = EventInputs {
        chat_id: 555,
        text: "hello lyra".to_string(),
        thread_id: None,
        owner_verified: None,
    };
    let mut trace = Vec::new();
    let result = run_pipeline(
        &state,
        owner_control_lane(),
        &inputs,
        Timestamp::now(),
        &mut trace,
    )
    .await
    .expect("pipeline infra must not error; fail-closed is a denial, not an I/O fault");
    assert!(
        result.is_none(),
        "no principal_id / no Owner relationship must yield no grant, got {result:?}"
    );
    assert_eq!(
        state.store.count_task_grants().unwrap(),
        0,
        "no grant may be persisted when identity has no principal"
    );
}

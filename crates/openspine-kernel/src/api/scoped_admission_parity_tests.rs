//! #244 parity: a failed `email.create_draft` must produce the SAME durable
//! record shape whether it flows through the scope-matched standing-rule path
//! (`mediate_and_dispatch_action` -> `dispatch_scoped_effect`) or the
//! interactive-approval path (which calls the shared executor directly and
//! discards the outcome, `pipeline/post_approval.rs:80`). Before #244 the
//! scoped path double-recorded: the executor self-audited/self-batched, and the
//! mediation error handler then re-appended `action.dispatch_failed` and
//! re-called `batch_failure`. Both paths must now record exactly once.
use jiff::Timestamp;
use openspine_schemas::action::{ActionId, ActionRequest};
use openspine_schemas::digest::digest_of;
use openspine_schemas::event::{TargetRef, TargetRefKind};
use serde_json::json;
use ulid::Ulid;

use super::dispatched_from_executor;
use super::scoped_admission_support::*;
use crate::api::actions::{mediate_and_dispatch_action, DispatchError, FailureSurface};
use crate::api::effect_executors::{EffectDisposition, RecordedEffectError};

/// One failure's durable footprint: the two audit vocabularies that could
/// record it and the owner-digest row count.
#[derive(Debug, PartialEq, Eq)]
struct RecordShape {
    creation_failed: i64,
    dispatch_failed: i64,
    digest_rows: usize,
}

/// Drive a scoped `ConfirmedFailure` through the production mediation path
/// against an HTTP 400 draft response and read back its record footprint.
async fn scoped_confirmed_failure_shape() -> RecordShape {
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(
        &env.api_server,
        400,
        json!({"error": {"code": 400, "message": "invalid draft"}}),
    )
    .await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &grant).await;
    env.state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-parity", &context),
            None,
            Timestamp::now(),
        )
        .unwrap();

    let result = mediate_and_dispatch_action(
        &env.state,
        &grant,
        ActionId::new("email.create_draft"),
        &crate::test_support::telegram_surface(CHAT_ID),
        Some(&draft_payload()),
        FailureSurface::DirectResponse,
        None,
    )
    .await;
    assert!(result.is_err(), "a 400 draft write is a confirmed failure");

    RecordShape {
        creation_failed: audit_count(&env.state, "draft.creation_failed"),
        dispatch_failed: audit_count(&env.state, "action.dispatch_failed"),
        digest_rows: digest_count(&env.state),
    }
}

/// Drive the same `ConfirmedFailure` through the interactive-approval path,
/// which invokes the shared executor directly and discards its outcome exactly
/// as `handle_create_approved_draft` does, then read back its footprint.
async fn interactive_confirmed_failure_shape() -> RecordShape {
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(
        &env.api_server,
        400,
        json!({"error": {"code": 400, "message": "invalid draft"}}),
    )
    .await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    // The executor recomputes the target digest from the thread's newest
    // non-owner recipient (`alice@example.com`, per `draft_env`); binding the
    // request to that exact recipient reaches the provider write instead of a
    // pre-effect target-mutation refusal.
    let payload_ref = env
        .state
        .artifacts
        .put(serde_json::to_vec(&draft_payload()).unwrap().as_slice())
        .unwrap();
    let target_digest = digest_of(&json!({
        "thread_id": "thread-1",
        "connector": "gmail_primary",
        "account_role": "owner_mailbox",
        "recipients": ["alice@example.com"],
    }));
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: ActionId::new("email.create_draft"),
        target_ref: Some(TargetRef {
            kind: TargetRefKind::EmailThread,
            id: Some("thread-1".to_string()),
        }),
        payload_ref: Some(payload_ref),
        target_digest: Some(target_digest),
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        skill_attribution: None,
        requested_at: Timestamp::now(),
        schema_version: 1,
    };

    let outcome = crate::pipeline::gmail_create_draft_executor(
        &env.state,
        &grant,
        &request,
        &crate::test_support::owner_surface_for(&env.state, CHAT_ID),
    )
    .await
    .unwrap();
    assert_eq!(
        outcome,
        EffectDisposition::ConfirmedFailure,
        "a 400 draft write is a confirmed failure on the interactive path too"
    );

    RecordShape {
        creation_failed: audit_count(&env.state, "draft.creation_failed"),
        dispatch_failed: audit_count(&env.state, "action.dispatch_failed"),
        digest_rows: digest_count(&env.state),
    }
}

/// Acceptance (#244): one failed scoped draft and one failed interactive draft
/// produce the identical single-record footprint — one `draft.creation_failed`
/// audit row, zero `action.dispatch_failed` rows, and one owner-digest row.
#[tokio::test]
async fn scoped_and_interactive_confirmed_failure_record_shape_is_identical() {
    let scoped = scoped_confirmed_failure_shape().await;
    let interactive = interactive_confirmed_failure_shape().await;

    let expected = RecordShape {
        creation_failed: 1,
        dispatch_failed: 0,
        digest_rows: 1,
    };
    assert_eq!(
        scoped, expected,
        "the scoped path records the failure exactly once"
    );
    assert_eq!(
        interactive, expected,
        "the interactive path records the failure exactly once"
    );
    assert_eq!(
        scoped, interactive,
        "both admission paths must produce the same record shape"
    );
}

/// #244 provenance mapping (dispatcher half): a [`RecordedEffectError`] — the
/// executor already committed its typed record and only a follow-up fail-closed
/// durable write failed — must map to `executor_recorded: true` carrying the
/// settled disposition, so the mediation handler skips the duplicate audit/batch
/// while the failure stays surfaced for redrive.
#[test]
fn recorded_effect_error_maps_to_executor_owned_surfaced_failure() {
    let dispatched = dispatched_from_executor(Err(RecordedEffectError {
        disposition: EffectDisposition::ConfirmedFailure,
        source: anyhow::anyhow!("owner-digest store write failed after settle"),
    }
    .into()));
    assert_eq!(dispatched.disposition, EffectDisposition::ConfirmedFailure);
    assert!(
        dispatched.executor_recorded,
        "the executor already owns the record; mediation must not duplicate it"
    );
    assert!(
        matches!(dispatched.result, Err(DispatchError::Resource(_))),
        "the fail-closed digest error stays surfaced for redrive"
    );
}

/// A plain executor error (bailed BEFORE recording anything) must stay
/// un-recorded so the mediation handler records the failure exactly once, and
/// fail closed as `DeliveryUnknown` because the effect ordering is unknown.
#[test]
fn plain_executor_error_stays_unrecorded_and_fails_closed() {
    let dispatched = dispatched_from_executor(Err(anyhow::anyhow!(
        "artifact read failed before any audit"
    )));
    assert_eq!(dispatched.disposition, EffectDisposition::DeliveryUnknown);
    assert!(
        !dispatched.executor_recorded,
        "nothing was recorded; the mediation handler must record it once"
    );
}

/// #244 fail-closed provenance (executor half): when the executor's fail-closed
/// owner-digest write fails AFTER it already settled the effect and appended
/// `draft.creation_failed`, `create_approved_draft` surfaces a
/// [`RecordedEffectError`] — not a bare `Err` (which would make the dispatcher
/// re-record) and not a swallowed success (which would drop the digest with no
/// redrive). The primary failure audit exists exactly once and no digest row is
/// falsely claimed. Uses the artifact store's existing `set_fault_put_for_test`
/// seam to fail the digest write's encrypt round-trip.
#[tokio::test]
async fn digest_write_failure_after_settle_surfaces_recorded_effect_error() {
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(
        &env.api_server,
        400,
        json!({"error": {"code": 400, "message": "invalid draft"}}),
    )
    .await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    // Recipient matches `draft_env`'s newest non-owner participant so the
    // executor's re-derivation reaches the provider write (a 400 confirmed
    // failure) instead of a pre-effect refusal.
    let payload_ref = env
        .state
        .artifacts
        .put(serde_json::to_vec(&draft_payload()).unwrap().as_slice())
        .unwrap();
    let target_digest = digest_of(&json!({
        "thread_id": "thread-1",
        "connector": "gmail_primary",
        "account_role": "owner_mailbox",
        "recipients": ["alice@example.com"],
    }));
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: ActionId::new("email.create_draft"),
        target_ref: Some(TargetRef {
            kind: TargetRefKind::EmailThread,
            id: Some("thread-1".to_string()),
        }),
        payload_ref: Some(payload_ref),
        target_digest: Some(target_digest),
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        skill_attribution: None,
        requested_at: Timestamp::now(),
        schema_version: 1,
    };

    // Fail the owner-digest write's encrypt round-trip (a SYSTEM_SCOPE `put`).
    // The payload blob above is already stored, and the confirmed-failure path
    // performs no other SYSTEM_SCOPE `put` before `batch_failure`.
    env.state.artifacts.set_fault_put_for_test(true);

    let outcome = crate::pipeline::gmail_create_draft_executor(
        &env.state,
        &grant,
        &request,
        &crate::test_support::owner_surface_for(&env.state, CHAT_ID),
    )
    .await;

    let err = outcome.expect_err("a fail-closed digest write must surface an error");
    let recorded = err
        .downcast_ref::<RecordedEffectError>()
        .expect("the error must carry record-once provenance, not a bare store error");
    assert_eq!(
        recorded.disposition,
        EffectDisposition::ConfirmedFailure,
        "the settled disposition travels with the error"
    );
    assert_eq!(
        audit_count(&env.state, "draft.creation_failed"),
        1,
        "the primary failure audit was committed before the digest write failed"
    );
    assert_eq!(
        digest_count(&env.state),
        0,
        "the fail-closed digest write left no row"
    );
}

//! Per-lane hook implementations invoked by [`super::driver::run_pipeline`]
//! through the [`super::driver::LaneSpec`] fn-pointer record.
//!
//! Each function is a single-stage adapter: it does exactly the work the
//! corresponding stage needs for one lane and nothing else. No hook calls
//! another hook, and none call `resolve_route` / `compose_authority` /
//! `insert_task_grant` / `run_task` directly — the driver owns stage
//! dispatch, early returns, `event.received` emission (only after Verify
//! succeeds), grant persistence, and the shell run.

use std::future::Future;
use std::pin::Pin;

use jiff::Timestamp;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::event::{
    AccountRole, ActorHint, ChannelTrust, Connector, DataClassification, EventEnvelope, EventType,
    InteractionMode, Lane, Source, TargetRef, TargetRefKind, TrustContext, VerificationMethod,
};
use openspine_schemas::grant::TaskGrant;
use ulid::Ulid;

use super::driver::{EventInputs, PreflightFailure};
use super::selection::{build_selection_token, format_pending_message};
use super::AppState;
use crate::sandbox;
use crate::telegram;

// ── Owner-control lane hooks ───────────────────────────────────────────────

pub(super) fn owner_build_envelope(
    state: &AppState,
    inputs: &EventInputs,
    now: Timestamp,
) -> anyhow::Result<(EventEnvelope, ArtifactRef)> {
    let raw_ref = state.artifacts.put(inputs.text.as_bytes())?;
    let envelope = telegram::build_owner_envelope(inputs.chat_id, raw_ref.clone(), now);
    Ok((envelope, raw_ref))
}

pub(super) fn owner_preflight<'a>(
    _state: &'a AppState,
    _inputs: &'a EventInputs,
    _lane: Lane,
    _now: Timestamp,
) -> Pin<Box<dyn Future<Output = Result<(), PreflightFailure>> + Send + 'a>> {
    Box::pin(async move { Ok(()) })
}

pub(super) fn owner_route_guard(
    state: &AppState,
    envelope: &EventEnvelope,
    _lane: Lane,
) -> anyhow::Result<bool> {
    if sandbox::refuses_external_communication_without_containment(
        envelope.lane,
        &state.sandbox,
        state.unsafe_allow_uncontained_private_data,
    ) {
        state.store.append_audit(
            "route.refused_uncontained",
            None,
            None,
            Some("external_communication lane requires a containing sandbox driver"),
            None,
            &[],
            &[],
        )?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub(super) fn owner_grant_binding(
    _state: &AppState,
    _grant: &mut TaskGrant,
    _inputs: &EventInputs,
    raw_ref: &ArtifactRef,
    _now: Timestamp,
) -> anyhow::Result<ArtifactRef> {
    // The owner-control lane never derives a synthetic pending message: the
    // original message ref is the pending task input.
    Ok(raw_ref.clone())
}

// ── Email-preview (selected-thread) lane hooks ─────────────────────────────

pub(super) fn email_build_envelope(
    state: &AppState,
    inputs: &EventInputs,
    now: Timestamp,
) -> anyhow::Result<(EventEnvelope, ArtifactRef)> {
    let thread_id = inputs
        .thread_id
        .as_deref()
        .expect("email-preview lane always carries a thread id");
    let raw_ref = state.artifacts.put(thread_id.as_bytes())?;
    let user = state.owner_user_id.to_string();
    let envelope = EventEnvelope {
        id: Ulid::new(),
        source: Source::Gmail,
        connector: Some(Connector::GmailPrimaryConnector),
        account_role: Some(AccountRole::OwnerMailbox),
        event_type: EventType::EmailThreadSelected,
        received_at: now,
        verified_source: true,
        verification_method: VerificationMethod::KernelUiSelection,
        replay_protected: false,
        replay_nonce: None,
        channel_account: user,
        raw_event_ref: raw_ref.clone(),
        actor_hint: ActorHint::default(),
        target_refs: vec![TargetRef {
            kind: TargetRefKind::EmailThread,
            id: Some(thread_id.to_string()),
        }],
        data_classification: DataClassification::Private,
        user_intent_hint: Some("draft_reply_for_selected_email_thread".to_string()),
        lane: Lane::ExternalCommunication,
        trust_context: TrustContext {
            channel_trust: ChannelTrust::OwnerDevice,
            interaction_mode: InteractionMode::UserSelected,
        },
        thread_id: None,
        schema_version: 1,
    };
    Ok((envelope, raw_ref))
}

pub(super) fn email_preflight<'a>(
    state: &'a AppState,
    inputs: &'a EventInputs,
    lane: Lane,
    _now: Timestamp,
) -> Pin<Box<dyn Future<Output = Result<(), PreflightFailure>> + Send + 'a>> {
    Box::pin(async move {
        let Some(gmail) = state.connectors.gmail() else {
            return Err(PreflightFailure::GmailNotConfigured);
        };
        // D-025 / O-003 / PRD §16: refuse before ever contacting Gmail or
        // minting a token — this lane is statically `ExternalCommunication`,
        // so the guard needs no envelope to evaluate. It runs BEFORE
        // `thread_exists` so a refused request never burns a live Gmail API
        // call or leaves an orphaned selection token.
        if sandbox::refuses_external_communication_without_containment(
            lane,
            &state.sandbox,
            state.unsafe_allow_uncontained_private_data,
        ) {
            return Err(PreflightFailure::RefusedUncontained);
        }
        let thread_id = inputs
            .thread_id
            .as_deref()
            .expect("email-preview lane always carries a thread id");
        let result = gmail.thread_exists(thread_id).await;
        crate::failure_surfacing::record_connector_outcome(&state.store, "gmail", result.is_ok())
            .map_err(|err| PreflightFailure::GmailError {
            err: format!("recording Gmail outcome failed: {err}"),
        })?;
        match result {
            Ok(true) => Ok(()),
            Ok(false) => Err(PreflightFailure::ThreadNotFound {
                thread_id: thread_id.to_string(),
            }),
            Err(err) => Err(PreflightFailure::GmailError {
                err: err.to_string(),
            }),
        }
    })
}

pub(super) fn email_route_guard(
    _state: &AppState,
    _envelope: &EventEnvelope,
    _lane: Lane,
) -> anyhow::Result<bool> {
    // The email-preview lane already ran the containment guard in its
    // Verify-stage preflight (before the Gmail `thread_exists` call), so the
    // Route stage needs no second check here — a refused request exited there.
    Ok(false)
}

pub(super) fn email_grant_binding(
    state: &AppState,
    grant: &mut TaskGrant,
    inputs: &EventInputs,
    _raw_ref: &ArtifactRef,
    now: Timestamp,
) -> anyhow::Result<ArtifactRef> {
    let thread_id = inputs
        .thread_id
        .as_deref()
        .expect("email-preview lane always carries a thread id");
    let token = build_selection_token(state, thread_id, now);
    state.store.insert_selection_token(&token)?;
    // PRD §12: this grant may use exactly the one token minted for it.
    grant.selection_tokens = vec![token.id];
    let pending_ref = state
        .artifacts
        .put(format_pending_message(thread_id, token.id).as_bytes())?;
    Ok(pending_ref)
}

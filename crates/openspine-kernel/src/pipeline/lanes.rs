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

/// The parsed, lane-agnostic intake the driver consumes. Lane selection has
/// already happened (the `/draft <id>` command detected) by the time this
/// reaches the driver; `thread_id` is `Some` only for the email-preview lane.
#[derive(Debug, Clone)]
pub struct EventInputs {
    pub chat_id: i64,
    pub text: String,
    pub thread_id: Option<String>,
    pub owner_verified: Option<crate::telegram::VerifiedOwnerContext>,
    pub principal_override: Option<Ulid>,
    pub event_type_override: Option<EventType>,
    #[allow(dead_code)]
    pub timer_event_id: Option<String>,
    pub correlated_task_id: Option<Ulid>,
    pub dispatch_key: Option<String>,
    pub dispatch_timer_id: Option<String>,
}

/// The request-local snapshot a preflight stage may produce and the driver
/// threads into the later (Grant→Run) stages. It carries only data the
/// pre-gate Verify stage is authorized to compute — currently the email
/// counterparty address derived from the Gmail thread the owner explicitly
/// selected — so no ungated connector read reaches `pack_for_pipeline`.
#[derive(Debug, Clone, Default)]
pub struct PreflightSnapshot {
    pub counterparty_address: Option<String>,
}

/// A preflight verification failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreflightFailure {
    GmailNotConfigured,
    RefusedUncontained,
    ThreadNotFound {
        thread_id: String,
    },
    CounterpartyUnavailable {
        thread_id: String,
    },
    GmailError {
        status: Option<u16>,
        class: crate::gmail::GmailFailureClass,
    },
}

/// Async preflight adapter hook. Returns a [`PreflightSnapshot`] the driver
/// forwards into packing; a `PreflightFailure` aborts the pipeline before
/// any grant is composed.
pub type PreflightFn = for<'a> fn(
    &'a AppState,
    &'a EventInputs,
    Lane,
    Timestamp,
) -> Pin<
    Box<dyn Future<Output = Result<PreflightSnapshot, PreflightFailure>> + Send + 'a>,
>;

/// Builds the event envelope.
pub type BuildEnvelopeFn =
    fn(&AppState, &EventInputs, Timestamp) -> anyhow::Result<(EventEnvelope, ArtifactRef)>;

/// Lane-driven containment guard, invoked in the `Route` stage. Returns
/// `Ok(true)` when the lane is refused (audit already emitted) so the driver
/// can exit; `Ok(false)` means the lane may proceed.
pub type RouteGuardFn = fn(&AppState, &EventEnvelope, Lane) -> anyhow::Result<bool>;

/// Grant-binding adapter: mints/binds any lane-specific selection token and
/// returns the pending task input ref persisted with the grant.
pub type GrantBindingFn = fn(
    &AppState,
    &mut TaskGrant,
    &EventInputs,
    &ArtifactRef,
    Timestamp,
) -> anyhow::Result<ArtifactRef>;

/// Compiled-in data record capturing everything that diverges between the
/// two event flows. A lane carries no sequencing capability — the driver
/// owns the order via [`PipelineStage::SYNC_PREFIX`]; per-lane "absence of
/// stage work" (e.g. the owner-control lane's no-op preflight) is expressed
/// as a no-op input to that stage, never a skipped stage.
#[derive(Clone, Copy)]
pub struct LaneSpec {
    pub lane: Lane,
    pub channel_trust: ChannelTrust,
    pub purpose: &'static str,
    pub build_envelope: BuildEnvelopeFn,
    pub preflight: PreflightFn,
    pub route_containment_guard: RouteGuardFn,
    pub grant_binding: GrantBindingFn,
}

/// The verified-owner Telegram conversation lane.
pub fn owner_control_lane() -> LaneSpec {
    LaneSpec {
        lane: Lane::OwnerControl,
        channel_trust: ChannelTrust::VerifiedOwnerChannel,
        purpose: "owner_control_conversation",
        build_envelope: owner_build_envelope,
        preflight: owner_preflight,
        route_containment_guard: owner_route_guard,
        grant_binding: owner_grant_binding,
    }
}

/// The `/draft <thread_id>` selected-thread email reply draft lane.
pub fn email_preview_lane() -> LaneSpec {
    LaneSpec {
        lane: Lane::ExternalCommunication,
        channel_trust: ChannelTrust::OwnerDevice,
        purpose: "selected_thread_email_reply_draft",
        build_envelope: email_build_envelope,
        preflight: email_preflight,
        route_containment_guard: email_route_guard,
        grant_binding: email_grant_binding,
    }
}

/// The task-board internal scheduled-dispatch lane (D-082..D-084).
pub fn scheduled_internal_lane() -> LaneSpec {
    LaneSpec {
        lane: Lane::ScheduledInternal,
        channel_trust: ChannelTrust::Unknown,
        purpose: "task_board_scheduled",
        build_envelope: scheduled_build_envelope,
        preflight: scheduled_preflight,
        route_containment_guard: scheduled_route_guard,
        grant_binding: scheduled_grant_binding,
    }
}
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
) -> Pin<Box<dyn Future<Output = Result<PreflightSnapshot, PreflightFailure>> + Send + 'a>> {
    Box::pin(async move { Ok(PreflightSnapshot::default()) })
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
        schema_version: 1,
        thread_id: Some(thread_id.to_string()),
    };
    Ok((envelope, raw_ref))
}

pub(super) fn email_preflight<'a>(
    state: &'a AppState,
    inputs: &'a EventInputs,
    lane: Lane,
    _now: Timestamp,
) -> Pin<Box<dyn Future<Output = Result<PreflightSnapshot, PreflightFailure>> + Send + 'a>> {
    Box::pin(async move {
        let Some(gmail) = state.connectors.gmail() else {
            return Err(PreflightFailure::GmailNotConfigured);
        };
        // D-025 / O-003 / PRD §16: refuse before ever contacting Gmail or
        // minting a token — this lane is statically `ExternalCommunication`,
        // so the guard needs no envelope to evaluate. It runs BEFORE the
        // Gmail metadata read so a refused request never burns a live Gmail
        // API call or leaves an orphaned selection token.
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
        // D-055: a narrow, header-only recipient read (`format=metadata`,
        // `metadataHeaders=From`) — NOT `fetch_thread`'s body read, which is
        // gated by `email.read_thread:selected_no_attachments` only after the
        // grant. This is the catalogued `resolve_email_counterparty` effect
        // path, classified `PreGateOwnerSelectedRead`: the read is authorized
        // by the verified-owner `/draft` selection and the containment guard,
        // not by the authority `gate()`.
        let recipient = match gmail.fetch_thread_recipient(thread_id).await {
            Ok(crate::gmail::ThreadRecipient::Address(address)) => address,
            Ok(crate::gmail::ThreadRecipient::ThreadNotFound) => {
                return Err(PreflightFailure::ThreadNotFound {
                    thread_id: thread_id.to_string(),
                })
            }
            Ok(crate::gmail::ThreadRecipient::Unavailable) => {
                return Err(PreflightFailure::CounterpartyUnavailable {
                    thread_id: thread_id.to_string(),
                })
            }
            Err(err) => {
                return Err(PreflightFailure::GmailError {
                    status: err.status,
                    class: err.class,
                })
            }
        };
        // The recipient resolution is an enumerated effect; record it without
        // carrying the plaintext address into the audit (D-012).
        state
            .store
            .append_audit(
                "email.counterparty.resolved",
                None,
                None,
                None,
                None,
                &[],
                &[],
            )
            .map_err(|_| PreflightFailure::GmailError {
                status: None,
                class: crate::gmail::GmailFailureClass::Transport,
            })?;
        Ok(PreflightSnapshot {
            counterparty_address: Some(recipient),
        })
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
    let pending_ref = state
        .artifacts
        .put(format_pending_message(thread_id, token.id).as_bytes())?;
    state.store.insert_selection_token(&token)?;
    // PRD §12: this grant may use exactly the one token minted for it.
    grant.selection_tokens = vec![token.id];
    Ok(pending_ref)
}
// ── Scheduled task-board lane hooks ────────────────────────────────────────

pub(super) fn scheduled_build_envelope(
    state: &AppState,
    inputs: &EventInputs,
    now: Timestamp,
) -> anyhow::Result<(EventEnvelope, ArtifactRef)> {
    let raw_ref = state.artifacts.put(inputs.text.as_bytes())?;
    let event_type = inputs
        .event_type_override
        .unwrap_or(EventType::TimerDeadlineFired);
    let envelope = EventEnvelope {
        id: Ulid::new(),
        source: Source::Timer,
        connector: None,
        account_role: Some(AccountRole::SystemAccount),
        event_type,
        received_at: now,
        verified_source: false,
        verification_method: VerificationMethod::None,
        replay_protected: false,
        replay_nonce: None,
        channel_account: state.owner_user_id.to_string(),
        raw_event_ref: raw_ref.clone(),
        actor_hint: ActorHint::default(),
        target_refs: vec![],
        data_classification: DataClassification::Internal,
        user_intent_hint: Some(inputs.text.clone()),
        lane: Lane::ScheduledInternal,
        trust_context: TrustContext {
            channel_trust: ChannelTrust::Unknown,
            interaction_mode: InteractionMode::Scheduled,
        },
        thread_id: None,
        schema_version: 1,
    };
    Ok((envelope, raw_ref))
}

pub(super) fn scheduled_preflight<'a>(
    _state: &'a AppState,
    _inputs: &'a EventInputs,
    _lane: Lane,
    _now: Timestamp,
) -> Pin<Box<dyn Future<Output = Result<PreflightSnapshot, PreflightFailure>> + Send + 'a>> {
    Box::pin(async { Ok(PreflightSnapshot::default()) })
}

pub(super) fn scheduled_route_guard(
    _state: &AppState,
    _envelope: &EventEnvelope,
    _lane: Lane,
) -> anyhow::Result<bool> {
    Ok(false)
}

pub(super) fn scheduled_grant_binding(
    state: &AppState,
    _grant: &mut TaskGrant,
    inputs: &EventInputs,
    _raw_ref: &ArtifactRef,
    now: Timestamp,
) -> anyhow::Result<ArtifactRef> {
    const MASTER_SLICE_CAP: usize = 10;
    let task_id = inputs
        .correlated_task_id
        .ok_or_else(|| anyhow::anyhow!("scheduled grant binding missing correlated task id"))?;
    let slice = state
        .store
        .master_slice_for_task(task_id, now, MASTER_SLICE_CAP)?;
    let payload = serde_json::to_vec(&slice)?;
    Ok(state.artifacts.put(&payload)?)
}

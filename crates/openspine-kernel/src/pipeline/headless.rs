//! Headless webhook lane (AD-134).
//!
//! A verified webhook is an ordinary event source: it enters the governed
//! pipeline (verify → identify → route → compose → grant → run → gate →
//! audit) exactly like an owner message, but with **no owner conversation**
//! when the composed authority requires no approval. Its completion is
//! surfaced only via the owner digest.
//!
//! The lane reuses the kernel's existing pieces:
//! - [`crate::connector_reality::WebhookVerifier`] for HMAC + replay-window
//!   verification (the envelope is minted only after this passes),
//! - [`super::driver::run_pipeline_with_envelope`] for the ordinary
//!   `Identify → Route → Compose → Grant → Run` stages,
//! - [`openspine_gate::gate`] for the final approval decision.

#![allow(dead_code)] // exported lane is consumed by the hook ingress boundary
use jiff::Timestamp;
use openspine_schemas::action::ActionRequest;
use openspine_schemas::action::{ActionId, GateDecision};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::digest_of;
use openspine_schemas::event::{
    AccountRole, ActorHint, ChannelTrust, DataClassification, EventEnvelope, EventType,
    InteractionMode, Lane, Source, TrustContext, VerificationMethod,
};
use openspine_schemas::grant::TaskGrant;
use serde_json::json;
use std::future::Future;
use std::pin::Pin;
use ulid::Ulid;

use super::driver::{run_pipeline_with_envelope, PipelineStage};
use super::lanes::{EventInputs, LaneSpec, PreflightFailure, PreflightSnapshot};
use super::AppState;
/// Trusted, caller-independent webhook intake. Only the payload, signature,
/// idempotency key, and signed-at time are authenticatable (covered by the
/// verifier's MAC); the `channel_account` (the hook/route selector) is the
/// single trusted routing key derived from the verified delivery. The
/// `action` is validated against the composed grant before it is gated — a
/// caller can never run an action outside the route's composed authority.
#[derive(Debug, Clone)]
pub struct HeadlessHookRequest {
    pub payload: Vec<u8>,
    pub signature: String,
    pub idempotency_key: String,
    pub signed_at: Timestamp,
    pub channel_account: String,
    pub action: ActionId,
}

/// Outcome of driving one webhook through the headless lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadlessHookOutcome {
    /// Signature/replay verification failed; dropped with audit, no grant.
    Rejected(String),
    /// Verified but the pipeline decided not to compose a grant (route denied,
    /// spend cap, etc.).
    Dropped,
    /// Composed authority required no approval: ran silently, surfaced only in
    /// the owner digest.
    Completed(Ulid),
    /// Composed authority required owner approval (or was denied): escalated
    /// normally to the owner.
    Escalated(Ulid),
}

/// Drive one verified webhook through the full governed pipeline.
///
/// Verification, durable-event minting, and the synchronous pipeline prefix
/// all happen here; the only owner-facing surfacing for a no-approval run is a
/// single owner-digest entry — no Telegram conversation is ever created.
pub async fn run_headless_hook(
    state: &AppState,
    request: HeadlessHookRequest,
    now: Timestamp,
) -> anyhow::Result<HeadlessHookOutcome> {
    // 1. Verify the webhook signature + replay window (AD-134/AD-141). A
    //    failure is a hard drop with audit — rejected/replayed hooks never
    //    enter the pipeline.
    let webhook_envelope = crate::connector_reality::WebhookEnvelope {
        payload: &request.payload,
        signature: &request.signature,
        idempotency_key: &request.idempotency_key,
        signed_at: request.signed_at,
    };
    if let Err(rejection) = state.webhook_verifier.verify_bound(
        webhook_envelope,
        &request.action.0,
        &request.channel_account,
        now,
    ) {
        state.store.append_audit(
            "webhook.rejected",
            None,
            None,
            Some(&rejection.to_string()),
            None,
            &[],
            &[],
        )?;
        return Ok(HeadlessHookOutcome::Rejected(rejection.to_string()));
    }

    // 2. Mint the durable event envelope. The webhook is an ordinary event
    //    source: lane/event_type are trusted constants (never caller input),
    //    and `channel_account` is the authenticated hook id used for routing.
    let raw_ref = state.artifacts.put(&request.payload)?;
    let envelope = EventEnvelope {
        id: Ulid::new(),
        source: Source::Webhook,
        connector: None,
        account_role: Some(AccountRole::SystemAccount),
        event_type: EventType::WebhookReceived,
        received_at: now,
        verified_source: true,
        verification_method: VerificationMethod::WebhookSignature,
        replay_protected: true,
        replay_nonce: Some(request.idempotency_key.clone()),
        channel_account: request.channel_account.clone(),
        raw_event_ref: raw_ref.clone(),
        actor_hint: ActorHint::default(),
        target_refs: vec![],
        data_classification: DataClassification::Internal,
        user_intent_hint: None,
        lane: Lane::BusinessWorkflow,
        trust_context: TrustContext {
            channel_trust: ChannelTrust::VerifiedContact,
            interaction_mode: InteractionMode::SystemHook,
        },
        thread_id: None,
        schema_version: 1,
    };
    state.store.append_audit(
        "webhook.verified",
        None,
        None,
        None,
        None,
        &[],
        std::slice::from_ref(&raw_ref),
    )?;

    // 3. Admit against the global daily spend cap — the same gate `run_pipeline`
    //    applies to every other non-immediate lane.
    if !crate::spend::admit_spend(
        state,
        crate::spend::SpendLane::from_event_lane(Lane::BusinessWorkflow),
        now,
    )
    .await?
    {
        state.store.append_audit(
            "spend.cap_breached",
            None,
            None,
            Some("headless webhook paused by global daily spend cap"),
            None,
            &[],
            &[],
        )?;
        return Ok(HeadlessHookOutcome::Dropped);
    }

    // 4. Drive the ordinary pipeline from the verified envelope (Identify →
    //    Route → Compose → Grant → Run). The headless lane builds the envelope
    //    itself, so it calls `run_pipeline_with_envelope` directly.
    let inputs = EventInputs {
        chat_id: state.owner_user_id,
        text: String::new(),
        thread_id: None,
        owner_verified: None,
        // A verified webhook is a trusted, kernel-bound source: it resolves to
        // the owner principal so the route can compose bounded authority.
        principal_override: Some(state.owner_principal_id),
        event_type_override: None,
        timer_event_id: None,
        correlated_task_id: None,
        dispatch_key: None,
        dispatch_timer_id: None,
    };
    let mut trace = Vec::new();
    trace.extend([PipelineStage::Event, PipelineStage::Verify]);
    let grant = run_pipeline_with_envelope(
        state,
        headless_lane(),
        envelope,
        raw_ref.clone(),
        &inputs,
        now,
        PreflightSnapshot::default(),
        &mut trace,
        false, // spawn_shell: headless lane must never open an owner conversation
    )
    .await?;
    let Some(grant) = grant else {
        return Ok(HeadlessHookOutcome::Dropped);
    };

    // 5. Run the route's composed action through the shared mediation boundary.
    //    This is the real gate + effect path; no-approval actions execute
    //    silently, while approval-required actions retain normal escalation.
    let action = request.action;
    let (decision, _, _, _) = crate::api::actions::mediate_and_dispatch_action_headless(
        state,
        &grant,
        action.clone(),
        state.owner_user_id,
        None,
    )
    .await
    .map_err(|err| anyhow::anyhow!("headless action mediation failed: {err:?}"))?;

    match decision {
        GateDecision::Allow => {
            // Silent completion: no owner conversation — only a digest entry.
            let detail = format!("headless hook {action} completed (grant {})", grant.id);
            let detail_ref = state.artifacts.put(detail.as_bytes())?;
            state.store.record_headless_hook_completion(
                "headless",
                &detail,
                &detail_ref.digest.to_string(),
                Some(grant.id),
            )?;
            Ok(HeadlessHookOutcome::Completed(grant.id))
        }
        other => {
            if state.action_catalog.is_counterparty_facing(&action) {
                // Counterparty-facing actions already route through the
                // normal escalation path in the mediation boundary.
                let message = format!("Headless hook {action} escalated: {other:?}");
                super::notify_owner_required(state, state.owner_user_id, &message).await?;
            } else {
                // Persist a digest-bound request before surfacing the owner
                // button. The post-approval registry recognizes the explicit
                // `headless=true` marker and re-dispatches exactly once.
                let request_id = Ulid::new();
                let target_digest = digest_of(&json!({
                    "action": action.0,
                    "payload_ref": raw_ref.digest.to_string(),
                }));
                let mut params = std::collections::BTreeMap::new();
                params.insert("headless".to_string(), "true".to_string());
                let approval_request = ActionRequest {
                    id: request_id,
                    task_grant_id: grant.id,
                    action: action.clone(),
                    target_ref: None,
                    payload_ref: Some(raw_ref.clone()),
                    target_digest: Some(target_digest),
                    selection_token_id: None,
                    params,
                    skill_attribution: None,
                    requested_at: now,
                    schema_version: 1,
                };
                state.store.insert_action_request(&approval_request)?;
                let message =
                    format!("Headless hook {action} requires approval (request {request_id}).");
                state
                    .connectors
                    .telegram()
                    .send_reply_with_approval_button(state.owner_user_id, &message, request_id)
                    .await?;
            }
            let detail = format!("headless hook {action} escalated: {other:?}");
            let detail_ref = state.artifacts.put(detail.as_bytes())?;
            state.store.record_headless_hook_completion(
                "headless",
                &detail,
                &detail_ref.digest.to_string(),
                Some(grant.id),
            )?;
            Ok(HeadlessHookOutcome::Escalated(grant.id))
        }
    }
}

// ── Headless lane hooks ────────────────────────────────────────────────────
// `run_pipeline_with_envelope` receives a pre-built, pre-verified envelope, so
// the headless lane's `build_envelope` / `preflight` are never invoked; they
// exist only to satisfy the `LaneSpec` record shape.

fn headless_build_envelope(
    _state: &AppState,
    _inputs: &EventInputs,
    _now: Timestamp,
) -> anyhow::Result<(EventEnvelope, ArtifactRef)> {
    anyhow::bail!(
        "headless lane drives run_pipeline_with_envelope with a prebuilt, verified envelope"
    )
}

fn headless_preflight<'a>(
    _state: &'a AppState,
    _inputs: &'a EventInputs,
    _lane: Lane,
    _now: Timestamp,
) -> Pin<Box<dyn Future<Output = Result<PreflightSnapshot, PreflightFailure>> + Send + 'a>> {
    Box::pin(async move { Ok(PreflightSnapshot::default()) })
}

fn headless_route_guard(
    _state: &AppState,
    _envelope: &EventEnvelope,
    _lane: Lane,
) -> anyhow::Result<bool> {
    // The headless lane never refuses on containment; the route's composed
    // authority is the only gate.
    Ok(false)
}

fn headless_grant_binding(
    _state: &AppState,
    _grant: &mut TaskGrant,
    _inputs: &EventInputs,
    raw_ref: &ArtifactRef,
    _now: Timestamp,
) -> anyhow::Result<ArtifactRef> {
    // The webhook payload ref is the pending task input.
    Ok(raw_ref.clone())
}

/// The headless webhook lane specification.
pub fn headless_lane() -> LaneSpec {
    LaneSpec {
        lane: Lane::BusinessWorkflow,
        channel_trust: ChannelTrust::VerifiedContact,
        purpose: "headless_webhook",
        build_envelope: headless_build_envelope,
        preflight: headless_preflight,
        route_containment_guard: headless_route_guard,
        grant_binding: headless_grant_binding,
    }
}

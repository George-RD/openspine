use std::future::Future;
use std::pin::Pin;

use jiff::Timestamp;
use openspine_authority::{compose_authority, resolve_route, AuthorityInput, AuthorityOutcome};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::event::{ChannelTrust, EventEnvelope, Lane};
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::route::RouteResolution;

use super::lanes::{
    email_build_envelope, email_grant_binding, email_preflight, email_route_guard,
    owner_build_envelope, owner_grant_binding, owner_preflight, owner_route_guard,
};
use super::{empty_session_policy, notify_owner_best_effort, AppState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    Event,
    Verify,
    Identify,
    Route,
    Compose,
    Grant,
    Run,
    Gate,
    Audit,
}

impl PipelineStage {
    /// The canonical, complete stage sequence — declared in exactly one
    /// place. Tests pin this order.
    pub const SEQUENCE: [PipelineStage; 9] = [
        PipelineStage::Event,
        PipelineStage::Verify,
        PipelineStage::Identify,
        PipelineStage::Route,
        PipelineStage::Compose,
        PipelineStage::Grant,
        PipelineStage::Run,
        PipelineStage::Gate,
        PipelineStage::Audit,
    ];

    /// The synchronous prefix the driver actually executes: derived
    /// element-by-element from [`Self::SEQUENCE`], truncated before `Gate`,
    /// so the two declarations cannot drift. The driver's executed-stage
    /// trace is pinned to this prefix by the unit tests.
    pub const SYNC_PREFIX: [PipelineStage; 7] = [
        Self::SEQUENCE[0],
        Self::SEQUENCE[1],
        Self::SEQUENCE[2],
        Self::SEQUENCE[3],
        Self::SEQUENCE[4],
        Self::SEQUENCE[5],
        Self::SEQUENCE[6],
    ];
}
const _: () = {
    assert!(matches!(PipelineStage::SEQUENCE[7], PipelineStage::Gate));
    assert!(matches!(PipelineStage::SEQUENCE[8], PipelineStage::Audit));
    assert!(PipelineStage::SYNC_PREFIX.len() + 2 == PipelineStage::SEQUENCE.len());
};

pub struct EventInputs {
    pub chat_id: i64,
    pub text: String,
    pub thread_id: Option<String>,
    pub owner_verified: Option<crate::telegram::VerifiedOwnerContext>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreflightFailure {
    GmailNotConfigured,
    RefusedUncontained,
    ThreadNotFound { thread_id: String },
    GmailError { err: String },
}

pub type PreflightFn =
    for<'a> fn(
        &'a AppState,
        &'a EventInputs,
        Lane,
        Timestamp,
    ) -> Pin<Box<dyn Future<Output = Result<(), PreflightFailure>> + Send + 'a>>;

pub type BuildEnvelopeFn =
    fn(&AppState, &EventInputs, Timestamp) -> anyhow::Result<(EventEnvelope, ArtifactRef)>;

pub type RouteGuardFn = fn(&AppState, &EventEnvelope, Lane) -> anyhow::Result<bool>;

pub type GrantBindingFn = fn(
    &AppState,
    &mut TaskGrant,
    &EventInputs,
    &ArtifactRef,
    Timestamp,
) -> anyhow::Result<ArtifactRef>;

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

pub async fn run_pipeline(
    state: &AppState,
    spec: LaneSpec,
    inputs: &EventInputs,
    now: Timestamp,
    trace: &mut Vec<PipelineStage>,
) -> anyhow::Result<Option<TaskGrant>> {
    // Event stage — intake + lane selection were performed by the caller
    // (`handle_owner_update`) ahead of this function. Record it and proceed.
    trace.push(PipelineStage::Event);

    // Verify stage — lane preflight. Owner-control: no-op. Email-preview:
    // Gmail configured + containment guard + thread existence, in that order.
    trace.push(PipelineStage::Verify);
    if let Err(failure) = (spec.preflight)(state, inputs, spec.lane, now).await {
        emit_preflight_failure(state, inputs.chat_id, failure).await?;
        return Ok(None);
    }

    // The audited event envelope is emitted by the driver only after Verify
    // succeeds — preflight failures never reach here, so no `event.received`
    // is ever emitted on a preflight-failure path (preserves both flows'
    // audit surface).
    let (envelope, raw_ref) = (spec.build_envelope)(state, inputs, now)?;
    state.store.append_audit(
        "event.received",
        None,
        None,
        None,
        None,
        &[],
        std::slice::from_ref(&raw_ref),
    )?;

    // Identify stage.
    // Identify stage.
    trace.push(PipelineStage::Identify);
    let resolver = crate::identity::IdentityResolver::new(
        &state.store,
        state.owner_principal_id,
        state.owner_identity_id,
    );
    let (identity, relationship) = resolver.resolve(
        envelope.id,
        spec.channel_trust,
        envelope.actor_hint.channel_user_id.as_deref(),
        inputs.owner_verified.as_ref(),
    )?;

    // Route stage.
    trace.push(PipelineStage::Route);
    let routes = state.registry.read().routes.clone();
    let route_resolution = resolve_route(&envelope, &identity, relationship, &routes);
    let route_id = match route_resolution {
        RouteResolution::Success { route_id } => route_id,
        RouteResolution::Denied { reason } => {
            state
                .store
                .append_audit("route.denied", None, None, Some(&reason), None, &[], &[])?;
            return Ok(None);
        }
        RouteResolution::Ambiguous { reason, .. } => {
            state.store.append_audit(
                "route.ambiguous",
                None,
                None,
                Some(&reason),
                None,
                &[],
                &[],
            )?;
            return Ok(None);
        }
    };
    let route = routes
        .iter()
        .find(|r| r.id == route_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("resolved route {route_id} not found in registry"))?;

    // Containment guard (lane-driven): the email-preview lane already ran it
    // in preflight; the owner-control lane runs it here (after route
    // resolution, exactly as the prior owner flow did).
    if (spec.route_containment_guard)(state, &envelope, spec.lane)? {
        return Ok(None);
    }

    let agent_id = route
        .agent
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("route {route_id} names no agent"))?;
    let workflow_id = route
        .workflow
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("route {route_id} names no workflow"))?;
    let pack_id = route
        .capability_pack
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("route {route_id} names no capability_pack"))?;

    let (agent, workflow, pack, global_policy) = {
        let registry = state.registry.read();
        let agent = registry
            .agents
            .get(agent_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("agent {agent_id} not in registry"))?;
        let workflow = registry
            .workflows
            .get(workflow_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("workflow {workflow_id} not in registry"))?;
        let pack = registry
            .packs
            .get(pack_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("capability_pack {pack_id} not in registry"))?;
        let global_policy = registry
            .policies
            .get("global")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("global policy not in registry"))?;
        (agent, workflow, pack, global_policy)
    };

    // Compose stage.
    trace.push(PipelineStage::Compose);
    let principal_id = identity
        .principal_id
        .ok_or_else(|| anyhow::anyhow!("no principal resolved for owner event"))?;
    let session = empty_session_policy();
    let input = AuthorityInput {
        event: &envelope,
        identity: &identity,
        route: &route,
        global_policy: &global_policy,
        agent: &agent,
        workflow: &workflow,
        pack: &pack,
        session: &session,
        principal_id,
        purpose: spec.purpose,
    };
    let mut grant = match compose_authority(&input, &state.action_catalog, now) {
        AuthorityOutcome::Granted(grant) => *grant,
        AuthorityOutcome::Denied { reason } => {
            state.store.append_audit(
                "authority.denied",
                None,
                None,
                Some(&reason),
                None,
                &[],
                &[],
            )?;
            crate::failure_surfacing::notify_immediate_failure(
                state,
                inputs.chat_id,
                crate::failure_surfacing::FailureClass::Authority,
                &format!("Authority denied: {reason}"),
            )
            .await?;
            return Ok(None);
        }
        AuthorityOutcome::UnknownActionId { id, source } => {
            let summary = format!("Unknown action id {id} in {source}");
            state.store.append_audit(
                "authority.unknown_action_id",
                None,
                None,
                Some(&summary),
                None,
                &[],
                &[],
            )?;
            crate::failure_surfacing::notify_immediate_failure(
                state,
                inputs.chat_id,
                crate::failure_surfacing::FailureClass::Authority,
                &summary,
            )
            .await?;
            return Ok(None);
        }
        AuthorityOutcome::Ambiguous { .. } => {
            let summary =
                "compose_authority returned Ambiguous, which it is not expected to produce";
            state.store.append_audit(
                "authority.ambiguous",
                None,
                None,
                Some(summary),
                None,
                &[],
                &[],
            )?;
            crate::failure_surfacing::notify_immediate_failure(
                state,
                inputs.chat_id,
                crate::failure_surfacing::FailureClass::Escalation,
                summary,
            )
            .await?;
            return Ok(None);
        }
    };

    // Grant stage — lane binding, then persist the grant and audit it.
    trace.push(PipelineStage::Grant);
    let pending_ref = (spec.grant_binding)(state, &mut grant, inputs, &raw_ref, now)?;
    let Some(key) = crate::grant_hmac_key() else {
        state.store.append_audit(
            "authority.denied",
            None,
            None,
            Some("grant HMAC key is not configured"),
            None,
            &[],
            &[],
        )?;
        return Ok(None);
    };
    grant.seal_root(&key);
    state
        .store
        .insert_task_grant(&grant, &pending_ref, inputs.chat_id)?;
    state.store.append_audit(
        "authority.granted",
        None,
        None,
        None,
        Some(grant.id),
        &[],
        &[pending_ref],
    )?;

    // Run stage — spawn the sandboxed shell. A spawn failure is audited but
    // does not suppress the already-composed grant.
    trace.push(PipelineStage::Run);
    match state
        .sandbox
        .run_task(&state.kernel_endpoint, &grant.task_token)
        .await
    {
        Ok(()) => {
            state.store.append_audit(
                "task.shell_completed",
                None,
                None,
                None,
                Some(grant.id),
                &[],
                &[],
            )?;
        }
        Err(err) => {
            state.store.append_audit(
                "task.shell_failed",
                None,
                None,
                None,
                Some(grant.id),
                &[],
                &[],
            )?;
            crate::failure_surfacing::batch_failure(
                state,
                crate::failure_surfacing::FailureClass::Resource,
                "shell task failed",
                &format!("shell task failed: {err}"),
            )?;
        }
    }

    Ok(Some(grant))
}

async fn emit_preflight_failure(
    state: &AppState,
    chat_id: i64,
    failure: PreflightFailure,
) -> anyhow::Result<()> {
    match failure {
        PreflightFailure::GmailNotConfigured => {
            state.store.append_audit(
                "selection.gmail_not_configured",
                None,
                None,
                Some("no gmail connector configured; /draft is unavailable"),
                None,
                &[],
                &[],
            )?;
            notify_owner_best_effort(
                state,
                chat_id,
                "Gmail isn't configured on this kernel yet, so /draft is unavailable.",
            )
            .await;
        }
        PreflightFailure::RefusedUncontained => {
            // Matches the prior selection-flow exit: audited, but no owner
            // notification — a security denial stays silent-and-audited like
            // every other denial in this pipeline.
            state.store.append_audit(
                "route.refused_uncontained",
                None,
                None,
                Some("external_communication lane requires a containing sandbox driver"),
                None,
                &[],
                &[],
            )?;
        }
        PreflightFailure::ThreadNotFound { thread_id } => {
            state.store.append_audit(
                "selection.thread_not_found",
                None,
                None,
                Some(&thread_id),
                None,
                &[],
                &[],
            )?;
            notify_owner_best_effort(
                state,
                chat_id,
                &format!("Couldn't find a Gmail thread with id \"{thread_id}\"."),
            )
            .await;
        }
        PreflightFailure::GmailError { err } => {
            state
                .store
                .append_audit("selection.gmail_error", None, None, None, None, &[], &[])?;
            crate::failure_surfacing::batch_failure(
                state,
                crate::failure_surfacing::FailureClass::Connector,
                "gmail connector error",
                &format!("gmail: {err}"),
            )?;
        }
    }
    Ok(())
}

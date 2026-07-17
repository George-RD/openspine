use super::lanes::{
    email_build_envelope, email_grant_binding, email_preflight, email_route_guard,
    owner_build_envelope, owner_grant_binding, owner_preflight, owner_route_guard,
    scheduled_build_envelope, scheduled_grant_binding, scheduled_preflight, scheduled_route_guard,
};
pub use super::stages::PipelineStage;
use super::{empty_session_policy, notify_owner_best_effort, AppState};
use jiff::Timestamp;
use openspine_authority::{compose_authority, resolve_route, AuthorityInput, AuthorityOutcome};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::event::{ChannelTrust, EventEnvelope, Lane};
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::route::RouteResolution;
use std::future::Future;
use std::pin::Pin;
use ulid::Ulid;
pub struct EventInputs {
    pub chat_id: i64,
    pub text: String,
    pub thread_id: Option<String>,
    pub owner_verified: Option<crate::telegram::VerifiedOwnerContext>,
    pub principal_override: Option<Ulid>,
    pub event_type_override: Option<openspine_schemas::event::EventType>,
    #[allow(dead_code)]
    pub timer_event_id: Option<String>,
    pub correlated_task_id: Option<Ulid>,
    pub dispatch_key: Option<String>,
    pub dispatch_timer_id: Option<String>,
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
pub async fn run_pipeline(
    state: &AppState,
    spec: LaneSpec,
    inputs: &EventInputs,
    now: Timestamp,
    trace: &mut Vec<PipelineStage>,
) -> anyhow::Result<Option<TaskGrant>> {
    if !crate::spend::admit_spend(
        state,
        crate::spend::SpendLane::from_event_lane(spec.lane),
        now,
    )
    .await?
    {
        state.store.append_audit(
            "spend.cap_breached",
            None,
            None,
            Some("non-immediate lane paused by global daily spend cap"),
            None,
            &[],
            &[],
        )?;
        return Ok(None);
    }
    trace.push(PipelineStage::Event);
    trace.push(PipelineStage::Verify);
    if let Err(failure) = (spec.preflight)(state, inputs, spec.lane, now).await {
        emit_preflight_failure(state, inputs.chat_id, failure).await?;
        return Ok(None);
    }
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
    if !pack.applies_to.matches(&envelope, relationship) {
        state.store.append_audit(
            "authority.pack_not_applicable",
            None,
            None,
            Some(pack.id.as_str()),
            None,
            &[],
            &[],
        )?;
        return Ok(None);
    }
    trace.push(PipelineStage::Compose);
    let principal_id = identity
        .principal_id
        .or(inputs.principal_override)
        .ok_or_else(|| anyhow::anyhow!("no principal resolved for event"))?;
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
    if let Some(dispatch_key) = inputs.dispatch_key.as_deref() {
        let token_ref = state.artifacts.put(grant.task_token.as_bytes())?;
        state.store.persist_grant_with_handoff(
            dispatch_key,
            &grant,
            &pending_ref,
            inputs.chat_id,
            &token_ref,
            inputs.dispatch_timer_id.as_deref().unwrap_or_default(),
            inputs.correlated_task_id,
        )?;
    } else {
        state
            .store
            .insert_task_grant(&grant, &pending_ref, inputs.chat_id)?;
    }
    if inputs.dispatch_key.is_none() {
        state.store.append_audit(
            "authority.granted",
            None,
            None,
            None,
            Some(grant.id),
            &[],
            &[pending_ref],
        )?;
    }
    trace.push(PipelineStage::Run);
    let handoff_result = state
        .sandbox
        .run_task(&state.kernel_endpoint, &grant.task_token)
        .await;
    match handoff_result {
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
            if let Some(dispatch_key) = inputs.dispatch_key.as_deref() {
                state.store.complete_timer_dispatch(
                    dispatch_key,
                    "handed_off",
                    &grant.id.to_string(),
                )?;
            }
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

//! The single typed pipeline driver and lane specifications.
//!
//! Exposes `run_pipeline` which executes the synchronous prefix of stages.
//! MUST NOT import or call `gate()`.

use jiff::Timestamp;
use openspine_authority::{
    compose_authority, resolve_persona, resolve_route, AuthorityInput, AuthorityOutcome,
};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::event::{EventEnvelope, Lane, Source};
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::route::RouteResolution;
use ulid::Ulid;

use super::driver_failures::emit_preflight_failure;
use super::lanes::{EventInputs, LaneSpec, PreflightSnapshot};
pub use super::stages::PipelineStage;
use super::{empty_session_policy, AppState};

/// Run one verified owner event through the pipeline's synchronous prefix,
/// interpreting `spec` as lane data. Returns `Ok(None)` for every outcome the
/// pipeline itself decides on; `Ok(Some(grant))` once authority has been
/// composed and persisted regardless of whether the shell spawn succeeds; and
/// `Err` only for a genuine infrastructure failure.
///
/// `trace` receives the stages as they execute and must equal
/// [`PipelineStage::SYNC_PREFIX`] on the happy path for both lanes.
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
    let preflight_snapshot = match (spec.preflight)(state, inputs, spec.lane, now).await {
        Ok(snapshot) => snapshot,
        Err(failure) => {
            emit_preflight_failure(state, inputs.chat_id, failure).await?;
            return Ok(None);
        }
    };

    let (envelope, raw_ref) = (spec.build_envelope)(state, inputs, now)?;
    run_pipeline_with_envelope(
        state,
        spec,
        envelope,
        raw_ref,
        inputs,
        now,
        preflight_snapshot,
        trace,
        true, // spawn_shell: ordinary lanes run the conversational shell
    )
    .await
}

/// Run the synchronous pipeline prefix from a pre-built, pre-verified
/// `EventEnvelope` (already past the `Verify` / `preflight` stage).
///
/// The headless webhook lane verifies the webhook signature itself and mints
/// the envelope before driving it through the ordinary
/// `Identify -> Route -> Compose -> Grant -> Run` stages; it calls this
/// function directly with the verified envelope. The body is identical to the
/// post-`Verify` path of `run_pipeline`: `run_pipeline` performs the spend
/// gate + preflight, then delegates here after building the envelope.
///
/// `spawn_shell` controls the `Run` stage: the ordinary lanes spawn the
/// sandboxed conversational shell, but the headless lane passes `false` so no
/// owner conversation can ever be opened by a webhook — its effect is driven
/// later through the non-conversational action executor in `headless.rs`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_pipeline_with_envelope(
    state: &AppState,
    spec: LaneSpec,
    envelope: EventEnvelope,
    raw_ref: ArtifactRef,
    inputs: &EventInputs,
    now: Timestamp,
    preflight_snapshot: PreflightSnapshot,
    trace: &mut Vec<PipelineStage>,
    spawn_shell: bool,
) -> anyhow::Result<Option<TaskGrant>> {
    // The audited event envelope is emitted by the driver only after Verify
    // succeeds — preflight failures never reach here, so no `event.received`
    // is ever emitted on a preflight-failure path (preserves both flows'
    // audit surface).
    if spec.lane == Lane::OwnerControl {
        // AD-034: only the owner-control lane has the authorized plaintext
        // and `owner_control` aggregate required for the manipulation screen.
        // The event and structured signal commit atomically.
        state
            .store
            .append_event_received_with_screen(&raw_ref, &inputs.text)?;
    } else {
        // Other lanes retain their ordinary audited event path; their content
        // must not be attributed to owner-control screening authority.
        state.store.append_audit(
            "event.received",
            None,
            None,
            None,
            None,
            &[],
            std::slice::from_ref(&raw_ref),
        )?;
    }
    trace.push(PipelineStage::Identify);
    let resolver = crate::identity::IdentityResolver::new(
        &state.store,
        state.owner_principal_id,
        state.owner_identity_id,
    );
    let (mut identity, relationship) = resolver.resolve(
        envelope.id,
        spec.channel_trust,
        envelope.actor_hint.channel_user_id.as_deref(),
        inputs.owner_verified.as_ref(),
    )?;
    // A webhook's verifier is the source-authentication proof. There is no
    // Telegram owner context to pass to the identity resolver, so preserve
    // that kernel proof for the authority check without inventing a sender
    // identity or relationship.
    if envelope.source == Source::Webhook && envelope.verified_source {
        identity.source_verified = true;
    }
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
    let (agent, workflow, pack, global_policy) = {
        let registry = state.registry.read();
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
    // AD-136: persona binding is kernel machinery, resolved AFTER the
    // deterministic route/identity match and BEFORE the grant is sealed.
    // Personas carry no authority (D-094): `persona_id` is an
    // audit/fronting field only, never an authority composition input.
    // Because it derives from the matched `route`, a counterparty
    // reaching an owner-bound number cannot select the owner persona.
    grant.persona_id = {
        let personas = &state.registry.read().personas;
        resolve_persona(&envelope.channel_account, relationship, &route, personas)
    };
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
        crate::failure_surfacing::notify_immediate_failure(
            state,
            inputs.chat_id,
            crate::failure_surfacing::FailureClass::Authority,
            "Grant HMAC key is not configured",
        )
        .await?;
        return Ok(None);
    };

    let rollback_tokens = |ids: &[Ulid]| -> anyhow::Result<()> {
        for token_id in ids {
            state
                .store
                .delete_selection_token(*token_id)
                .map_err(|err| anyhow::anyhow!("selection-token rollback failed: {err}"))?;
        }
        Ok(())
    };
    // Grant stage — lane binding, then persist the grant and audit it.
    trace.push(PipelineStage::Grant);
    let pending_ref = (spec.grant_binding)(state, &mut grant, inputs, &raw_ref, now)?;
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
        let briefcase_result = crate::briefcase::pack_for_pipeline(
            state,
            inputs.thread_id.as_deref(),
            spec.lane,
            &grant,
            preflight_snapshot.counterparty_address.as_deref(),
        )
        .await;
        let briefcase = match briefcase_result {
            Ok(b) => b,
            Err(err) => {
                rollback_tokens(&grant.selection_tokens)?;
                return Err(err.into());
            }
        };
        let insert_result = state.store.insert_grant_and_briefcase_atomic(
            &grant,
            &pending_ref,
            inputs.chat_id,
            &briefcase,
        );
        if let Err(err) = insert_result {
            rollback_tokens(&grant.selection_tokens)?;
            return Err(err.into());
        }
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

    // Run stage — spawn the sandboxed shell (ordinary lanes only). A spawn
    // failure is audited but does not suppress the already-composed grant.
    trace.push(PipelineStage::Run);
    if spawn_shell {
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
                tracing::error!(error = %err, grant_id = %grant.id, "worker shell failed");
                state.store.append_audit(
                    "task.shell_failed",
                    None,
                    None,
                    Some("worker shell failed"),
                    Some(grant.id),
                    &[],
                    &[],
                )?;
            }
        }
    } else {
        // Headless lane: no shell. The effect is driven later through the
        // non-conversational action executor (`headless::run_headless_hook`),
        // which cannot open an owner conversation.
        state.store.append_audit(
            "task.shell_skipped",
            None,
            None,
            Some("headless lane: no conversational shell"),
            Some(grant.id),
            &[],
            &[],
        )?;
    }
    Ok(Some(grant))
}

//! Scheduled reflection-miner grant composition and driver (AD-050/AD-149).

use jiff::Timestamp;
use openspine_authority::{compose_authority, resolve_route, AuthorityInput, AuthorityOutcome};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::event::{
    ActorHint, ChannelTrust, DataClassification, EventEnvelope, EventType, InteractionMode, Lane,
    Source, TrustContext, VerificationMethod,
};
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::identity::{IdentityResolution, MatchedIdentifierType, RelationshipKind};
use openspine_schemas::policy::{Constraints, SessionPolicy};
use openspine_schemas::reflection_miner::{
    ApprovalObservation, ReflectionObservation, ReflectionProvenance,
};
use openspine_schemas::route::RouteResolution;
use ulid::Ulid;

use super::{run_reflection_miner, MinerRuntimeError};
use crate::pipeline::AppState;
use crate::store::StoreError;

/// Route id for the scheduled reflection-miner grant.
pub(crate) const REFLECTION_SCHEDULED_MINER_ROUTE: &str = "reflection_scheduled_miner";
/// Route id for the scheduled proposal-submitter grant.
pub(crate) const REFLECTION_SCHEDULED_SUBMITTER_ROUTE: &str = "reflection_scheduled_submitter";

/// Find the newest authenticated, unexpired grant for `route`.
pub(crate) fn find_active_grant_by_route(
    state: &AppState,
    route: &str,
) -> Result<Option<(TaskGrant, ArtifactRef, i64)>, MinerRuntimeError> {
    let key = crate::grant_hmac_key().ok_or(MinerRuntimeError::GrantKeyUnavailable)?;
    let conn = state.store.conn.lock();
    let rows: Vec<(String, String, i64)> = (|| -> Result<_, StoreError> {
        let mut statement = conn.prepare(
            "SELECT grant_json, pending_message_digest, bound_chat_id
             FROM task_grants
             WHERE json_extract(grant_json, '$.route_id') = ?1
             ORDER BY id DESC",
        )?;
        let rows = statement
            .query_map([route], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })()?;
    drop(conn);
    let now = Timestamp::now();
    for (grant_json, digest, chat) in rows {
        let grant: TaskGrant = serde_json::from_str(&grant_json).map_err(StoreError::from)?;
        if grant.is_expired(now) {
            continue;
        }
        if grant.user != state.owner_principal_id.to_string() || !grant.verify_mac(&key) {
            return Err(MinerRuntimeError::UnauthenticatedGrant);
        }
        let digest = openspine_schemas::digest::Digest::parse(digest)
            .map_err(|_| StoreError::BadDigest("pending_message_digest".into()))?;
        return Ok(Some((
            grant,
            ArtifactRef {
                digest,
                schema_version: 1,
            },
            chat,
        )));
    }
    Ok(None)
}

fn compose_scheduled_grant(
    state: &AppState,
    expected_route_id: &str,
    channel_account: &str,
    purpose: &str,
) -> Result<(TaskGrant, ArtifactRef, i64), MinerRuntimeError> {
    let now = Timestamp::now();
    let raw_ref = state
        .artifacts
        .put(format!("timer.reflection.fired:{expected_route_id}:{}", Ulid::new()).as_bytes())?;
    let event = EventEnvelope {
        id: Ulid::new(),
        source: Source::Internal,
        connector: None,
        account_role: None,
        event_type: EventType::TimerReflectionFired,
        received_at: now,
        verified_source: true,
        verification_method: VerificationMethod::None,
        replay_protected: true,
        replay_nonce: None,
        channel_account: channel_account.to_string(),
        raw_event_ref: raw_ref.clone(),
        actor_hint: ActorHint::default(),
        target_refs: vec![],
        data_classification: DataClassification::Private,
        user_intent_hint: Some(purpose.to_string()),
        lane: Lane::ScheduledInternal,
        trust_context: TrustContext {
            channel_trust: ChannelTrust::OwnerDevice,
            interaction_mode: InteractionMode::Scheduled,
        },
        thread_id: None,
        schema_version: 1,
    };
    let identity = IdentityResolution {
        event_id: event.id,
        matched_identity_id: Some(state.owner_identity_id),
        principal_id: Some(state.owner_principal_id),
        confidence: 1.0,
        matched_identifier_type: MatchedIdentifierType::Device,
        channel_trust: ChannelTrust::OwnerDevice,
        source_verified: true,
        authority_warning: None,
        schema_version: 1,
    };
    let (route, agent, workflow, pack, global_policy) = {
        let registry = state.registry.read();
        let route_id = match resolve_route(
            &event,
            &identity,
            Some(RelationshipKind::Owner),
            &registry.routes,
        ) {
            RouteResolution::Success { route_id } if route_id == expected_route_id => route_id,
            other => return Err(MinerRuntimeError::Route(format!("{other:?}"))),
        };
        let route = registry
            .routes
            .iter()
            .find(|candidate| candidate.id == route_id)
            .cloned()
            .ok_or_else(|| MinerRuntimeError::Registry(format!("route {route_id}")))?;
        let agent_id = route
            .agent
            .as_ref()
            .ok_or_else(|| MinerRuntimeError::Registry(format!("route {route_id} agent")))?;
        let workflow_id = route
            .workflow
            .as_ref()
            .ok_or_else(|| MinerRuntimeError::Registry(format!("route {route_id} workflow")))?;
        let pack_id = route.capability_pack.as_ref().ok_or_else(|| {
            MinerRuntimeError::Registry(format!("route {route_id} capability pack"))
        })?;
        let agent = registry
            .agents
            .get(agent_id)
            .cloned()
            .ok_or_else(|| MinerRuntimeError::Registry(format!("agent {agent_id}")))?;
        let workflow = registry
            .workflows
            .get(workflow_id)
            .cloned()
            .ok_or_else(|| MinerRuntimeError::Registry(format!("workflow {workflow_id}")))?;
        let pack = registry
            .packs
            .get(pack_id)
            .cloned()
            .ok_or_else(|| MinerRuntimeError::Registry(format!("pack {pack_id}")))?;
        let global_policy = registry
            .policies
            .get("global")
            .cloned()
            .ok_or_else(|| MinerRuntimeError::Registry("policy global".into()))?;
        (route, agent, workflow, pack, global_policy)
    };
    if workflow.required_agent != agent.id
        || workflow.required_capability_pack != pack.id
        || !pack
            .applies_to
            .matches(&event, Some(RelationshipKind::Owner))
    {
        return Err(MinerRuntimeError::Registry(format!(
            "route {expected_route_id} artifact bindings"
        )));
    }
    let session = SessionPolicy {
        schema_version: 1,
        candidate_allowed_actions: vec![],
        approval_required: vec![],
        denied_actions: vec![],
        constraints: Constraints::default(),
    };
    let input = AuthorityInput {
        event: &event,
        identity: &identity,
        route: &route,
        global_policy: &global_policy,
        agent: &agent,
        workflow: &workflow,
        pack: &pack,
        session: &session,
        principal_id: state.owner_principal_id,
        purpose,
    };
    let mut grant = match compose_authority(&input, &state.action_catalog, now) {
        AuthorityOutcome::Granted(grant) => *grant,
        other => return Err(MinerRuntimeError::Authority(format!("{other:?}"))),
    };
    let key = crate::grant_hmac_key().ok_or(MinerRuntimeError::GrantKeyUnavailable)?;
    grant.seal_root(&key);
    state
        .store
        .insert_task_grant(&grant, &raw_ref, state.owner_user_id)?;
    Ok((grant, raw_ref, state.owner_user_id))
}

fn derive_repeated_approval_observation(
    state: &AppState,
    miner_grant_id: Ulid,
    ceiling: DataClassification,
) -> Result<Option<ReflectionObservation>, MinerRuntimeError> {
    let key = crate::grant_hmac_key().ok_or(MinerRuntimeError::GrantKeyUnavailable)?;
    let scope = format!("reflection:{miner_grant_id}");
    let entries = state.store.load_owner_miner_audit_slice(
        &state.owner_principal_id.to_string(),
        &key,
        &scope,
        ceiling,
    )?;
    let mut counts: std::collections::HashMap<(String, String), (usize, usize)> =
        std::collections::HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        let Some(action_id) = state
            .store
            .audit_event_by_id(entry.event_id)?
            .and_then(|event| event.action)
            .map(|action| action.as_str().to_string())
        else {
            continue;
        };
        let counter = counts
            .entry((entry.artifact_id.clone(), action_id))
            .or_insert((0, index));
        counter.0 += 1;
    }
    let Some(((dominant_id, action_id), (count, entry_index))) =
        counts.into_iter().max_by_key(|(_, (count, _))| *count)
    else {
        return Ok(None);
    };
    if count < 2 {
        return Ok(None);
    }
    let entry = &entries[entry_index];
    Ok(Some(ReflectionObservation::RepeatedApproval(
        ApprovalObservation {
            kind: "standing_rule".into(),
            artifact_id: entry.artifact_id.clone(),
            version: 1,
            action_id: action_id.clone(),
            candidate: format!("Recurring owner approval of {dominant_id} ({action_id})"),
            provenance: ReflectionProvenance {
                source_event_id: entry.event_id,
                source_exchange: entry.exchange.clone(),
            },
        },
    )))
}

/// Run one scheduled reflection-miner pass.
pub(crate) async fn reflection_miner_tick(state: &AppState) -> Result<u32, MinerRuntimeError> {
    let (miner_grant, _, _) =
        match find_active_grant_by_route(state, REFLECTION_SCHEDULED_MINER_ROUTE)? {
            Some(grant) => grant,
            None => compose_scheduled_grant(
                state,
                REFLECTION_SCHEDULED_MINER_ROUTE,
                "reflection-miner",
                "AD-050 scheduled reflection miner",
            )?,
        };
    let (submitting_grant, _, _) =
        match find_active_grant_by_route(state, REFLECTION_SCHEDULED_SUBMITTER_ROUTE)? {
            Some(grant) => grant,
            None => compose_scheduled_grant(
                state,
                REFLECTION_SCHEDULED_SUBMITTER_ROUTE,
                "reflection-submitter",
                "AD-050 scheduled reflection submitter",
            )?,
        };
    let pack_constraints = state
        .registry
        .read()
        .packs
        .get("reflection_miner_pack")
        .map(|pack| pack.constraints.clone())
        .ok_or_else(|| MinerRuntimeError::Registry("pack reflection_miner_pack".into()))?;
    let ceiling = pack_constraints
        .data_classification_max
        .unwrap_or(DataClassification::Private);
    let Some(observation) = derive_repeated_approval_observation(state, miner_grant.id, ceiling)?
    else {
        return Ok(0);
    };
    let ReflectionObservation::RepeatedApproval(candidate) = &observation else {
        return Ok(0);
    };
    if state.store.count_owner_control_conversation_turns()? == 0
        || state.store.proposed_artifact_exists(
            "standing_rule",
            &candidate.artifact_id,
            candidate.version,
        )?
        || state
            .registry
            .read()
            .standing_rules
            .contains_key(&candidate.artifact_id)
    {
        return Ok(0);
    }
    run_reflection_miner(
        state,
        std::slice::from_ref(&observation),
        &pack_constraints,
        miner_grant.id,
        submitting_grant.id,
        state.owner_user_id,
    )
    .await
}

/// Run the config-backed periodic driver; per-tick failures remain isolated.
pub(crate) async fn run_reflection_miner_driver(state: &AppState, interval: std::time::Duration) {
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;
        if let Err(err) = reflection_miner_tick(state).await {
            tracing::error!(error = %err, "scheduled reflection-miner tick failed");
        }
    }
}

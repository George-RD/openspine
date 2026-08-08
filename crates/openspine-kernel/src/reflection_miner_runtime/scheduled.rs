//! Scheduled reflection-miner grant composition and driver (AD-050/AD-149).

use jiff::Timestamp;
use openspine_authority::{compose_authority, resolve_route, AuthorityInput, AuthorityOutcome};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::delegation_evidence::{DelegationEvidence, OwnerApprovalEvidence};
use openspine_schemas::digest::Digest;
use openspine_schemas::event::{
    ActorHint, ChannelTrust, DataClassification, EventEnvelope, EventType, InteractionMode, Lane,
    Source, TrustContext, VerificationMethod,
};
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::identity::{IdentityResolution, MatchedIdentifierType, RelationshipKind};
use openspine_schemas::owner_surface::OwnerSurfaceRef;
use openspine_schemas::policy::{Constraints, SessionPolicy};
use openspine_schemas::reflection_miner::{
    ApprovalObservation, ReflectionObservation, ReflectionProvenance,
};
use openspine_schemas::route::RouteResolution;
use openspine_schemas::standing_rule::ReviewedScopeBinding;
use std::collections::{BTreeMap, BTreeSet};
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
) -> Result<Option<(TaskGrant, ArtifactRef, OwnerSurfaceRef)>, MinerRuntimeError> {
    let key = crate::grant_hmac_key().ok_or(MinerRuntimeError::GrantKeyUnavailable)?;
    let conn = state.store.conn.lock();
    let rows: Vec<(String, String, Option<String>)> = (|| -> Result<_, StoreError> {
        let mut statement = conn.prepare(
            "SELECT grant_json, pending_message_digest, owner_surface_json
             FROM task_grants
             WHERE json_extract(grant_json, '$.route_id') = ?1
             ORDER BY id DESC",
        )?;
        let rows = statement
            .query_map([route], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })()?;
    drop(conn);
    let now = Timestamp::now();
    for (grant_json, digest, surface_json) in rows {
        let grant: TaskGrant = serde_json::from_str(&grant_json).map_err(StoreError::from)?;
        if grant.is_expired(now) {
            continue;
        }
        if grant.user != state.owner_principal_id.to_string() || !grant.verify_mac(&key) {
            return Err(MinerRuntimeError::UnauthenticatedGrant);
        }
        let digest = openspine_schemas::digest::Digest::parse(digest)
            .map_err(|_| StoreError::BadDigest("pending_message_digest".into()))?;
        // A pre-v7 grant row has no channel-neutral owner binding; refuse it
        // rather than resurrecting a scheduled miner against an unauthenticated
        // surface (fail closed).
        let surface: OwnerSurfaceRef = serde_json::from_str(
            &surface_json.ok_or_else(|| StoreError::BadOwnerSurface("task_grants".into()))?,
        )
        .map_err(|err| StoreError::BadOwnerSurface(err.to_string()))?;
        return Ok(Some((
            grant,
            ArtifactRef {
                digest,
                schema_version: 1,
            },
            surface,
        )));
    }
    Ok(None)
}

fn compose_scheduled_grant(
    state: &AppState,
    expected_route_id: &str,
    channel_account: &str,
    purpose: &str,
) -> Result<(TaskGrant, ArtifactRef, OwnerSurfaceRef), MinerRuntimeError> {
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
        .insert_task_grant(&grant, &raw_ref, &state.telegram_owner_surface())?;
    Ok((grant, raw_ref, state.telegram_owner_surface()))
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
    type Group = (
        Digest,
        Vec<OwnerApprovalEvidence>,
        BTreeSet<Ulid>,
        ReviewedScopeBinding,
        ArtifactRef,
        Ulid,
    );
    let mut groups: BTreeMap<(String, String), Group> = BTreeMap::new();
    for entry in &entries {
        let Some(event) = state.store.audit_event_by_id(entry.event_id)? else {
            continue;
        };
        let Some(action) = event.action.as_ref() else {
            continue;
        };
        if action.as_str() != "email.create_draft" {
            continue;
        }
        let Some(metadata) = crate::store::OwnerApprovalAuditMetadata::from_payload_json(
            event.payload_json.as_deref(),
        ) else {
            // Historical approval rows have no resolved context metadata and
            // cannot contribute evidence without guessing.
            continue;
        };
        let Ok(scope_bytes) = state
            .artifacts
            .get_scoped(metadata.counterparty_scope_id, &metadata.reviewed_scope_ref)
        else {
            continue;
        };
        let Ok(scope_artifact) =
            serde_json::from_slice::<crate::store::OwnerApprovalScopeArtifact>(&scope_bytes)
        else {
            continue;
        };
        let binding = ReviewedScopeBinding::derive_from(
            scope_artifact.scope,
            scope_artifact.compatibility_digest,
        );
        if binding.reviewed_scope_digest != metadata.reviewed_scope_digest
            || binding.compatibility_digest != metadata.compatibility_digest
            || binding.scope.context_class_digest() != &metadata.context_class_digest
        {
            continue;
        }
        let group_key = (
            action.as_str().to_string(),
            metadata.context_class_digest.to_string(),
        );
        let group = groups.entry(group_key).or_insert_with(|| {
            (
                metadata.context_class_digest.clone(),
                Vec::new(),
                BTreeSet::new(),
                binding.clone(),
                entry.exchange.clone(),
                event.id,
            )
        });
        if group.3 != binding || !group.2.insert(event.id) {
            continue;
        }
        group.1.push(OwnerApprovalEvidence {
            decision_event_id: event.id,
            owner_principal_id: state.owner_principal_id,
            request_digest: metadata.request_digest,
            target_digest: metadata.target_digest,
            payload_digest: metadata.payload_digest,
        });
    }

    let mut selected: Option<(
        u32,
        DelegationEvidence,
        ReviewedScopeBinding,
        ArtifactRef,
        Ulid,
    )> = None;
    for (_, (context_class_digest, approvals, _, binding, exchange, source_event_id)) in groups {
        let Ok(evidence) = DelegationEvidence::repeated_approvals(context_class_digest, approvals)
        else {
            continue;
        };
        let Some(count) = evidence.approval_count() else {
            continue;
        };
        if selected
            .as_ref()
            .is_none_or(|(selected_count, _, _, _, _)| count > *selected_count)
        {
            selected = Some((count, evidence, binding, exchange, source_event_id));
        }
    }
    let Some((count, evidence, binding, exchange, source_event_id)) = selected else {
        return Ok(None);
    };
    let context_class_digest = evidence
        .context_class_digest()
        .expect("repeated approval evidence always has a context class digest");
    Ok(Some(ReflectionObservation::RepeatedApproval(Box::new(
        ApprovalObservation {
            kind: "standing_rule".into(),
            artifact_id: context_class_digest.to_string(),
            version: 1,
            action_id: "email.create_draft".into(),
            candidate: format!("{count} matching owner approvals"),
            evidence,
            reviewed_scope_binding: binding,
            provenance: ReflectionProvenance {
                source_event_id,
                source_exchange: exchange,
            },
        },
    ))))
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
        &state.telegram_owner_surface(),
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

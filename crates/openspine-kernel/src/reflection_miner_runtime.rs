//! Kernel reflection-miner runtime (AD-022/050/053/054/135/149).
//!
//! This is the ONLY place that constructs `OrdinaryMinerGrant` and packs the
//! `MinerBriefcase`. The schema types are pure/non-authorizing; this module
//! owns the security boundary: it packs the briefcase from the verified audit
//! store, enforces durable per-grant budgets through `BEGIN IMMEDIATE`
//! transactions, rechecks expiry at dispatch, and dispatches proposals through
//! the normal `artifact.propose` lifecycle. It is the AD-135 owner-correction →
//! miner → proposal route.

use jiff::Timestamp;
use openspine_gate::{gate, ActionOrigin};
use openspine_schemas::action::{ActionId, ActionRequest, GateDecision};
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::policy::Constraints;
use openspine_schemas::reflection_miner::{
    MinerBriefcase, MinerError, OrdinaryMinerGrant, ReflectionMiner, ReflectionObservation,
    ReflectionProposal, ReflectionProposalBody,
};
use serde_json::json;
use ulid::Ulid;

use crate::api::actions::mediate_and_dispatch_action_headless;
use crate::pipeline::AppState;
use crate::store::StoreError;

#[path = "reflection_miner_runtime/scheduled.rs"]
mod scheduled;
pub(crate) use scheduled::run_reflection_miner_driver;
#[cfg(test)]
pub(crate) use scheduled::{
    find_active_grant_by_route, reflection_miner_tick, REFLECTION_SCHEDULED_MINER_ROUTE,
    REFLECTION_SCHEDULED_SUBMITTER_ROUTE,
};

/// Error type for the kernel reflection-miner route.
#[derive(Debug, thiserror::Error)]
pub enum MinerRuntimeError {
    #[error("store error packing miner briefcase: {0}")]
    Store(#[from] StoreError),
    #[error("artifact-store error preparing scheduled miner input: {0}")]
    Artifact(#[from] crate::artifact_store::ArtifactStoreError),
    #[error("miner grant admission rejected: {0}")]
    Admission(#[from] MinerError),
    #[error("durable artifact budget exhausted for miner grant")]
    ArtifactBudgetExhausted,
    #[error("miner grant expired before dispatch")]
    GrantExpiredAtDispatch,
    #[error("failed to serialize miner proposal payload")]
    Payload,
    #[error("dispatch through normal lifecycle failed: {0}")]
    Dispatch(String),
    #[error("referenced grant is not present in the verified store")]
    GrantNotFound,
    #[error("durable model-call budget exhausted for miner grant")]
    ModelBudgetExhausted,
    #[error("miner model operation denied at gate: {0}")]
    ModelGateDenied(String),
    #[error("grant HMAC key is unavailable")]
    GrantKeyUnavailable,
    #[error("persisted scheduled grant failed authentication")]
    UnauthenticatedGrant,
    #[error("missing or inconsistent reflection artifact: {0}")]
    Registry(String),
    #[error("scheduled reflection route resolution failed: {0}")]
    Route(String),
    #[error("scheduled reflection authority composition failed: {0}")]
    Authority(String),
}

/// Reserve one model call for an already authenticated/gated miner grant.
/// Provider invocation remains outside this helper; the durable reservation is
/// the kernel's model-budget boundary.
pub(crate) fn reserve_model_call(
    state: &AppState,
    grant_id: Ulid,
    max_calls: u32,
) -> Result<(), MinerRuntimeError> {
    if state.store.try_count_model_call(grant_id, max_calls)? {
        Ok(())
    } else {
        Err(MinerRuntimeError::ModelBudgetExhausted)
    }
}

fn gate_and_reserve_model_call(
    state: &AppState,
    grant: &TaskGrant,
) -> Result<(), MinerRuntimeError> {
    let now = Timestamp::now();
    let action = ActionId::new("model.generate:approved_provider");
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: action.clone(),
        target_ref: None,
        payload_ref: None,
        target_digest: None,
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        skill_attribution: None,
        requested_at: now,
        schema_version: 1,
    };
    let outcome = gate(
        grant,
        &request,
        ActionOrigin::Shell,
        &state.store,
        &state.action_catalog,
        &state.connectors,
        now,
    );
    state.store.append_audit(
        "reflection.miner.model_gated",
        Some(&action),
        Some(&outcome.decision),
        None,
        Some(grant.id),
        &[],
        &[],
    )?;
    if outcome.decision != GateDecision::Allow {
        return Err(MinerRuntimeError::ModelGateDenied(format!(
            "{:?}",
            outcome.decision
        )));
    }
    reserve_model_call(state, grant.id, grant.limits.max_model_calls)
}

/// AD-135 route. Reloads the canonical persisted grants, authenticates their
/// MACs, packs a read-only owner-scoped briefcase from the verified audit
/// ledger, admits the ordinary miner grant, and sends every result through
/// the same gated `artifact.propose` mediation used by other workers.
pub(crate) async fn run_reflection_miner(
    state: &AppState,
    observations: &[ReflectionObservation],
    pack_constraints: &Constraints,
    miner_grant_id: Ulid,
    submitting_grant_id: Ulid,
    owner_chat_id: i64,
) -> Result<u32, MinerRuntimeError> {
    let (miner_grant, _, _) = state
        .store
        .find_task_grant_by_id(miner_grant_id)?
        .ok_or(MinerRuntimeError::GrantNotFound)?;
    let (submitting_grant, _, _) = state
        .store
        .find_task_grant_by_id(submitting_grant_id)?
        .ok_or(MinerRuntimeError::GrantNotFound)?;
    let key = crate::grant_hmac_key().ok_or(MinerRuntimeError::GrantKeyUnavailable)?;
    let owner_principal = state.owner_principal_id.to_string();
    if !miner_grant.verify_mac(&key)
        || !submitting_grant.verify_mac(&key)
        || miner_grant.user != owner_principal
        || submitting_grant.user != owner_principal
    {
        return Err(MinerRuntimeError::UnauthenticatedGrant);
    }
    gate_and_reserve_model_call(state, &miner_grant)?;

    let ceiling = pack_constraints
        .data_classification_max
        .unwrap_or(openspine_schemas::event::DataClassification::Private);
    let scope = format!("reflection:{}", miner_grant.id);

    // Kernel-packed slice: the miner sees only allowed, provenance-bearing
    // events emitted under this owner principal, stamped into this grant's
    // immutable scope.
    let mut entries =
        state
            .store
            .load_owner_miner_audit_slice(&owner_principal, &key, &scope, ceiling)?;

    // Corrections and stated preferences may originate from non-Allow audit
    // rows, so admit their anchor only when it belongs to the authenticated
    // submitting grant. Repeated approvals are already present in the
    // owner-scoped allowed-event slice above.
    for observation in observations {
        let provenance = match observation {
            ReflectionObservation::Correction(c) => &c.provenance,
            ReflectionObservation::StatedPreference(p) => &p.provenance,
            ReflectionObservation::RepeatedApproval(_) => continue,
        };
        if let Some(event) = state.store.audit_event_by_id(provenance.source_event_id)? {
            if event.task_grant_id != Some(submitting_grant_id) {
                continue;
            }
            if let Some(exchange) = event
                .target_refs
                .first()
                .or_else(|| event.payload_refs.first())
                .cloned()
            {
                entries.push(openspine_schemas::reflection_miner::AuditTrailEntry {
                    scope: scope.clone(),
                    artifact_id: exchange.digest.as_str().to_string(),
                    event_id: event.id,
                    exchange,
                    classification: ceiling,
                });
            }
        }
    }

    let briefcase = MinerBriefcase::scoped(miner_grant.id, &scope, entries)?;
    let grant = OrdinaryMinerGrant::admit(&miner_grant, pack_constraints, briefcase)?;
    let proposals = ReflectionMiner.mine(&grant, observations)?;

    let mut dispatched = 0u32;
    for proposal in proposals {
        dispatch_reflection_proposal(state, &grant, proposal, &submitting_grant, owner_chat_id)
            .await?;
        dispatched += 1;
    }
    Ok(dispatched)
}

/// Dispatch one miner proposal: recheck expiry, charge the durable artifact
/// budget (BEGIN IMMEDIATE), retain source provenance + reason in the audit
/// ledger, then enter the normal `artifact.propose` lifecycle.
async fn dispatch_reflection_proposal(
    state: &AppState,
    grant: &OrdinaryMinerGrant,
    proposal: ReflectionProposal,
    submitting_grant: &TaskGrant,
    owner_chat_id: i64,
) -> Result<(), MinerRuntimeError> {
    // A queued admitted miner must not emit proposals after grant expiry.
    if grant.expires_at < Timestamp::now() {
        return Err(MinerRuntimeError::GrantExpiredAtDispatch);
    }
    // Durable, transactional artifact budget — resets only on grant expiry.
    if !state
        .store
        .try_count_artifact_put(grant.grant_id, grant.limits.max_artifacts)?
    {
        return Err(MinerRuntimeError::ArtifactBudgetExhausted);
    }
    let payload = proposal
        .to_proposal_payload()
        .map_err(|_| MinerRuntimeError::Payload)?;

    // Retain source provenance + reason + eval probe in the audit ledger so
    // the metadata survives the strict PersonaElement YAML the normal
    // lifecycle persists. This row is the only lifecycle retention for that
    // metadata, so its failure is fatal.
    let (reason, eval_probe) = match &proposal.body {
        ReflectionProposalBody::InstructionRewrite {
            reason, eval_probe, ..
        } => (reason.clone(), eval_probe.clone()),
        _ => (String::new(), None),
    };
    let provenance_json = json!({
        "source_event_id": proposal.provenance.source_event_id.to_string(),
        "artifact_id": proposal.artifact_id,
        "version": proposal.version,
        "reason": reason,
        "eval_probe": eval_probe,
    });
    state
        .store
        .append_audit(
            "reflection.miner.provenance",
            None,
            None,
            Some(&provenance_json.to_string()),
            Some(grant.grant_id),
            std::slice::from_ref(&proposal.provenance.source_exchange),
            &[],
        )
        .map_err(MinerRuntimeError::Store)?;

    let (decision, _, _, _) = mediate_and_dispatch_action_headless(
        state,
        submitting_grant,
        ActionId::new("artifact.propose"),
        owner_chat_id,
        Some(&payload),
    )
    .await
    .map_err(|error| MinerRuntimeError::Dispatch(format!("{error:?}")))?;
    if decision != GateDecision::Allow {
        return Err(MinerRuntimeError::Dispatch(format!(
            "artifact.propose denied at gate: {decision:?}"
        )));
    }
    Ok(())
}

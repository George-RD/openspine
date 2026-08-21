// openspine:allow-large-module reason: reflection-miner runtime owns briefcase packing, budgeted audit selection, proposal dispatch, and evaluation-bound owner-review origination as one kernel trust boundary.
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
use openspine_schemas::digest::{canonical_json, Digest};
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::owner_review::{
    BoundaryBehavior, OwnerReviewDecision, OwnerReviewEvaluationBinding,
    OwnerReviewEvaluationEpochs, OwnerReviewRequest, OwnerReviewRequestInput, ProposalKind,
    ResponsibilityLifecycleControl, ReviewFallbackBehavior, ReviewLimits,
};
use openspine_schemas::policy::Constraints;
use openspine_schemas::reflection_miner::{
    MinerBriefcase, MinerError, OrdinaryMinerGrant, ReflectionMiner, ReflectionObservation,
    ReflectionProposal, ReflectionProposalBody,
};
use serde_json::json;
use ulid::Ulid;

use crate::api::actions::mediate_and_dispatch_action_headless;
use crate::api::artifact_propose::dispatch_artifact_propose_core;
use crate::artifact_loader::{parse_proposal, ParsedProposal};
use crate::pipeline::owner_review_surface::persist_owner_review;
use crate::pipeline::AppState;
use crate::store::StoreError;
use openspine_schemas::owner_surface::{OwnerSurfaceKind, OwnerSurfaceRef};

#[path = "reflection_miner_runtime/scheduled.rs"]
mod scheduled;
pub(crate) use scheduled::run_reflection_miner_driver;
#[cfg(test)]
pub(crate) use scheduled::{
    find_active_grant_by_route, reflection_miner_tick, REFLECTION_SCHEDULED_MINER_ROUTE,
    REFLECTION_SCHEDULED_SUBMITTER_ROUTE,
};
#[cfg(test)]
#[path = "reflection_miner_runtime/binding_hooks.rs"]
mod binding_hooks;
#[cfg(test)]
pub(crate) use binding_hooks::{
    apply_dispatch_test_mutation, set_dispatch_test_mutation, DispatchTestMutation,
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
    owner_surface: &OwnerSurfaceRef,
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
    let owner_principal: openspine_schemas::ids::PrincipalId = state.owner.principal_id;
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
            .load_owner_miner_audit_slice(owner_principal, &key, &scope, ceiling)?;

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
        dispatch_reflection_proposal(state, &grant, proposal, &submitting_grant, owner_surface)
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
    owner_surface: &OwnerSurfaceRef,
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

    let receipt = if proposal.evidence.is_some() {
        Some(
            dispatch_miner_evaluated_proposal(state, submitting_grant, owner_surface, &payload)
                .await?,
        )
    } else {
        let (decision, _, _, _) = mediate_and_dispatch_action_headless(
            state,
            submitting_grant,
            ActionId::new("artifact.propose"),
            owner_surface,
            Some(&payload),
        )
        .await
        .map_err(|error| MinerRuntimeError::Dispatch(format!("{error:?}")))?;
        if decision != GateDecision::Allow {
            return Err(MinerRuntimeError::Dispatch(format!(
                "artifact.propose denied at gate: {decision:?}"
            )));
        }
        None
    };

    if let Some(receipt) = receipt {
        #[cfg(test)]
        apply_dispatch_test_mutation(state, &receipt);
        verify_proposal_receipt(state, &receipt)?;
        let review = build_miner_owner_review(state, &proposal, &receipt)?;
        let now = Timestamp::now();
        let rendered = persist_owner_review(
            state,
            &review,
            state.owner.principal_id.as_ulid(),
            now + jiff::SignedDuration::from_secs(review.limits.expires_after_secs),
            now,
            None,
        )
        .map_err(|error| {
            MinerRuntimeError::Dispatch(format!("owner review persistence: {error}"))
        })?;
        if matches!(owner_surface.kind(), OwnerSurfaceKind::TelegramPrivate) {
            crate::spend::guard_connector(state, true)
                .await
                .map_err(|error| {
                    MinerRuntimeError::Dispatch(format!("connector guard: {error}"))
                })?;
            state
                .connectors
                .telegram()
                .send_owner_review(
                    owner_surface,
                    &rendered.text,
                    rendered.review_id,
                    &rendered.binding_digest,
                    &rendered.intents,
                )
                .await
                .map_err(|error| {
                    MinerRuntimeError::Dispatch(format!("owner review notify: {error}"))
                })?;
        }
        // The review is now the only owner-facing approval surface for this
        // miner proposal. In particular, the core was asked not to send the
        // generic artifact.activate button.
        state
            .store
            .append_audit(
                "reflection.miner.owner_review_created",
                Some(&ActionId::new("artifact.propose")),
                None,
                Some(&review.id.to_string()),
                Some(grant.grant_id),
                &[],
                &[],
            )
            .map_err(MinerRuntimeError::Store)?;
    }
    Ok(())
}

async fn dispatch_miner_evaluated_proposal(
    state: &AppState,
    grant: &TaskGrant,
    owner_surface: &OwnerSurfaceRef,
    payload: &serde_json::Value,
) -> Result<crate::api::artifact_propose::ArtifactProposalReceipt, MinerRuntimeError> {
    let now = Timestamp::now();
    if !crate::spend::admit_spend(state, crate::spend::SpendLane::from_grant(grant), now)
        .await
        .map_err(|error| MinerRuntimeError::Dispatch(format!("spend admission: {error}")))?
    {
        return Err(MinerRuntimeError::Dispatch(
            "daily spend cap exceeded".to_string(),
        ));
    }
    let payload_ref = state
        .artifacts
        .put(canonical_json(payload).as_bytes())
        .map_err(|error| MinerRuntimeError::Dispatch(format!("payload artifact: {error}")))?;
    let action = ActionId::new("artifact.propose");
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: action.clone(),
        target_ref: None,
        payload_ref: Some(payload_ref.clone()),
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
    state
        .store
        .append_audit(
            "action.gated",
            Some(&action),
            Some(&outcome.decision),
            None,
            Some(grant.id),
            &[],
            std::slice::from_ref(&payload_ref),
        )
        .map_err(MinerRuntimeError::Store)?;
    if outcome.decision != GateDecision::Allow {
        return Err(MinerRuntimeError::Dispatch(format!(
            "artifact.propose denied at gate: {:?}",
            outcome.decision
        )));
    }
    dispatch_artifact_propose_core(state, grant, &action, owner_surface, Some(payload), false)
        .await
        .map_err(|error| MinerRuntimeError::Dispatch(format!("{error:?}")))
}

fn verify_proposal_receipt(
    state: &AppState,
    receipt: &crate::api::artifact_propose::ArtifactProposalReceipt,
) -> Result<(), MinerRuntimeError> {
    let row = state
        .store
        .find_proposed_artifact(
            &receipt.artifact_kind,
            &receipt.artifact_id,
            receipt.artifact_version,
        )
        .map_err(MinerRuntimeError::Store)?
        .ok_or_else(|| {
            MinerRuntimeError::Dispatch("proposal receipt has no persisted artifact".to_string())
        })?;
    if row.id != receipt.proposal_id
        || row.state != openspine_schemas::artifact::Lifecycle::ReviewRequired
        || row.yaml_digest != receipt.artifact_digest.to_string()
        || row.action_request_id != Some(receipt.action_request_id)
    {
        return Err(MinerRuntimeError::Dispatch(
            "proposal receipt does not match persisted artifact".to_string(),
        ));
    }
    let verdicts = state
        .store
        .eval_verdicts_for_artifact(
            &receipt.artifact_kind,
            &receipt.artifact_id,
            receipt.artifact_version,
        )
        .map_err(MinerRuntimeError::Store)?;
    let replay = verdicts
        .iter()
        .find(|verdict| verdict.id == receipt.replay_verdict_id)
        .ok_or_else(|| {
            MinerRuntimeError::Dispatch("replay verdict is not persisted".to_string())
        })?;
    let judge = verdicts
        .iter()
        .find(|verdict| verdict.id == receipt.judge_verdict_id)
        .ok_or_else(|| MinerRuntimeError::Dispatch("judge verdict is not persisted".to_string()))?;
    if replay.verdict != "pass" || judge.verdict != "pass" {
        return Err(MinerRuntimeError::Dispatch(
            "proposal receipt evaluation verdicts must both pass".to_string(),
        ));
    }
    if replay.artifact_digest != receipt.artifact_digest.to_string()
        || judge.artifact_digest != receipt.artifact_digest.to_string()
        || replay.epochs != receipt.epochs
        || judge.epochs != receipt.epochs
    {
        return Err(MinerRuntimeError::Dispatch(
            "proposal receipt eval identity mismatch".to_string(),
        ));
    }
    Ok(())
}

fn build_miner_owner_review(
    state: &AppState,
    proposal: &ReflectionProposal,
    receipt: &crate::api::artifact_propose::ArtifactProposalReceipt,
) -> Result<OwnerReviewRequest, MinerRuntimeError> {
    let evidence = proposal
        .evidence
        .clone()
        .ok_or(MinerRuntimeError::Payload)?;
    let payload = proposal
        .to_proposal_payload()
        .map_err(|_| MinerRuntimeError::Payload)?;
    let yaml = payload
        .get("yaml")
        .and_then(serde_json::Value::as_str)
        .ok_or(MinerRuntimeError::Payload)?;
    let manifest =
        match parse_proposal("standing_rule", yaml).map_err(|_| MinerRuntimeError::Payload)? {
            ParsedProposal::StandingRule(manifest) => manifest,
            _ => return Err(MinerRuntimeError::Payload),
        };
    let binding = manifest.reviewed_scope.ok_or(MinerRuntimeError::Payload)?;
    let action = manifest.action_id.clone();
    let scope = binding.scope.clone();
    let fallback = ReviewFallbackBehavior {
        scope_mismatch: BoundaryBehavior::RequireApproval,
        compatibility_drift: BoundaryBehavior::RequireApproval,
        budget_exhaustion: BoundaryBehavior::RequireApproval,
        timeout: BoundaryBehavior::Deny,
    };
    let epochs = owner_review_epochs(&receipt.epochs)?;
    let descriptor = state
        .action_catalog
        .delegation_descriptor_for(&action)
        .ok_or_else(|| MinerRuntimeError::Dispatch(format!("unknown action {action}")))?;
    let policy = descriptor
        .delegation_policy
        .as_ref()
        .ok_or_else(|| MinerRuntimeError::Dispatch(format!("action {action} is not delegable")))?;
    let input = OwnerReviewRequestInput {
        id: Ulid::new(),
        schema_version: 1,
        review_version: 1,
        proposal_kind: ProposalKind::Responsibility,
        evidence,
        title: format!("Delegate {action}"),
        description: manifest.description,
        reviewed_scope: scope,
        automatic_effects: vec![
            format!("Execute {action} only within the reviewed scope"),
            "Retain owner approval and bounded budget controls".to_string(),
        ],
        remaining_boundaries: vec![
            "Final send remains owner-controlled".to_string(),
            "Scope mismatch falls back to ordinary approval".to_string(),
        ],
        limits: ReviewLimits {
            quota: manifest.quota,
            rate: manifest.rate,
            expires_after_secs: manifest.expires_after_secs,
        },
        fallback_behavior: fallback,
        proposal_digest: receipt.artifact_digest.clone(),
        compatibility_digest: binding.compatibility_digest.clone(),
        available_decisions: std::collections::BTreeSet::from([
            OwnerReviewDecision::Approve,
            OwnerReviewDecision::Reject,
            OwnerReviewDecision::Narrow,
        ]),
        lifecycle_controls: std::collections::BTreeSet::from([
            ResponsibilityLifecycleControl::Pause,
            ResponsibilityLifecycleControl::Resume,
            ResponsibilityLifecycleControl::Revoke,
        ]),
        evaluation_binding: Some(OwnerReviewEvaluationBinding {
            artifact_kind: receipt.artifact_kind.clone(),
            artifact_id: receipt.artifact_id.clone(),
            artifact_version: receipt.artifact_version,
            proposal_digest: receipt.artifact_digest.clone(),
            action_request_id: receipt.action_request_id,
            replay_verdict_id: receipt.replay_verdict_id,
            judge_verdict_id: receipt.judge_verdict_id,
            epochs,
        }),
    };
    OwnerReviewRequest::try_new(input, policy)
        .map_err(|error| MinerRuntimeError::Dispatch(format!("owner review schema: {error}")))
}

fn owner_review_epochs(
    epochs: &crate::store::eval_verdict_store::VerdictEpochs,
) -> Result<OwnerReviewEvaluationEpochs, MinerRuntimeError> {
    fn digest(value: &Option<String>) -> Result<Option<Digest>, MinerRuntimeError> {
        value
            .as_deref()
            .map(Digest::parse)
            .transpose()
            .map_err(|_| MinerRuntimeError::Payload)
    }
    Ok(OwnerReviewEvaluationEpochs {
        proposal_digest: digest(&epochs.proposal_digest)?,
        compatibility_digest: digest(&epochs.compatibility_digest)?,
        reviewed_scope_digest: digest(&epochs.reviewed_scope_digest)?,
        evidence_set_digest: digest(&epochs.evidence_set_digest)?,
        descriptor_version: epochs.descriptor_version,
        implementation_version: epochs.implementation_version,
        policy_version: epochs.policy_version,
    })
}

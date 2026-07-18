//! `artifact.propose` dispatch (PRD §13 / 5c): validate a declarative
//! artifact the chat proposed, persist it as `proposed`, and ask the owner
//! to approve activating it via a digest-bound `artifact.activate` request.
//!
//! Mirrors `propose_draft_creation`'s shape (D-043): budget → store payload →
//! persist a pending `ActionRequest` → send an approval button. The approval
//! binds the *exact* YAML bytes the dispatcher stored (D-011), never a value
//! re-supplied at activation time; the `target_digest` additionally binds
//! `{kind, artifact_id, version}` so a swap of *which* artifact activates is
//! caught even if the YAML coincidentally re-hashes.

use super::actions::DispatchError;
use super::connector_breaker::call_with_connector;
use jiff::Timestamp;
use openspine_schemas::action::{ActionId, ActionRequest};
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::digest::digest_of;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::lineage::ArtifactLineage;
use serde::Deserialize;
use serde_json::{json, Value};
use ulid::Ulid;

use crate::artifact_loader::{find_kind_spec, is_proposable_kind, parse_proposal, ParsedProposal};
use crate::model_swap::enrich;
use crate::overlay_eval_gate::{run_gate, run_model_swap_gate};
use crate::pipeline::AppState;
use crate::store::proposed_artifacts::ProposedArtifact;

/// The wire-contract-mandated shape of `artifact.propose`'s payload.
/// `deny_unknown_fields` matters beyond style: it forces the shell to send
/// exactly `{kind, yaml}`, so a future field can never sneak through as a
/// silent dropped value (same rationale as `TelegramReplyPayload`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactProposePayload {
    kind: String,
    yaml: String,
}

pub(super) async fn dispatch_artifact_propose(
    state: &AppState,
    grant: &TaskGrant,
    action: &ActionId,
    bound_chat_id: i64,
    payload: Option<&Value>,
) -> Result<Value, DispatchError> {
    // 1. Payload contract.
    let payload = payload.ok_or_else(|| {
        DispatchError::BadRequest("artifact.propose requires a payload".to_string())
    })?;
    let req: ArtifactProposePayload = serde_json::from_value(payload.clone()).map_err(|_| {
        DispatchError::BadRequest(
            "artifact.propose payload must be exactly {\"kind\": string, \"yaml\": string}"
                .to_string(),
        )
    })?;
    if !is_proposable_kind(&req.kind) {
        return Err(DispatchError::BadRequest(
            "artifact.propose kind must be one of route|agent|workflow|pack|policy|model_swap"
                .to_string(),
        ));
    }

    let mut parsed = parse_proposal(&req.kind, &req.yaml).map_err(|err| {
        DispatchError::BadRequest(format!("artifact.propose yaml failed to parse: {err}"))
    })?;
    if parsed.lifecycle_state() != Lifecycle::Proposed {
        return Err(DispatchError::BadRequest(
            "artifact.propose yaml lifecycle_state must be proposed; the proposer cannot pre-activate"
                .to_string(),
        ));
    }
    if let ParsedProposal::ModelSwap(swap) = &parsed {
        if !swap.identity_valid() {
            return Err(DispatchError::BadRequest(
                "model_swap id must equal role name".to_string(),
            ));
        }
        if swap.golden_set_result.is_some() {
            return Err(DispatchError::BadRequest(
                "model_swap golden_set_result is kernel-generated and cannot be supplied"
                    .to_string(),
            ));
        }
        if !state.provider_pool.contains_key(&swap.target_provider_id) {
            return Err(DispatchError::BadRequest(format!(
                "unknown configured provider {}",
                swap.target_provider_id
            )));
        }
        let golden_set = state
            .registry
            .read()
            .golden_sets
            .get(&swap.golden_set_id)
            .cloned();
        let Some(golden_set) = golden_set else {
            return Err(DispatchError::BadRequest(format!(
                "unknown trusted golden set {}",
                swap.golden_set_id
            )));
        };
        if !golden_set.roles.contains(&swap.role) {
            return Err(DispatchError::BadRequest(
                "trusted golden set is not authorized for this model role".to_string(),
            ));
        }
    }
    let kind = parsed.kind().to_string();
    let artifact_id = parsed.artifact_id().to_string();
    let version = parsed.version();
    if state
        .base_artifact_ids
        .contains(&(kind.clone(), artifact_id.clone()))
    {
        return Err(DispatchError::BadRequest(
            "artifact id is owned by the base namespace; choose a new id".to_string(),
        ));
    }

    // 3. Reject duplicates across the live registry and pending proposals
    //    (D-028 monotonic versions). Read guard held only for the scan.
    let exists_in_registry = {
        let registry = state.registry.read();
        let spec = find_kind_spec(&kind).expect("kind already validated against the table above");
        (spec.duplicate_exists)(&registry, &artifact_id, version)
    };
    let exists_in_proposals = state
        .store
        .proposed_artifact_exists(&kind, &artifact_id, version)
        .map_err(|err| DispatchError::Resource(err.into()))?;
    let current_version = {
        let registry = state.registry.read();
        crate::artifact_loader::artifact_version(&registry, &kind, &artifact_id)
    };
    let pending_version = state
        .store
        .highest_proposed_version(&kind, &artifact_id)
        .map_err(|err| DispatchError::Resource(err.into()))?;
    if current_version.is_some_and(|current| version < current)
        || pending_version.is_some_and(|pending| version < pending)
    {
        return Err(DispatchError::BadRequest(
            "artifact version is lower than the active or pending version".to_string(),
        ));
    }
    if exists_in_registry || exists_in_proposals {
        return Err(DispatchError::BadRequest(
            "artifact id/version already exists; bump version".to_string(),
        ));
    }
    if let ParsedProposal::ModelSwap(swap) = &parsed {
        let current_version = state
            .registry
            .read()
            .model_swaps
            .get(&swap.id)
            .map(|current| current.version);
        if current_version.is_some_and(|current| version <= current) {
            return Err(DispatchError::BadRequest(format!(
                "model_swap version {version} is not newer than active version {}",
                current_version.unwrap_or_default()
            )));
        }
        let (golden_set, provider, provider_digest) = {
            let golden_set = state
                .registry
                .read()
                .golden_sets
                .get(&swap.golden_set_id)
                .cloned()
                .ok_or_else(|| {
                    DispatchError::BadRequest(format!(
                        "unknown trusted golden set {}",
                        swap.golden_set_id
                    ))
                })?;
            let provider = state
                .provider_pool
                .get(&swap.target_provider_id)
                .cloned()
                .ok_or_else(|| {
                    DispatchError::BadRequest(format!(
                        "unknown configured provider {}",
                        swap.target_provider_id
                    ))
                })?;
            let provider_digest = state
                .provider_config_digests
                .get(&swap.target_provider_id)
                .cloned()
                .ok_or_else(|| {
                    DispatchError::BadRequest("missing provider configuration digest".to_string())
                })?;
            (golden_set, provider, provider_digest)
        };
        let count = u32::try_from(golden_set.cases.len()).map_err(|_| {
            DispatchError::BadRequest("golden set case count exceeds budget type".to_string())
        })?;
        if !state
            .store
            .try_count_model_calls(grant.id, count, grant.limits.max_model_calls)
            .map_err(|err| DispatchError::Resource(err.into()))?
        {
            return Err(DispatchError::BadRequest(
                "model-swap golden-set run exceeds this task's model-call budget".to_string(),
            ));
        }
        let enriched = enrich(
            state,
            swap,
            &golden_set,
            &provider,
            &provider_digest,
            grant.expires_at,
        )
        .await
        .map_err(|err| DispatchError::BadRequest(err.to_string()))?;
        parsed = ParsedProposal::ModelSwap(enriched);
    }
    let effective_yaml = if matches!(parsed, ParsedProposal::ModelSwap(_)) {
        parsed
            .to_yaml()
            .map_err(|err| DispatchError::Resource(err.into()))?
    } else {
        req.yaml.clone()
    };

    // 4. AD-142 overlay eval gate — run BEFORE any persisted side effect
    //    (budget, row insert, audit) so a denied proposal leaves nothing
    //    stranded: the (kind, id, version) stays re-proposable, the owner
    //    is not silently debited, and no validated row lingers. The digest
    //    is derived from the exact YAML bytes the owner supplied; the later
    //    `state.artifacts.put` is content-addressed, so it yields the same
    //    digest (D-011 binding preserved across the boundary).
    let proposal_digest = openspine_schemas::digest::digest_of_bytes(effective_yaml.as_bytes());
    let eval = if matches!(parsed, ParsedProposal::ModelSwap(_)) {
        run_model_swap_gate(
            &state.store,
            &state.action_catalog,
            &parsed,
            &proposal_digest,
        )
    } else {
        run_gate(
            &state.store,
            &state.action_catalog,
            &parsed,
            &proposal_digest,
        )
    }
    .map_err(|err| DispatchError::BadRequest(err.to_string()))?;

    // 5. Budget (D-046): a shell-initiated artifact put.
    if !state
        .store
        .try_count_artifact_put(grant.id, grant.limits.max_artifacts)
        .map_err(|err| DispatchError::Resource(err.into()))?
    {
        return Err(DispatchError::BadRequest(
            "artifact.propose budget exhausted for this task".to_string(),
        ));
    }

    // 6. Persist the reviewed YAML and the proposed row; advance
    //    proposed → validated (parse succeeded) and audit.
    let yaml_ref = state
        .artifacts
        .put(effective_yaml.as_bytes())
        .map_err(|err| DispatchError::Resource(err.into()))?;
    let proposal_id = Ulid::new();
    // Pre-generated so the row links to the approval request in one insert
    // (no separate setter), matching propose_draft_creation's atomicity.
    let action_request_id = Ulid::new();
    let now = Timestamp::now();
    state
        .store
        .insert_proposed_artifact(&ProposedArtifact {
            id: proposal_id,
            kind: kind.clone(),
            artifact_id: artifact_id.clone(),
            version,
            state: Lifecycle::Proposed,
            yaml_digest: yaml_ref.digest.as_str().to_string(),
            task_grant_id: grant.id,
            action_request_id: Some(action_request_id),
            proposed_at: now,
            // Fresh proposals are root artifacts (no derivation). Explicit
            // Some(root) — never leave None (None = unknown legacy only).
            lineage: Some(ArtifactLineage::root()),
        })
        .map_err(|err| DispatchError::Resource(err.into()))?;
    state
        .store
        .set_proposed_artifact_state(proposal_id, Lifecycle::Proposed, Lifecycle::Validated)
        .map_err(|err| DispatchError::Resource(err.into()))?;
    state
        .store
        .append_audit(
            "artifact.proposed",
            Some(&ActionId::new("artifact.propose")),
            None,
            None,
            Some(grant.id),
            &[],
            std::slice::from_ref(&yaml_ref),
        )
        .map_err(|err| DispatchError::Resource(err.into()))?;

    // 7. Atomically persist both digest-bound eval verdicts and advance
    //    validated → review_required. Only after this succeeds is the
    //    digest-bound `artifact.activate` request persisted, so a failed
    //    promotion leaves no orphan action_request the owner could tap.
    state
        .store
        .promote_authority_bearing_proposal(proposal_id, eval.replay, eval.judge)
        .map_err(|err| DispatchError::Resource(err.into()))?;
    let target_digest = digest_of(&json!({
        "kind": kind,
        "artifact_id": artifact_id,
        "version": version,
    }));
    let request = ActionRequest {
        id: action_request_id,
        task_grant_id: grant.id,
        action: ActionId::new("artifact.activate"),
        target_ref: None,
        payload_ref: Some(yaml_ref.clone()),
        target_digest: Some(target_digest),
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        skill_attribution: None,
        requested_at: now,
        schema_version: 1,
    };
    state
        .store
        .insert_action_request(&request)
        .map_err(|err| DispatchError::Resource(err.into()))?;
    let summary = format!(
        "Artifact proposal\nKind: {kind}\nId: {artifact_id} v{version}\nDigest: {digest}\n\n{}\n\nApprove to activate.",
        eval.summary,
        digest = yaml_ref.digest
    );
    crate::spend::guard_connector_for(state, grant)
        .await
        .map_err(DispatchError::Resource)?;
    // AD-103/AD-141: admit + bound-timeout the Telegram send at the call
    // site; the helper records breaker health and the D-069 counter.
    call_with_connector(
        state,
        "telegram",
        action,
        grant,
        state.connectors.telegram().send_reply_with_approval_button(
            bound_chat_id,
            &summary,
            action_request_id,
        ),
    )
    .await?;
    Ok(json!({
        "proposed": true,
        "action_request_id": action_request_id.to_string(),
    }))
}

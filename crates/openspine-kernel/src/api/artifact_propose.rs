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

use jiff::Timestamp;
use openspine_schemas::action::{ActionId, ActionRequest};
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::digest::digest_of;
use openspine_schemas::grant::TaskGrant;
use serde::Deserialize;
use serde_json::{json, Value};
use ulid::Ulid;

use super::actions::DispatchError;
use crate::artifact_loader::parse_proposal;
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

/// The five kinds a chat may propose at runtime. Prompt templates are
/// deliberately absent (D-048): a template changes the model's instruction
/// surface, so letting chat propose one is an injection-escalation channel.
const PROPOSABLE_KINDS: &[&str] = &["route", "agent", "workflow", "pack", "policy"];

pub(super) async fn dispatch_artifact_propose(
    state: &AppState,
    grant: &TaskGrant,
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
    if !PROPOSABLE_KINDS.contains(&req.kind.as_str()) {
        return Err(DispatchError::BadRequest(
            "artifact.propose kind must be one of route|agent|workflow|pack|policy".to_string(),
        ));
    }

    // 2. Budget (D-046): a shell-initiated artifact put.
    if !state
        .store
        .try_count_artifact_put(grant.id, grant.limits.max_artifacts)
        .map_err(|err| DispatchError::Internal(err.into()))?
    {
        return Err(DispatchError::BadRequest(
            "artifact.propose budget exhausted for this task".to_string(),
        ));
    }

    // 3. Parse per kind; require lifecycle_state == proposed (the proposer
    //    can never pre-activate). Extract (id, version) for the dup check.
    let parsed = parse_proposal(&req.kind, &req.yaml).map_err(|err| {
        DispatchError::BadRequest(format!("artifact.propose yaml failed to parse: {err}"))
    })?;
    if parsed.lifecycle_state() != Lifecycle::Proposed {
        return Err(DispatchError::BadRequest(
            "artifact.propose yaml lifecycle_state must be proposed; the proposer cannot pre-activate"
                .to_string(),
        ));
    }
    let kind = parsed.kind().to_string();
    let artifact_id = parsed.artifact_id().to_string();
    let version = parsed.version();

    // 4. Reject duplicates across the live registry and pending proposals
    //    (D-028 monotonic versions). Read guard held only for the scan.
    let exists_in_registry = {
        let registry = state.registry.read();
        match kind.as_str() {
            "route" => registry
                .routes
                .iter()
                .any(|r| r.id == artifact_id && r.version == version),
            "agent" => registry
                .agents
                .get(&artifact_id)
                .is_some_and(|a| a.version == version),
            "workflow" => registry
                .workflows
                .get(&artifact_id)
                .is_some_and(|w| w.version == version),
            "pack" => registry
                .packs
                .get(&artifact_id)
                .is_some_and(|p| p.version == version),
            "policy" => registry
                .policies
                .get(&artifact_id)
                .is_some_and(|p| p.version == version),
            // Unreachable: kind validated against PROPOSABLE_KINDS above.
            _ => false,
        }
    };
    let exists_in_proposals = state
        .store
        .proposed_artifact_exists(&kind, &artifact_id, version)
        .map_err(|err| DispatchError::Internal(err.into()))?;
    if exists_in_registry || exists_in_proposals {
        return Err(DispatchError::BadRequest(
            "artifact id/version already exists; bump version".to_string(),
        ));
    }

    // 5. Persist the reviewed YAML and the proposed row; advance
    //    proposed → validated (parse succeeded) and audit.
    let yaml_ref = state
        .artifacts
        .put(req.yaml.as_bytes())
        .map_err(|err| DispatchError::Internal(err.into()))?;
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
        })
        .map_err(|err| DispatchError::Internal(err.into()))?;
    state
        .store
        .set_proposed_artifact_state(proposal_id, Lifecycle::Proposed, Lifecycle::Validated)
        .map_err(|err| DispatchError::Internal(err.into()))?;
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
        .map_err(|err| DispatchError::Internal(err.into()))?;

    // 6. Persist the digest-bound `artifact.activate` request, advance
    //    validated → review_required, and send the owner an approval button
    //    carrying only kernel-authored summary text.
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
        requested_at: now,
        schema_version: 1,
    };
    state
        .store
        .insert_action_request(&request)
        .map_err(|err| DispatchError::Internal(err.into()))?;
    state
        .store
        .set_proposed_artifact_state(proposal_id, Lifecycle::Validated, Lifecycle::ReviewRequired)
        .map_err(|err| DispatchError::Internal(err.into()))?;
    let summary = format!(
        "Artifact proposal\nKind: {kind}\nId: {artifact_id} v{version}\nDigest: {digest}\n\nApprove to activate.",
        digest = yaml_ref.digest
    );
    state
        .telegram
        .send_reply_with_approval_button(bound_chat_id, &summary, action_request_id)
        .await
        .map_err(DispatchError::Internal)?;
    Ok(json!({
        "proposed": true,
        "action_request_id": action_request_id.to_string(),
    }))
}

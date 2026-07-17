//! Metadata-only owner secret mode and direct next-message capture (D-014).

use jiff::Timestamp;
use openspine_gate::{gate, ActionOrigin};
use openspine_schemas::action::{ActionId, ActionRequest, GateDecision};
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::digest::{digest_of_bytes, Digest};
use openspine_schemas::event::{TargetRef, TargetRefKind};
use openspine_schemas::grant::{GrantLimits, GrantMode, TaskGrant};
use serde::{Deserialize, Serialize};

use crate::pipeline::AppState;
use crate::secret_store::SecretStore;
use ulid::Ulid;

const PENDING_KEY: &str = "secret.intake.pending";
const PENDING_TTL_SECONDS: i64 = 300;
const STAGE_TTL_SECONDS: i64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StageMeta {
    correlation_id: Ulid,
    grant_id: Ulid,
    mode: SecretMode,
    counterpart_slot: String,
    staged_at: Timestamp,
    expires_at: Timestamp,
}

fn rollback_secret(
    secrets: &SecretStore,
    slot: &str,
    previous: Option<Vec<u8>>,
) -> anyhow::Result<()> {
    match previous {
        Some(value) => secrets
            .put(slot, &value)
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("secret rollback failed: {e}")),
        None => secrets
            .delete(slot)
            .map_err(|e| anyhow::anyhow!("secret rollback failed: {e}")),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretMode {
    Intake,
    Rotate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Pending {
    slot: String,
    mode: SecretMode,
    chat_id: i64,
    target_digest: Digest,
    grant_id: Ulid,
    action_request_id: Ulid,
    requested_at: Timestamp,
    expires_at: Timestamp,
    #[serde(default)]
    stage_correlation_id: Option<Ulid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureOutcome {
    Stored(SecretMode),
    Staged(SecretMode),
    Rejected,
}

pub fn parse_command(text: &str) -> Option<(SecretMode, &str)> {
    let rest = text.trim().strip_prefix("/secret")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let mut parts = rest.split_whitespace();
    let mode = match parts.next()? {
        "intake" => SecretMode::Intake,
        "rotate" => SecretMode::Rotate,
        _ => return None,
    };
    let slot = parts.next()?;
    if parts.next().is_some() || !SecretStore::validate_slot(slot) {
        return None;
    }
    Some((mode, slot))
}

fn action_for(mode: SecretMode) -> ActionId {
    ActionId::new(match mode {
        SecretMode::Intake => "secret.intake",
        SecretMode::Rotate => "secret.rotate",
    })
}

fn kind_prefix(mode: SecretMode) -> &'static str {
    match mode {
        SecretMode::Intake => "secret.intake",
        SecretMode::Rotate => "secret.rotate",
    }
}
fn owner_grant(
    action: &ActionId,
    now: Timestamp,
    owner_principal_id: Ulid,
    _proof: &crate::telegram::VerifiedOwnerContext,
) -> Option<TaskGrant> {
    let key = crate::grant_hmac_key()?;
    let mut grant = TaskGrant {
        id: Ulid::new(),
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user: owner_principal_id.to_string(),
        purpose: "secret-intake".to_string(),
        issued_by: "owner-control".to_string(),
        issued_at: now,
        expires_at: now + std::time::Duration::from_secs(PENDING_TTL_SECONDS as u64),
        event_id: Ulid::new(),
        route_id: "secret-intake".to_string(),
        agent_id: "kernel".to_string(),
        workflow_id: "secret-intake".to_string(),
        capability_pack_id: "owner".to_string(),
        authority_sources: vec!["verified-owner".to_string()],
        thread_id: None,
        selection_tokens: vec![],
        allowed_actions: vec![action.clone()],
        approval_required_actions: vec![],
        denied_actions: vec![],
        allowed_egress_classes: vec![],
        output_channels: vec!["telegram-owner".to_string()],
        limits: GrantLimits {
            max_model_calls: 0,
            max_artifacts: 0,
            max_runtime_seconds: PENDING_TTL_SECONDS as u64,
        },
        task_token: String::new(),
        root_grant_id: Ulid::nil(),
        parent_grant_id: None,
        mode: GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
    };
    grant.seal_root(&key);
    Some(grant)
}

/// Gate a metadata-only mode request and persist its bound, expiring pending slot.
pub fn arm(
    state: &AppState,
    chat_id: i64,
    owner_principal_id: Ulid,
    proof: &crate::telegram::VerifiedOwnerContext,
    mode: SecretMode,
    slot: &str,
) -> anyhow::Result<bool> {
    let now = Timestamp::now();
    let action = action_for(mode);
    let Some(grant) = owner_grant(&action, now, owner_principal_id, proof) else {
        state.store.append_audit(
            &format!("{}.denied", kind_prefix(mode)),
            Some(&action),
            None,
            Some("grant HMAC key unavailable"),
            None,
            &[],
            &[],
        )?;
        return Ok(false);
    };
    let target_digest = digest_of_bytes(slot.as_bytes());
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: action.clone(),
        target_ref: Some(TargetRef {
            kind: TargetRefKind::SecretSlot,
            id: Some(slot.to_string()),
        }),
        payload_ref: None,
        target_digest: Some(target_digest.clone()),
        selection_token_id: None,
        requested_at: now,
        schema_version: 1,
    };
    let outcome = gate(
        &grant,
        &request,
        ActionOrigin::Shell,
        &state.store,
        &state.action_catalog,
        &state.connectors,
        now,
    );
    state.store.append_audit(
        &format!("{}.gate", kind_prefix(mode)),
        Some(&action),
        Some(&outcome.decision),
        Some(&format!("slot={slot}; action_request_id={}", request.id)),
        Some(grant.id),
        &[],
        &[],
    )?;
    if !matches!(outcome.decision, GateDecision::Allow) {
        return Ok(false);
    }
    let stage_correlation_id = match slot {
        "gmail.client_secret" => state
            .store
            .get_kv("secret.stage.gmail.refresh_token")?
            .and_then(|raw| serde_json::from_str::<StageMeta>(&raw).ok())
            .map(|meta| meta.correlation_id),
        "gmail.refresh_token" => state
            .store
            .get_kv("secret.stage.gmail.client_secret")?
            .and_then(|raw| serde_json::from_str::<StageMeta>(&raw).ok())
            .map(|meta| meta.correlation_id),
        _ => None,
    };
    let pending = Pending {
        slot: slot.to_string(),
        mode,
        chat_id,
        grant_id: grant.id,
        action_request_id: request.id,
        requested_at: now,
        expires_at: now + std::time::Duration::from_secs(PENDING_TTL_SECONDS as u64),
        target_digest: target_digest.clone(),
        stage_correlation_id,
    };
    state
        .store
        .set_kv(PENDING_KEY, &serde_json::to_string(&pending)?)?;
    if let Err(err) = state.store.append_audit(
        &format!("{}.armed", kind_prefix(mode)),
        Some(&action),
        Some(&GateDecision::Allow),
        Some(&format!("slot={slot}; action_request_id={}", request.id)),
        Some(grant.id),
        &[],
        &[],
    ) {
        let _ = state.store.delete_kv(PENDING_KEY);
        return Err(err.into());
    }
    Ok(true)
}

mod capture;
pub use capture::capture;

#[cfg(test)]
mod rollback_tests;
#[cfg(test)]
mod tests;

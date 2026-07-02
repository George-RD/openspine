//! The owner-message pipeline (build plan 4a/4b/4c wiring): Telegram update
//! -> owner verification -> identity resolution -> route resolution ->
//! containment guard -> authority composition -> task grant -> sandboxed
//! shell spawn. Every step that terminates the pipeline early is audited,
//! so "why didn't Lyra reply" is always answerable from `audit_log` alone.
//!
//! Phase 1 has exactly one live identity source: the configured Telegram
//! owner. [`resolve_owner_identity`] is a hardcoded match, not a real
//! identity graph lookup — [`crate::telegram::verify_update`] already
//! filtered every event reaching this module down to "owner, private chat,
//! text message" before an [`AppState`] method ever sees it, so by
//! construction the identity here IS the owner. A persisted multi-identity
//! graph is future work (a second real identity source), not fabricated
//! ahead of one.

use jiff::Timestamp;
use openspine_authority::{compose_authority, resolve_route, AuthorityInput, AuthorityOutcome};
use openspine_schemas::event::{ChannelTrust, EventEnvelope};
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::identity::{IdentityResolution, MatchedIdentifierType, RelationshipKind};
use openspine_schemas::policy::{Constraints, SessionPolicy};
use openspine_schemas::route::RouteResolution;

use crate::artifact_loader::ArtifactRegistry;
use crate::artifact_store::ArtifactStore;
use crate::sandbox::{self, Sandbox};
use crate::store::Store;
use crate::telegram::{self, TelegramConnector, VerifiedUpdate};

/// Everything the pipeline needs to turn one Telegram update into an
/// audited, sandboxed task. Built once at kernel startup and shared
/// (read-only except for its own interior-mutable pieces) across the
/// Telegram poll loop and the axum HTTP layer.
pub struct AppState {
    pub store: Store,
    pub artifacts: ArtifactStore,
    pub registry: ArtifactRegistry,
    pub sandbox: Sandbox,
    pub telegram: TelegramConnector,
    pub owner_user_id: i64,
    /// e.g. `http://127.0.0.1:7777` — passed to the shell as `KERNEL_ENDPOINT`.
    pub kernel_endpoint: String,
    /// D-025 / PRD §16 escape hatch. See [`sandbox::refuses_external_communication_without_containment`].
    pub unsafe_allow_uncontained_private_data: bool,
    /// The single configured model provider (build plan 4c: "one provider
    /// call", not real multi-provider routing — that is
    /// `implement-model-gateway`'s deferred scope).
    pub provider: crate::model_gateway::ProviderClient,
    /// Backs `GET /v1/status`'s `uptime_seconds`.
    pub started_at: std::time::Instant,
}

/// Phase 1 has no persisted per-user/session policy system yet (D-013's
/// "dynamic behavior should be easy" is served by the artifact registry, not
/// a session-policy store that doesn't exist). An empty session policy
/// narrows nothing — see `compose_authority`'s documented interpretation of
/// design.md's merge rule.
fn empty_session_policy() -> SessionPolicy {
    SessionPolicy {
        schema_version: 1,
        candidate_allowed_actions: vec![],
        approval_required: vec![],
        denied_actions: vec![],
        constraints: Constraints::default(),
    }
}

/// PRD §5.4: identity resolution is one *input* to authority, never a
/// grant of authority itself (D-006) — but by the time this runs, the
/// Telegram connector has already verified sender id + private chat, so
/// confidence is 1.0 and `source_verified` is `true` unconditionally.
fn resolve_owner_identity(envelope: &EventEnvelope) -> IdentityResolution {
    IdentityResolution {
        event_id: envelope.id,
        matched_identity_id: None,
        confidence: 1.0,
        matched_identifier_type: MatchedIdentifierType::TelegramUserId,
        channel_trust: ChannelTrust::VerifiedOwnerChannel,
        source_verified: true,
        authority_warning: None,
        schema_version: 1,
    }
}

/// Long-poll Telegram forever, dispatching every verified owner update
/// through [`handle_owner_update`]. Replay protection (design.md):
/// **at-most-once**, not at-least-once — `update_id` is persisted to
/// `kv_state` *before* the update is handed to the pipeline. For an
/// action-taking assistant a duplicate task grant (double shell spawn,
/// double reply, and in a future phase a double-sent email) is worse than
/// occasionally dropping a message the owner can just retype; a crash
/// between "offset persisted" and "handling finished" loses at most one
/// update rather than replaying an already-actioned one.
pub async fn run_telegram_poll_loop(state: &AppState) -> anyhow::Result<()> {
    const POLL_ERROR_BACKOFF: std::time::Duration = std::time::Duration::from_secs(5);
    loop {
        let last_update_id: Option<i64> = state
            .store
            .get_kv("last_telegram_update_id")?
            .and_then(|s| s.parse().ok());

        let updates = match state.telegram.poll_once(last_update_id).await {
            Ok(updates) => updates,
            Err(err) => {
                tracing::warn!(error = %err, "telegram poll_once failed, backing off");
                tokio::time::sleep(POLL_ERROR_BACKOFF).await;
                continue;
            }
        };

        for update in updates {
            if let Some(last) = last_update_id {
                if update.update_id <= last {
                    continue; // already processed before an earlier crash
                }
            }
            // Persist the offset *before* handling: see this function's
            // doc comment on the at-most-once tradeoff.
            state
                .store
                .set_kv("last_telegram_update_id", &update.update_id.to_string())?;
            if let Err(err) = handle_owner_update(state, &update).await {
                tracing::warn!(error = %err, update_id = update.update_id, "owner update handling failed");
            }
        }
    }
}

/// Run one verified-or-not Telegram update through the full pipeline.
/// Returns `Ok(None)` for every outcome the pipeline itself decides on
/// (ignored, denied, refused, ambiguous) — those are not errors, they are
/// the pipeline correctly declining to act. Returns `Ok(Some(grant))` once
/// authority has been composed and persisted, *regardless* of whether the
/// subsequent shell spawn succeeds (a spawn failure is audited as
/// `task.shell_failed`, not swallowed, but authority was already granted
/// and that fact must survive in the return value and the audit log
/// alike). Only a genuine infrastructure failure — store I/O or an
/// inconsistent registry — surfaces as `Err`.
pub async fn handle_owner_update(
    state: &AppState,
    update: &telegram::TelegramUpdate,
) -> anyhow::Result<Option<TaskGrant>> {
    let (chat_id, text) = match telegram::verify_update(update, state.owner_user_id) {
        VerifiedUpdate::OwnerMessage { chat_id, text } => (chat_id, text),
        VerifiedUpdate::Ignored { reason } => {
            state.store.append_audit(
                "telegram.update.ignored",
                None,
                None,
                Some(reason),
                None,
                &[],
                &[],
            )?;
            return Ok(None);
        }
    };

    let now = Timestamp::now();
    let raw_ref = state.artifacts.put(text.as_bytes())?;
    let envelope = telegram::build_owner_envelope(chat_id, raw_ref.clone(), now);
    state.store.append_audit(
        "event.received",
        None,
        None,
        None,
        None,
        &[],
        std::slice::from_ref(&raw_ref),
    )?;

    let identity = resolve_owner_identity(&envelope);
    let route_resolution = resolve_route(
        &envelope,
        &identity,
        Some(RelationshipKind::Owner),
        &state.registry.routes,
    );

    let route_id = match route_resolution {
        RouteResolution::Success { route_id } => route_id,
        RouteResolution::Denied { reason } => {
            state
                .store
                .append_audit("route.denied", None, None, Some(&reason), None, &[], &[])?;
            return Ok(None);
        }
        RouteResolution::Ambiguous { reason, .. } => {
            // Never grants widened authority on ambiguity (PRD §6.4) — falls
            // back to logged inaction, same as an explicit deny, for Phase 1.
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

    let route = state
        .registry
        .routes
        .iter()
        .find(|r| r.id == route_id)
        .ok_or_else(|| anyhow::anyhow!("resolved route {route_id} not found in registry"))?;

    // D-025 / O-003 / PRD §16: refuse before ever composing authority.
    if sandbox::refuses_external_communication_without_containment(
        envelope.lane,
        &state.sandbox,
        state.unsafe_allow_uncontained_private_data,
    ) {
        state.store.append_audit(
            "route.refused_uncontained",
            None,
            None,
            Some("external_communication lane requires a containing sandbox driver"),
            None,
            &[],
            &[],
        )?;
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

    let agent = state
        .registry
        .agents
        .get(agent_id)
        .ok_or_else(|| anyhow::anyhow!("agent {agent_id} not in registry"))?;
    let workflow = state
        .registry
        .workflows
        .get(workflow_id)
        .ok_or_else(|| anyhow::anyhow!("workflow {workflow_id} not in registry"))?;
    let pack = state
        .registry
        .packs
        .get(pack_id)
        .ok_or_else(|| anyhow::anyhow!("capability_pack {pack_id} not in registry"))?;
    let global_policy = state
        .registry
        .policies
        .get("global")
        .ok_or_else(|| anyhow::anyhow!("global policy not in registry"))?;
    let session = empty_session_policy();
    let user = state.owner_user_id.to_string();

    let input = AuthorityInput {
        event: &envelope,
        identity: &identity,
        route,
        global_policy,
        agent,
        workflow,
        pack,
        session: &session,
        user: &user,
        purpose: "owner_control_conversation",
    };

    let grant = match compose_authority(&input, now) {
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
            return Ok(None);
        }
        // `compose_authority` never produces this itself (see its doc
        // comment — reserved only for API symmetry with `RouteResolution`);
        // handled here purely to stay exhaustive against future variants.
        AuthorityOutcome::Ambiguous { .. } => {
            state.store.append_audit(
                "authority.ambiguous",
                None,
                None,
                Some("compose_authority returned Ambiguous, which it is not expected to produce"),
                None,
                &[],
                &[],
            )?;
            return Ok(None);
        }
    };

    state.store.insert_task_grant(&grant, &raw_ref, chat_id)?;
    state.store.append_audit(
        "authority.granted",
        None,
        None,
        None,
        Some(grant.id),
        &[],
        &[raw_ref],
    )?;

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
                Some(&err.to_string()),
                Some(grant.id),
                &[],
                &[],
            )?;
        }
    }

    Ok(Some(grant))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::fixtures::*;

    #[tokio::test]
    async fn non_owner_update_is_ignored_and_audited_without_a_grant() {
        let state = test_state();
        let mut update = owner_update("hi");
        update.sender_user_id = Some(999);
        handle_owner_update(&state, &update).await.unwrap();
        assert_eq!(state.store.count_task_grants().unwrap(), 0);
    }

    #[tokio::test]
    async fn owner_update_composes_authority_and_persists_a_grant_bound_to_the_chat() {
        let state = test_state();
        let update = owner_update("hello lyra");
        // ProcessDriver spawning a real shell binary will fail in this test
        // environment (no `openspine-shell` on PATH) — that's fine, the
        // pipeline still must reach `insert_task_grant` before the spawn
        // attempt, which is what this test asserts by inspecting the
        // returned grant and the store directly.
        let grant = handle_owner_update(&state, &update)
            .await
            .unwrap()
            .expect("owner message must compose a grant");
        assert_eq!(grant.agent_id, "main_assistant_agent");
        assert_eq!(grant.workflow_id, "owner_control_conversation");
        assert_eq!(grant.route_id, "owner_telegram_main_assistant");

        let (stored_grant, pending_ref, bound_chat_id) = state
            .store
            .find_task_grant_by_token(&grant.task_token)
            .unwrap()
            .expect("grant must be persisted");
        assert_eq!(stored_grant, grant);
        assert_eq!(bound_chat_id, 555);
        assert_eq!(state.artifacts.get(&pending_ref).unwrap(), b"hello lyra");
        assert!(state.store.verify_audit_chain().unwrap());
    }
}

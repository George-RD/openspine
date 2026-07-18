// openspine:allow-large-module reason: owner command routing and shared pipeline entrypoints remain co-located for the single event boundary
//! The owner-message pipeline: Telegram update -> owner verification ->
//! identity resolution -> route resolution -> authority composition -> task
//! grant -> sandboxed shell spawn.
//!
//! The pipeline's execution is now delegated to the single typed
//! [`driver::run_pipeline`], which interprets one of two compiled-in lane
//! specifications ([`driver::owner_control_lane`] and
//! [`driver::email_preview_lane`]) over the nine-stage sequence declared once
//! in [`driver::PipelineStage`]. This module keeps the shared helpers the
//! lanes rely on ([`AppState`], [`empty_session_policy`],
//! [`notify_owner_best_effort`]) and the public entry points
//! ([`run_telegram_poll_loop`], [`handle_owner_update`]). Identity resolution
//! lives in [`crate::identity::IdentityResolver`], driven by an unforgeable
//! [`crate::telegram::VerifiedOwnerContext`] minted only by
//! [`crate::telegram::verify_update`].
//!
//! Lane selection (the `/draft <thread_id>` command) is recognized here, at
//! the Event-stage boundary, and handed to the driver as lane data — the
//! driver never re-branches on it. Every step that terminates the pipeline
//! early is audited, so "why didn't Lyra reply" is always answerable from
//! `audit_log` alone.
//!
//! v1 has one owner principal (bootstrapped at kernel start). The Telegram
//! owner user id remains only the channel *authentication* signal for
//! [`crate::telegram::verify_update`]; composition consumes the resolved
//! `principal_id` (AD-146).
mod approval;
mod artifact_activation;
mod artifact_nomination;
mod artifact_reconfirmation;
mod digest_pagination;
mod driver;
mod driver_failures;
pub(crate) mod headless;
mod lanes;
mod message_notify;
mod offset;
mod polling;
pub(crate) use message_notify::{
    notify_owner_best_effort, notify_owner_required, notify_owner_required_outcome,
    notify_owner_with_digest,
};
pub use polling::run_telegram_poll_loop;
mod plan_approval;
mod stages;
pub(crate) mod standing_rule_timer;
mod timer_dispatch;
mod worker_failed_consumer;
mod worker_result_consumer;
pub(crate) use offset::initialize_telegram_bot_id_until_ready;
#[cfg(test)]
pub(crate) use offset::{
    dispatch_polled_updates_for_test, initialize_telegram_bot_id, resolve_telegram_offset_for_test,
};
pub(crate) use offset::{is_already_processed, resolve_telegram_offset};
pub(crate) use standing_rule_timer::run_standing_rule_dark_window_consumer;
pub(crate) use timer_dispatch::run_task_deadline_consumer;
#[cfg(test)]
pub(crate) use timer_dispatch::{
    dispatch_task_timer_event, dispatch_task_wake, recover_timer_dispatches, TimerDispatchOutcome,
};
pub(crate) use worker_failed_consumer::run_worker_failed_consumer;
pub(crate) use worker_result_consumer::run_worker_result_consumer;
mod post_approval;
mod selection;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::approval_fixture_grant;

use jiff::Timestamp;
use openspine_schemas::action::ActionCatalog;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::grant::GrantLimits;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::policy::{Constraints, SessionPolicy};
use ulid::Ulid;

use crate::api::handler_registry::ActionHandlerRegistry;
use crate::artifact_loader::ArtifactRegistry;
use crate::artifact_store::ArtifactStore;
use crate::connector_reality::WebhookVerifier;
use crate::connectors::ConnectorRegistry;
use crate::sandbox::Sandbox;
use crate::secret_store::SecretStore;
use crate::store::Store;
use crate::telegram::{self, VerifiedUpdate};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use approval::handle_draft_approval_callback;
use driver::run_pipeline;
use lanes::{email_preview_lane, owner_control_lane, EventInputs};
use plan_approval::handle_plan_approval_callback;

/// Everything the pipeline needs to turn one Telegram update into an
/// audited, sandboxed task. Built once at kernel startup and shared
/// (read-only except for its own interior-mutable pieces) across the
/// Telegram poll loop and the axum HTTP layer.
pub struct AppState {
    pub store: Store,
    pub artifacts: ArtifactStore,
    pub secrets: std::sync::Arc<SecretStore>,
    pub registry: parking_lot::RwLock<ArtifactRegistry>,
    pub action_catalog: ActionCatalog,
    pub sandbox: Sandbox,
    pub action_handlers: ActionHandlerRegistry,
    pub connectors: ConnectorRegistry,
    /// HMAC-SHA256 verifier for inbound webhooks (AD-134/AD-141). Shared
    /// kernel state so replay dedup is stable across hook deliveries.
    #[allow(dead_code)]
    pub webhook_verifier: WebhookVerifier,
    pub owner_user_id: i64,
    pub owner_principal_id: Ulid,
    pub owner_identity_id: Ulid,
    /// e.g. `http://127.0.0.1:7777` — passed to the shell as `KERNEL_ENDPOINT`.
    pub kernel_endpoint: String,
    /// D-025 / PRD §16 escape hatch. See [`sandbox::refuses_external_communication_without_containment`].
    pub unsafe_allow_uncontained_private_data: bool,
    /// Provider clients are resolved once at startup from the operator's
    /// configured pool; runtime proposals can only switch the active role
    /// to one of these pre-vetted clients (AD-152, no silent swaps).
    pub provider_pool: HashMap<String, crate::model_gateway::ProviderClient>,
    pub gateway_tier_map: crate::model_gateway::GatewayTierMap,
    /// Active provider id per governed model role. The map is kernel-owned
    /// and changes only in post-approval model-swap activation.
    pub active_model_providers:
        parking_lot::RwLock<HashMap<openspine_schemas::model_swap::ModelRole, String>>,
    pub provider_config_digests: HashMap<String, openspine_schemas::digest::Digest>,
    /// Backs `GET /v1/status`'s `uptime_seconds`.
    pub started_at: std::time::Instant,
    /// Bounded duration of any single connector call (AD-141: per-call
    /// connector timeout). Defaults to 30s; tests set it tiny to exercise
    /// timeouts quickly.
    pub connector_call_timeout: Duration,
    /// `data/artifacts.d` overlay dir (5a/5d): approved `artifact.propose`
    /// activations are written here as `<kind-plural>/<id>-v<version>.yaml`
    /// so they survive restart, and the startup loader re-merges them into
    /// the live registry alongside the fixtures.
    pub overlay_dir: PathBuf,
    /// AD-143: required global per-day spend cap across model and connector
    /// calls. The lane gate and usage reservations read this kernel setting.
    pub spend_cap: crate::config::SpendCapConfig,
    pub conversation_locks:
        parking_lot::Mutex<std::collections::HashMap<i64, std::sync::Arc<tokio::sync::Mutex<()>>>>,
    /// `(kind, id)` identities loaded from base fixtures before overlay merge.
    pub base_artifact_ids: std::collections::HashSet<(String, String)>,
    /// Digest of sorted active base artifacts reviewed by owner taps.
    pub base_compatibility_epoch: String,
}

impl AppState {
    pub async fn lock_conversation(&self, chat_id: i64) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.conversation_locks.lock();
            locks
                .entry(chat_id)
                .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        lock.lock_owned().await
    }
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
pub(crate) use message_notify::NotifyOutcome;

/// Short-lived owner-bound synthetic grant minted at the moment an owner taps
/// an `artifact.reconfirm` button (AD-070). The durable review object is the
/// pending learned-artifact row + ActionRequest; authority begins only here.
pub(super) fn mint_reconfirm_grant(task_grant_id: Ulid) -> Option<TaskGrant> {
    use openspine_schemas::action::ActionId;
    use openspine_schemas::grant::GrantMode;
    let key = crate::grant_hmac_key()?;
    let now = Timestamp::now();
    let reconfirm = ActionId::new("artifact.reconfirm");
    let mut grant = TaskGrant {
        id: task_grant_id,
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user: "kernel".to_string(),
        purpose: "overlay-reconfirm".to_string(),
        issued_by: "kernel".to_string(),
        issued_at: now,
        expires_at: now + std::time::Duration::from_secs(300),
        event_id: Ulid::new(),
        route_id: "overlay_reconfirm".to_string(),
        agent_id: "kernel".to_string(),
        workflow_id: "overlay_reconfirm".to_string(),
        capability_pack_id: "kernel".to_string(),
        authority_sources: vec![],
        selection_tokens: vec![],
        allowed_actions: vec![reconfirm.clone()],
        approval_required_actions: vec![reconfirm],
        denied_actions: vec![],
        allowed_egress_classes: vec![],
        output_channels: vec![],
        limits: GrantLimits {
            max_model_calls: 0,
            max_artifacts: 0,
            max_runtime_seconds: 0,
        },
        task_token: format!("reconfirm-{}", Ulid::new()),
        root_grant_id: Ulid::nil(),
        parent_grant_id: None,
        mode: GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
        persona_id: None,
    };
    grant.seal_root(&key);
    Some(grant)
}

#[cfg(test)]
pub(crate) use polling::poll_telegram_once_for_test;

/// Run one verified-or-not Telegram update through the full pipeline.
/// Returns `Ok(None)` for every outcome the pipeline itself decides on
/// (ignored, denied, refused, ambiguous) — those are not errors, they are
/// the pipeline correctly declining to act. Returns `Ok(Some(grant))` once
/// authority has been composed and persisted, *regardless* of whether the
/// subsequent shell spawn succeeds. Only a genuine infrastructure failure —
/// store I/O or an inconsistent registry — surfaces as `Err`.
///
/// Lane selection happens here, at the Event-stage boundary: a `/draft
/// <thread_id>` message selects the email-preview lane, any other owner
/// message selects the owner-control lane. The driver interprets the chosen
/// lane as data; it does not branch on command syntax itself.
/// Deterministic line-based content diff summary for the owner-facing
/// promote preview. Shows added/removed lines (up to 5 each) so the owner
/// can see what changed between prior and current skill content without
/// leaking the full body into the preview surface.
fn bounded_preview(text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes.saturating_sub("…".len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

fn content_diff_summary(prior: &str, current: &str) -> String {
    fn bounded_text(text: &str, max_bytes: usize) -> String {
        if text.len() <= max_bytes {
            return text.to_string();
        }
        let mut end = max_bytes.saturating_sub("…".len());
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &text[..end])
    }
    let prior_lines: Vec<&str> = prior.lines().collect();
    let current_lines: Vec<&str> = current.lines().collect();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut pi = 0;
    let mut ci = 0;
    while pi < prior_lines.len() || ci < current_lines.len() {
        if ci >= current_lines.len() {
            removed.push(prior_lines[pi]);
            pi += 1;
        } else if pi >= prior_lines.len() {
            added.push(current_lines[ci]);
            ci += 1;
        } else if prior_lines[pi] == current_lines[ci] {
            pi += 1;
            ci += 1;
        } else {
            // Try to find the current line in prior (line was removed)
            let mut found = false;
            for lookahead in pi + 1..prior_lines.len().min(pi + 3) {
                if prior_lines[lookahead] == current_lines[ci] {
                    for prior_line in prior_lines.iter().take(lookahead).skip(pi) {
                        removed.push(*prior_line);
                    }
                    pi = lookahead;
                    found = true;
                    break;
                }
            }
            if !found {
                // Try to find the prior line in current (line was added)
                for lookahead in ci + 1..current_lines.len().min(ci + 3) {
                    if current_lines[lookahead] == prior_lines[pi] {
                        for cur_line in current_lines.iter().take(lookahead).skip(ci) {
                            added.push(*cur_line);
                        }
                        ci = lookahead;
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                removed.push(prior_lines[pi]);
                added.push(current_lines[ci]);
                pi += 1;
                ci += 1;
            }
        }
    }
    let mut parts: Vec<String> = Vec::new();
    if !removed.is_empty() {
        let shown: Vec<String> = removed
            .iter()
            .take(5)
            .map(|line| bounded_text(line, 256))
            .collect();
        parts.push(format!(
            "-{} removed lines{}",
            removed.len(),
            if shown.len() < removed.len() {
                format!(" (e.g. {:?})", shown)
            } else {
                format!(": {:?}", shown)
            }
        ));
    }
    if !added.is_empty() {
        let shown: Vec<String> = added
            .iter()
            .take(5)
            .map(|line| bounded_text(line, 256))
            .collect();
        parts.push(format!(
            "+{} added lines{}",
            added.len(),
            if shown.len() < added.len() {
                format!(" (e.g. {:?})", shown)
            } else {
                format!(": {:?}", shown)
            }
        ));
    }
    if parts.is_empty() {
        "no content change".to_string()
    } else {
        bounded_text(&parts.join("; "), 2000)
    }
}

pub async fn handle_owner_update(
    state: &AppState,
    update: &telegram::TelegramUpdate,
) -> anyhow::Result<Option<TaskGrant>> {
    let verified = telegram::verify_update(update, state.owner_user_id);
    if let VerifiedUpdate::Ignored { reason } = &verified {
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

    let chat_id = match &verified {
        VerifiedUpdate::OwnerMessage { chat_id, .. } => *chat_id,
        VerifiedUpdate::OwnerCallback { chat_id, .. } => *chat_id,
        VerifiedUpdate::Ignored { .. } => unreachable!(),
    };

    let _guard = state.lock_conversation(chat_id).await;

    let (chat_id, text, owner_verified) = match verified {
        VerifiedUpdate::OwnerMessage {
            chat_id,
            text,
            context,
        } => (chat_id, text, Some(context)),
        VerifiedUpdate::OwnerCallback {
            chat_id,
            callback_query_id,
            data,
            context: _,
        } => {
            if let Some((pending_id, allow)) = telegram::parse_standing_rule_callback(&data) {
                let changed = state.store.resolve_pending_action(
                    &pending_id.to_string(),
                    allow,
                    jiff::Timestamp::now(),
                )?;
                crate::spend::guard_connector(state, true).await?;
                state
                    .connectors
                    .telegram()
                    .answer_callback_query(&callback_query_id)
                    .await?;
                crate::pipeline::notify_owner_best_effort(
                    state,
                    chat_id,
                    if changed {
                        "Standing-rule request resolved."
                    } else {
                        "Standing-rule request was already resolved."
                    },
                )
                .await;
            } else if let Some(action_request_id) = telegram::parse_approve_callback(&data) {
                handle_draft_approval_callback(
                    state,
                    chat_id,
                    &callback_query_id,
                    action_request_id,
                )
                .await?;
            } else if let Some(action_request_id) = telegram::parse_approve_plan_callback(&data) {
                handle_plan_approval_callback(
                    state,
                    chat_id,
                    &callback_query_id,
                    action_request_id,
                )
                .await?;
            } else {
                crate::spend::guard_connector(state, true).await?;
                let answer_result = crate::api::connector_breaker::call_with_connector_preflight(
                    state,
                    "telegram",
                    None,
                    state
                        .connectors
                        .telegram()
                        .answer_callback_query(&callback_query_id),
                )
                .await;
                crate::failure_surfacing::record_callback_ack(
                    state,
                    answer_result.is_ok(),
                    answer_result
                        .as_ref()
                        .err()
                        .map(|e| e.to_string())
                        .as_deref(),
                );

                state.store.append_audit(
                    "telegram.callback_unrecognized",
                    None,
                    None,
                    Some(&data),
                    None,
                    &[],
                    &[],
                )?;
            }
            return Ok(None);
        }
        VerifiedUpdate::Ignored { .. } => unreachable!(),
    };
    match crate::secret_intake::capture(state, chat_id, &text).await {
        Ok(Some(outcome)) => {
            let response = match outcome {
                crate::secret_intake::CaptureOutcome::Stored(crate::secret_intake::SecretMode::Intake) => {
                    "Secret intake completed; value was stored."
                }
                crate::secret_intake::CaptureOutcome::Staged(crate::secret_intake::SecretMode::Intake) => {
                    "Secret received and staged; provide the paired Gmail credential to validate and activate it."
                }
                crate::secret_intake::CaptureOutcome::Staged(crate::secret_intake::SecretMode::Rotate) => {
                    "Secret received and staged; provide the paired Gmail credential to validate and activate rotation."
                }
                crate::secret_intake::CaptureOutcome::Stored(crate::secret_intake::SecretMode::Rotate) => {
                    "Secret rotation completed; value was stored."
                }
                crate::secret_intake::CaptureOutcome::Rejected => {
                    "Secret message discarded; intake expired, failed validation, or was not bound to this chat. Retry."
                }
            };
            notify_owner_best_effort(state, chat_id, response).await;
            return Ok(None);
        }
        Ok(None) => {}
        Err(err) => {
            let _ = state.store.delete_kv("secret.intake.pending");
            tracing::warn!(error = %err, "secret capture failed; pending state cleared");
            notify_owner_best_effort(
                state,
                chat_id,
                "Secret capture failed; intake was cleared. Retry.",
            )
            .await;
            return Ok(None);
        }
    }
    if text.trim().starts_with("/secret") {
        if let Some((mode, slot)) = crate::secret_intake::parse_command(&text) {
            let proof = owner_verified
                .as_ref()
                .expect("verified owner message carries proof");
            let armed = crate::secret_intake::arm(
                state,
                chat_id,
                state.owner_principal_id,
                proof,
                mode,
                slot,
            )?;
            let response = if armed {
                "Secret mode armed; send the value in your next private message."
            } else {
                "Secret mode was denied; retry after verifying owner authority."
            };
            notify_owner_best_effort(state, chat_id, response).await;
        } else {
            notify_owner_best_effort(
                state,
                chat_id,
                "Invalid /secret command. Use /secret intake <slot> or /secret rotate <slot>.",
            )
            .await;
        }
        return Ok(None);
    }

    if let Some(args) = telegram::parse_digest_namespace(&text) {
        if !args.is_empty() && telegram::parse_digest_detail_command(&text).is_none() {
            notify_owner_best_effort(state, chat_id, "Usage: /digest or /digest <ULID> [page]")
                .await;
            return Ok(None);
        }
    }
    if let Some((channel_user_id, relationship_str)) = telegram::parse_bind_command(&text) {
        let result = crate::identity::handle_owner_bind(
            &state.store,
            state.owner_principal_id,
            state.owner_identity_id,
            owner_verified
                .as_ref()
                .expect("bind command requires verified owner"),
            channel_user_id,
            relationship_str,
        );
        let message = result.unwrap_or_else(|err| err);
        if !message.is_empty() {
            notify_owner_best_effort(state, chat_id, &message).await;
        }
        return Ok(None);
    }

    if let Some(rest) = text
        .strip_prefix("/skill install")
        .filter(|r| r.is_empty() || r.chars().next().is_some_and(char::is_whitespace))
    {
        let message = match owner_verified.as_ref() {
            Some(proof) => {
                let payload = rest.trim();
                if payload.is_empty() {
                    "Usage: /skill install <skill-json>".to_string()
                } else if payload.len() > 64 * 1024 {
                    "Skill install rejected: payload too large (max 64 KiB).".to_string()
                } else {
                    // The complete Skill JSON is the sole artifact source
                    // (AD-041): deny_unknown_fields rejects extra fields and a
                    // missing required field is a serde error, so a message
                    // cannot header-address one id/version while persisting
                    // another. The ceremony assigns UserInstalled provenance,
                    // the matching state, and recomputes the digest from the
                    // body — the payload's self-asserted
                    // provenance/state/digest are never trusted.
                    match serde_json::from_str::<openspine_schemas::skill::Skill>(payload) {
                        Ok(mut skill) => {
                            match crate::skill::ceremony::install_user_skill(
                                &state.store,
                                state.owner_principal_id,
                                proof,
                                &mut skill,
                                jiff::Timestamp::now(),
                            ) {
                                Ok(()) => {
                                    format!("Skill {} v{} installed.", skill.id, skill.version)
                                }
                                Err(e) => format!("Skill install failed: {e}"),
                            }
                        }
                        Err(e) => format!("Skill install rejected: invalid payload ({e})."),
                    }
                }
            }
            None => "Usage: /skill install <skill-json> (owner verification required)".to_string(),
        };
        if !message.is_empty() {
            notify_owner_best_effort(state, chat_id, &message).await;
        }
        return Ok(None);
    }
    if let Some(rest) = text.strip_prefix("/promote ") {
        let mut parts = rest
            .splitn(4, char::is_whitespace)
            .filter(|p| !p.is_empty());
        let skill_id = parts.next();
        let version = parts.next().and_then(|v| v.parse::<u32>().ok());
        let action = parts.next();
        let reason = parts.next().unwrap_or("owner decision");
        let message = match (skill_id, version, action, owner_verified.as_ref()) {
            (Some(id), Some(ver), None, Some(_proof)) => {
                match crate::store::skill_store::get_skill(&state.store, id, ver) {
                    Ok(Some(skill)) => {
                        // AD-041: resolve the actual highest active prior version
                        // from the store (not assumed v-1), so a non-contiguous
                        // version history still shows the correct prior content.
                        let prior = match crate::store::skill_read_queries::highest_prior_version(
                            &state.store,
                            id,
                            ver,
                        ) {
                            Ok(Some(prev)) => {
                                let diff = content_diff_summary(&prev.body, &skill.body);
                                bounded_preview(
                                    format!(
                                        "prior v{} digest={} title={:?} task_shape={:?} visibility={:?}\n{}",
                                        prev.version,
                                        prev.content_digest,
                                        prev.title,
                                        prev.task_shape,
                                        prev.visibility,
                                        diff
                                    ),
                                    900,
                                )
                            }
                            Ok(None) => bounded_preview(
                                format!(
                                    "first version (no prior); body excerpt={:?}",
                                    skill.body.chars().take(128).collect::<String>()
                                ),
                                900,
                            ),
                            Err(_) => "no prior version found".to_string(),
                        };
                        let provenance_summary =
                            bounded_preview(format!("{:?}", skill.provenance), 300);
                        let current_diff = bounded_preview(
                            format!(
                                "digest={} title={:?} provenance={:?} visibility={:?}",
                                skill.content_digest,
                                skill.title,
                                skill.provenance,
                                skill.visibility
                            ),
                            900,
                        );
                        let task_shape_summary =
                            bounded_preview(format!("{:?}", skill.task_shape), 300);
                        let preview_text = bounded_preview(
                            format!(
                            "Skill review: id={} version={} provenance={} digest={} task_shape={:?}.
                             --- RESERVED CONTENT DIFF ---
                             prior: {}
                             current: {}
                             --- END CONTENT DIFF ---
Use /promote {} {} approve or /promote {} {} reject <reason>.",
                            skill.id,
                            skill.version,
                            provenance_summary,
                            skill.content_digest,
                            task_shape_summary,
                            prior,
                            current_diff,
                            skill.id,
                            skill.version,
                            skill.id,
                            skill.version,
                        ),
                            3500,
                        );
                        // Persist the preview ONLY after the owner actually
                        // receives the message (confirmed Telegram send).
                        // A failed send leaves no consumable preview record,
                        // so approve/reject without a confirmed preview fails
                        // closed (AD-041/AD-110: the owner must see the
                        // content before deciding).
                        let outcome = crate::pipeline::notify_owner_with_digest(
                            state,
                            chat_id,
                            &preview_text,
                            &[],
                            None,
                        )
                        .await;
                        if outcome == crate::pipeline::message_notify::NotifyOutcome::Sent {
                            if let Err(e) =
                                crate::store::skill_preview_records::record_skill_preview(
                                    &state.store.conn.lock(),
                                    id,
                                    ver,
                                    &state.owner_principal_id.to_string(),
                                    &skill.content_digest,
                                    &provenance_summary,
                                    &prior,
                                    &current_diff,
                                    &preview_text,
                                )
                            {
                                format!("Skill preview failed: {e}")
                            } else {
                                // Preview already sent via notify_owner_with_digest;
                                // return empty so the outer notify_owner_best_effort
                                // is a no-op (avoids double-sending).
                                String::new()
                            }
                        } else {
                            format!("Skill preview delivery failed (outcome={outcome:?}); preview not persisted. Retry /promote.")
                        }
                    }
                    Ok(None) => "Skill not found.".to_string(),
                    Err(err) => format!("Skill review unavailable: {err}"),
                }
            }
            (Some(id), Some(ver), Some("approve"), Some(proof)) => {
                crate::skill::ceremony::owner_decide_promotion(
                    &state.store,
                    state.owner_principal_id,
                    proof,
                    id,
                    ver,
                    crate::skill::ceremony::OwnerSkillDecision::Approve,
                )
                .map(|_| "Skill promotion approved.".to_string())
                .unwrap_or_else(|err| format!("Skill promotion denied: {err}"))
            }
            (Some(id), Some(ver), Some("reject"), Some(proof)) => {
                crate::skill::ceremony::owner_decide_promotion(
                    &state.store,
                    state.owner_principal_id,
                    proof,
                    id,
                    ver,
                    crate::skill::ceremony::OwnerSkillDecision::Reject {
                        reason: reason.to_string(),
                    },
                )
                .map(|_| "Skill rejected.".to_string())
                .unwrap_or_else(|err| format!("Skill rejection failed: {err}"))
            }
            _ => "Usage: /promote <skill_id> <version> <approve|reject> [reason]".to_string(),
        };
        if !message.is_empty() {
            notify_owner_best_effort(state, chat_id, &message).await;
        }
        return Ok(None);
    }

    if let Some((id, page)) = telegram::parse_digest_detail_command(&text) {
        digest_pagination::handle_detail_command(state, chat_id, id, page).await?;
        return Ok(None);
    }
    if telegram::parse_digest_command(&text) {
        digest_pagination::handle_command(state, chat_id).await?;
        return Ok(None);
    }

    // D-036 / design.md: the `/draft <thread_id>` command is the entire
    // trust boundary for "did the owner select this thread" — it is
    // recognized here as lane selection, and the driver interprets lane data;
    // it never re-branches on command syntax. The chosen lane is then run
    // through the same synchronous stage prefix as every other owner event.
    let thread_id = telegram::parse_draft_command(&text).map(str::to_string);
    let spec = if thread_id.is_some() {
        email_preview_lane()
    } else {
        owner_control_lane()
    };
    let inputs = EventInputs {
        chat_id,
        text,
        thread_id,
        owner_verified,
        principal_override: None,
        event_type_override: None,
        timer_event_id: None,
        correlated_task_id: None,
        dispatch_key: None,
        dispatch_timer_id: None,
    };
    run_pipeline(state, spec, &inputs, Timestamp::now(), &mut Vec::new()).await
}

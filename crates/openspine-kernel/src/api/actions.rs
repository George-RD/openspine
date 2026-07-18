// openspine:allow-large-module reason: action mediation and dispatch (gate, handler dispatch, lyra preview, approval flow, failure surfacing)
use super::authenticate;
use super::connector_breaker::call_with_connector;
use super::proposal::{propose_draft_creation, ProposalError};
use super::telegram_truncate::{truncate_for_telegram, truncate_with_notice};
use crate::failure_surfacing::{batch_failure, FailureClass};
use crate::pipeline::AppState;
use crate::store::standing_rules::{standing_rule_fingerprint, PendingScheduleCtx};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use jiff::Timestamp;
use openspine_gate::{gate, ActionOrigin};
use openspine_schemas::action::{
    ActionId, ActionRequest, GateDecision, SkillAttribution, SkillAttributionKind,
};
use openspine_schemas::digest::canonical_json;
use openspine_schemas::escalation::{surface_denial, EscalationEvent};
use openspine_schemas::grant::TaskGrant;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::str::FromStr;
use std::sync::Arc;
use ulid::Ulid;

async fn guard_connector_dispatch(
    state: &AppState,
    grant: &TaskGrant,
) -> Result<(), DispatchError> {
    let immediate = matches!(grant.workflow_id.as_str(), "owner_control_conversation");
    crate::spend::guard_connector(state, immediate)
        .await
        .map_err(DispatchError::Resource)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActionRequestBody {
    action: String,
    #[serde(default)]
    #[allow(dead_code)]
    target: Option<Value>,
    #[serde(default)]
    payload: Option<Value>,
    #[serde(default)]
    skill_context_token_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TelegramReplyPayload {
    pub(super) text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PreviewPayload {
    pub(super) subject: String,
    pub(super) body: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadThreadPayload {
    pub(super) selection_token_id: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ActionResponseBody {
    decision: GateDecision,
    #[serde(skip_serializing_if = "Option::is_none")]
    counterparty_deferral: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    standing_rule_budget: Option<StandingRuleBudgetInfo>,
}

/// Remaining standing-rule budget returned in the gate response so agents
/// self-adjust without extra round-trips (AD-013 calibration / AD-106).
#[derive(Debug, Serialize)]
pub struct StandingRuleBudgetInfo {
    pub(crate) quota_remaining: u32,
    pub(crate) rate_remaining: u32,
    /// Whether a dark-window timer was scheduled for this consultation
    /// (AD-012 leaning): the owner's silence will apply the rule's
    /// pre-agreed default. Surfaced so the agent can report the pending
    /// default rather than retrying a saturated window.
    dark_window_scheduled: bool,
}

pub(super) async fn post_actions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ActionRequestBody>,
) -> Result<Json<ActionResponseBody>, (StatusCode, Json<Value>)> {
    let (grant, _pending_ref, bound_chat_id) = authenticate(&state, &headers).await?;
    let action = ActionId::new(body.action);
    let payload = body.payload;
    let token_text = body.skill_context_token_id.as_deref();
    let (skill_attribution, skill_context_token) = match token_text {
        Some(text) => {
            let token_id = Ulid::from_str(text).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid skill context token"})),
                )
            })?;
            let selection = crate::store::skill_read_queries::find_live_skill_context_selection(
                &state.store,
                token_id,
                grant.id,
            )
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "internal_error"})),
                )
            })?
            .ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid or expired skill context token"})),
                )
            })?;
            if selection.agent_id != grant.agent_id || selection.pack_id != grant.capability_pack_id
            {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "skill context token scope mismatch"})),
                ));
            }
            (
                Some(SkillAttribution {
                    id: selection.skill_id.clone(),
                    version: selection.skill_version,
                    kind: SkillAttributionKind::Causal,
                }),
                Some((token_id, selection)),
            )
        }
        None => {
            let selections = crate::store::skill_read_queries::live_skill_context_selections(
                &state.store,
                grant.id,
            )
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "internal_error"})),
                )
            })?;
            if selections.is_empty() {
                (None, None)
            } else {
                (
                    Some(SkillAttribution {
                        id: "skill.context".to_string(),
                        version: 0,
                        kind: SkillAttributionKind::Contextual {
                            skills: selections
                                .iter()
                                .take(8)
                                .map(|s| format!("{} v{}", s.skill_id, s.skill_version))
                                .collect(),
                            omitted: selections.len().saturating_sub(8),
                        },
                    }),
                    None,
                )
            }
        }
    };
    let (decision, counterparty_deferral, result, standing_rule_budget) =
        mediate_and_dispatch_action_with_attribution_and_token(
            &state,
            &grant,
            action,
            bound_chat_id,
            payload.as_ref(),
            FailureSurface::DirectResponse,
            skill_attribution.as_ref(),
            skill_context_token.map(|(id, _)| id),
            None,
        )
        .await
        .map_err(|err| match &err {
            DispatchError::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, Json(json!({"error": message})))
            }
            DispatchError::Connector(cause)
            | DispatchError::ConnectorUnavailable(cause)
            | DispatchError::DeliveryUnknown(cause)
            | DispatchError::Resource(cause) => {
                tracing::error!(error = %cause, "action dispatch failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "internal_error"})),
                )
            }
        })?;
    Ok(Json(ActionResponseBody {
        decision,
        counterparty_deferral,
        result,
        standing_rule_budget,
    }))
}

fn cleanup_pre_effect_reservations(
    state: &AppState,
    consult_reservation: Option<&(String, u32, String)>,
    fired_reservation: Option<&(String, u32, String)>,
    fired_pending_id: Option<&str>,
) {
    if let Some((_, _, reservation_id)) = consult_reservation {
        if let Err(err) = state.store.cancel_standing_rule_reservation(reservation_id) {
            tracing::error!(
                error = %err,
                reservation_id,
                "standing-rule reservation cancel failed before effect"
            );
        }
    }
    if let Some((_, _, reservation_id)) = fired_reservation {
        // Rearm the fired one-use token ONLY after the reserved budget was
        // actually cancelled. A cancel failure must leave the row in its
        // pre-cleanup `claimed` state so recovery surfaces it fail-closed
        // (never silently re-run, never rearm a double-spent token).
        match state.store.cancel_standing_rule_reservation(reservation_id) {
            Ok(()) => {
                if let Some(pending_id) = fired_pending_id {
                    if let Err(err) = state.store.rearm_standing_rule_fired_pending(pending_id) {
                        tracing::error!(
                            error = %err,
                            pending_id,
                            "standing-rule fired pending rearm failed before effect"
                        );
                    }
                }
            }
            Err(err) => {
                tracing::error!(
                    error = %err,
                    reservation_id,
                    "standing-rule fired reservation cancel failed before effect"
                );
            }
        }
    }
}

/// Shared mediation boundary for HTTP and durable workflow actions.
pub(crate) async fn mediate_and_dispatch_action(
    state: &AppState,
    grant: &TaskGrant,
    action: ActionId,
    bound_chat_id: i64,
    payload: Option<&Value>,
    surface: FailureSurface,
    fired_pending: Option<&str>,
) -> Result<
    (
        GateDecision,
        Option<String>,
        Option<Value>,
        Option<StandingRuleBudgetInfo>,
    ),
    DispatchError,
> {
    mediate_and_dispatch_action_with_attribution_and_token(
        state,
        grant,
        action,
        bound_chat_id,
        payload,
        surface,
        None,
        None,
        fired_pending,
    )
    .await
}

#[cfg(test)]
pub(crate) async fn mediate_and_dispatch_action_with_attribution(
    state: &AppState,
    grant: &TaskGrant,
    action: ActionId,
    bound_chat_id: i64,
    payload: Option<&Value>,
    surface: FailureSurface,
    skill_attribution: Option<&SkillAttribution>,
) -> Result<(GateDecision, Option<String>, Option<Value>), DispatchError> {
    let (decision, deferral, result, _budget) =
        mediate_and_dispatch_action_with_attribution_and_token(
            state,
            grant,
            action,
            bound_chat_id,
            payload,
            surface,
            skill_attribution,
            None,
            None,
        )
        .await?;
    Ok((decision, deferral, result))
}

#[allow(clippy::too_many_arguments)]
async fn mediate_and_dispatch_action_with_attribution_and_token(
    state: &AppState,
    grant: &TaskGrant,
    action: ActionId,
    bound_chat_id: i64,
    payload: Option<&Value>,
    surface: FailureSurface,
    skill_attribution: Option<&SkillAttribution>,
    skill_context_token: Option<Ulid>,
    fired_pending: Option<&str>,
) -> Result<
    (
        GateDecision,
        Option<String>,
        Option<Value>,
        Option<StandingRuleBudgetInfo>,
    ),
    DispatchError,
> {
    let now = Timestamp::now();
    let spend_lane = crate::spend::SpendLane::from_grant(grant);
    if !crate::spend::admit_spend(state, spend_lane, now)
        .await
        .map_err(|err| DispatchError::Resource(err.into()))?
    {
        return Err(DispatchError::Resource(anyhow::anyhow!(
            "daily spend cap exceeded"
        )));
    }
    let payload_ref = match payload {
        Some(value) => Some(
            state
                .artifacts
                .put(canonical_json(value).as_bytes())
                .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?,
        ),
        None => None,
    };
    let selection_token_id = payload
        .and_then(|value| value.get("selection_token_id"))
        .and_then(Value::as_str)
        .and_then(|value| Ulid::from_str(value).ok());
    let params = payload
        .map(|v| {
            v.as_object()
                .map(|obj| {
                    obj.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect::<std::collections::BTreeMap<String, String>>()
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();
    if let Some(attr) = skill_attribution.as_ref() {
        if !matches!(&attr.kind, SkillAttributionKind::Contextual { .. }) {
            let visible = crate::store::skill_read_queries::installed_skills_for_agent_and_pack(
                &state.store,
                &grant.agent_id.to_string(),
                &grant.capability_pack_id.to_string(),
            )
            .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?;
            if !visible
                .iter()
                .any(|skill| skill.id == attr.id && skill.version == attr.version)
            {
                return Err(DispatchError::BadRequest(
                    "skill attribution is outside grant visibility".to_string(),
                ));
            }
        }
    }
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: action.clone(),
        target_ref: None,
        payload_ref: payload_ref.clone(),
        target_digest: None,
        selection_token_id,
        params,
        skill_attribution: skill_attribution.cloned(),
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
    if let Some(token_id) = skill_context_token {
        let consumed = state
            .store
            .consume_skill_context_selection_and_append_audit(
                token_id,
                grant.id,
                &grant.agent_id.to_string(),
                &grant.capability_pack_id.to_string(),
                &action,
                &outcome.decision,
                payload_ref.as_slice(),
            )
            .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?;
        if !consumed {
            return Err(DispatchError::BadRequest(
                "invalid or expired skill context token".to_string(),
            ));
        }
    } else {
        state
            .store
            .append_audit(
                "action.gated",
                Some(&action),
                Some(&outcome.decision),
                None,
                Some(grant.id),
                &[],
                payload_ref.as_slice(),
            )
            .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?;
    }

    let mut decision = outcome.decision;
    // Standing-rule consultation (AD-010/AD-106/AD-012): a fired dark-window
    // default and a normal consultation are mutually exclusive — at most one
    // reserves budget for this effect, so a re-dispatched default never also
    // consumes a fresh normal reservation (P1-6 double-charge guard).
    let mut standing_budget: Option<StandingRuleBudgetInfo> = None;
    let mut consult_reservation: Option<(String, u32, String)> = None;
    let mut fired_reservation: Option<(String, u32, String)> = None;
    if let Some(token) = fired_pending {
        // Fired dark-window default (owner silence): re-dispatch this action
        // with a digest-bound, one-use token. Consume it *before* the effect
        // so the re-dispatch is over-budget only against the fired waiver; the
        // effective Allow is audited only after the token is admitted, and the
        // reservation is finalized on success or cancelled on failure
        // (P1-5/P1-6/P1-11).
        if matches!(decision, GateDecision::ApprovalRequired { .. }) {
            if let Ok(Some((rule_id, version, reservation_id))) =
                state.store.consume_standing_rule_fired_pending(
                    token,
                    &action,
                    grant.id,
                    bound_chat_id,
                    &payload_ref,
                    now,
                )
            {
                decision = GateDecision::Allow;
                fired_reservation = Some((rule_id, version, reservation_id));
                if let Err(err) = state.store.append_audit(
                    "action.gated",
                    Some(&action),
                    Some(&GateDecision::Allow),
                    Some("fired dark-window default admitted (effective Allow audited before effect)"),
                    Some(grant.id),
                    &[],
                    payload_ref.as_slice(),
                ) {
                    cleanup_pre_effect_reservations(
                        state,
                        consult_reservation.as_ref(),
                        fired_reservation.as_ref(),
                        Some(token),
                    );
                    return Err(DispatchError::Resource(anyhow::Error::new(err)));
                }
            }
        }
    } else if matches!(decision, GateDecision::ApprovalRequired { .. }) {
        // Normal path: an active, non-expired, non-revoked rule covers this
        // action and still has budget → reserve it atomically and allow
        // without a fresh owner approval; otherwise keep ApprovalRequired and,
        // if a dark window is configured, let the gate schedule its timer.
        let ctx = PendingScheduleCtx {
            bound_chat_id,
            grant_id: grant.id,
            payload_ref: payload_ref.clone(),
            fingerprint: standing_rule_fingerprint(&action, grant.id, bound_chat_id, &payload_ref),
        };
        let consult = crate::standing_rules_gate::consult_standing_rule_gate(
            &state.store,
            &action,
            now,
            Some(&ctx),
        )
        .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?;
        if let (Some(rule), Some(reservation_id)) =
            (consult.rule.clone(), consult.reservation_id.clone())
        {
            consult_reservation = Some((rule.rule_id, rule.version, reservation_id));
        }
        if consult.allow {
            decision = GateDecision::Allow;
            if let Err(err) = state.store.append_audit(
                "action.gated",
                Some(&action),
                Some(&GateDecision::Allow),
                Some("standing-rule effective Allow admitted before effect"),
                Some(grant.id),
                &[],
                payload_ref.as_slice(),
            ) {
                cleanup_pre_effect_reservations(
                    state,
                    consult_reservation.as_ref(),
                    fired_reservation.as_ref(),
                    fired_pending,
                );
                return Err(DispatchError::Resource(anyhow::Error::new(err)));
            }
        }
        if consult.matched {
            // Headroom is only returned on an authorized Allow (AD-013/AD-106):
            // a denial must not expose remaining-capacity metadata, so a
            // saturated/expired consult yields `None` budget info.
            if consult.allow {
                let (q, r) = consult.budget_info().unwrap_or((0, 0));
                standing_budget = Some(StandingRuleBudgetInfo {
                    quota_remaining: q,
                    rate_remaining: r,
                    dark_window_scheduled: consult.dark_window_scheduled,
                });
            }
            if consult.dark_window_scheduled {
                if let Some(rule) = consult.rule.as_ref() {
                    let pending_id = match state.store.pending_id_for_fingerprint(
                        &rule.rule_id,
                        rule.version,
                        &ctx.fingerprint,
                    ) {
                        Ok(pending_id) => pending_id,
                        Err(err) => {
                            cleanup_pre_effect_reservations(
                                state,
                                consult_reservation.as_ref(),
                                fired_reservation.as_ref(),
                                fired_pending,
                            );
                            return Err(DispatchError::Resource(anyhow::Error::new(err)));
                        }
                    };
                    if let Some(pending_id) = pending_id {
                        if let Ok(pending_ulid) = pending_id.parse() {
                            if let Err(err) = guard_connector_dispatch(state, grant).await {
                                tracing::warn!(
                                    error = ?err,
                                    pending_id,
                                    "connector guard blocked standing-rule resolution buttons"
                                );
                            } else if let Err(err) = state
                                .connectors
                                .telegram()
                                .send_reply_with_standing_rule_buttons(
                                    bound_chat_id,
                                    "Standing-rule budget is exhausted. Choose the pending action's Allow or Deny outcome.",
                                    pending_ulid,
                                )
                                .await
                            {
                                tracing::warn!(
                                    error = %err,
                                    pending_id,
                                    "failed to send standing-rule resolution buttons"
                                );
                            }
                        } else {
                            tracing::warn!(pending_id, "malformed standing-rule pending id");
                        }
                    }
                }
            }
        }
    }
    if !matches!(decision, GateDecision::Allow) {
        if state.action_catalog.is_counterparty_facing(&action) {
            if let Some((deferral, notice)) = surface_denial(grant, &action, &decision, None, now) {
                let event = EscalationEvent::from_denial(&notice);
                crate::escalation::route_escalation(state, grant, &event)
                    .await
                    .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?;
                if let Some(attr) = request.skill_attribution.as_ref() {
                    let summary = match &attr.kind {
                        SkillAttributionKind::Contextual { skills, omitted } => {
                            let suffix = if *omitted > 0 {
                                format!(" +{} more", omitted)
                            } else {
                                String::new()
                            };
                            format!(
                                "denied action in task with active skills: {}{}",
                                skills.join(", "),
                                suffix
                            )
                        }
                        SkillAttributionKind::Causal => format!(
                            "skill-derived action denied at gate: {} skill {} v{}",
                            action.0, attr.id, attr.version
                        ),
                    };
                    batch_failure(
                        state,
                        FailureClass::GateDenial,
                        &summary,
                        &format!(
                            "action={} skill={} version={} decision={:?}",
                            action.0, attr.id, attr.version, decision
                        ),
                    )
                    .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?;
                }

                return Ok((
                    decision,
                    Some(deferral.text.to_string()),
                    None,
                    standing_budget,
                ));
            }
        }
        return Ok((decision, None, None, standing_budget));
    }

    // AD-103/AD-141: every connector effect runs through its breaker + bounded
    // timeout via the handler's `call_with_connector`. An Open breaker blocks
    // the effect with the distinct `connector_unavailable` audit event
    // (operational, not policy) — emitted inside the helper, never here.
    let dispatched = super::connector_breaker::dispatch_allowed_action(
        state,
        grant,
        &action,
        bound_chat_id,
        payload,
    )
    .await;
    match dispatched {
        Ok(result) => {
            if let Some((rule_id, version, reservation_id)) = &consult_reservation {
                if let Err(err) = state.store.finalize_standing_rule_reservation(
                    rule_id,
                    *version,
                    reservation_id,
                    now,
                ) {
                    tracing::error!(error = %err, reservation_id, "standing-rule reservation finalize failed after successful dispatch");
                }
            }
            if let Some((rule_id, version, reservation_id)) = &fired_reservation {
                if let Err(err) = state.store.finalize_standing_rule_reservation(
                    rule_id,
                    *version,
                    reservation_id,
                    now,
                ) {
                    tracing::error!(error = %err, reservation_id, "standing-rule fired reservation finalize failed after successful dispatch");
                }
                let receipt = format!("fired-effect:{reservation_id}:{now}");
                if let Err(err) = state
                    .store
                    .mark_fired_effect_attempted(reservation_id, &receipt)
                {
                    tracing::error!(error = %err, reservation_id, "standing-rule fired effect attempt not recorded");
                }
            }
            Ok((GateDecision::Allow, None, Some(result), standing_budget))
        }
        Err(err) => {
            // DeliveryUnknown means the provider may have acted before the
            // response was lost. Keep the budget consumed and fence retries;
            // releasing either reservation would permit a duplicate effect.
            let retain_reservation = match &err {
                // Ambiguous provider outcome: retain budget and fence any retry.
                DispatchError::DeliveryUnknown(_) => true,
                // Only these variants are proven pre-effect failures here.
                DispatchError::BadRequest(_)
                | DispatchError::Connector(_)
                | DispatchError::ConnectorUnavailable(_) => false,
                // Resource includes persistence/recording failures whose
                // effect ordering may be unknown; retain budget fail-closed.
                DispatchError::Resource(_) => true,
            };
            if retain_reservation {
                if let Some((rule_id, version, reservation_id)) = &consult_reservation {
                    if let Err(finalize_err) = state.store.finalize_standing_rule_reservation(
                        rule_id,
                        *version,
                        reservation_id,
                        now,
                    ) {
                        tracing::error!(error = %finalize_err, reservation_id, "standing-rule reservation finalize failed after delivery-unknown dispatch");
                    }
                }
                if let Some((rule_id, version, reservation_id)) = &fired_reservation {
                    if let Err(finalize_err) = state.store.finalize_standing_rule_reservation(
                        rule_id,
                        *version,
                        reservation_id,
                        now,
                    ) {
                        tracing::error!(error = %finalize_err, reservation_id, "standing-rule fired reservation finalize failed after delivery-unknown dispatch");
                    }
                    let receipt = format!("delivery-unknown:{reservation_id}:{now}");
                    if let Err(mark_err) = state
                        .store
                        .mark_fired_effect_attempted(reservation_id, &receipt)
                    {
                        tracing::error!(error = %mark_err, reservation_id, "standing-rule fired delivery-unknown attempt not recorded");
                    }
                }
            } else {
                // Confirmed pre-effect failures release the reservation and,
                // for fired defaults, rearm the one-use token only after the
                // cancellation succeeds.
                cleanup_pre_effect_reservations(
                    state,
                    consult_reservation.as_ref(),
                    fired_reservation.as_ref(),
                    fired_pending,
                );
            }
            // AD-103: a `ConnectorUnavailable` (Open/HalfOpen breaker) already
            // appended the distinct `connector_unavailable` audit; do not also
            // record `action.dispatch_failed` or batch it (that would
            // double-count the operational outage).
            if matches!(err, DispatchError::ConnectorUnavailable(_)) {
                return Err(err);
            }
            let digest_class = match &err {
                DispatchError::Resource(_) | DispatchError::DeliveryUnknown(_) => {
                    FailureClass::Resource
                }
                DispatchError::Connector(_) | DispatchError::BadRequest(_) => {
                    FailureClass::Connector
                }
                DispatchError::ConnectorUnavailable(_) => unreachable!(),
            };
            let digest_summary = match &err {
                DispatchError::BadRequest(msg) => msg.clone(),
                DispatchError::Connector(cause)
                | DispatchError::Resource(cause)
                | DispatchError::DeliveryUnknown(cause) => {
                    tracing::error!(error = %cause, "action dispatch failed");
                    format!("{action}: {cause}")
                }
                DispatchError::ConnectorUnavailable(_) => unreachable!(),
            };
            state
                .store
                .append_audit(
                    "action.dispatch_failed",
                    Some(&action),
                    None,
                    None,
                    Some(grant.id),
                    &[],
                    &[],
                )
                .map_err(|audit_err| DispatchError::Resource(anyhow::Error::new(audit_err)))?;
            let suppress_batch = matches!(err, DispatchError::BadRequest(_))
                && surface == FailureSurface::DirectResponse;
            if !suppress_batch {
                batch_failure(
                    state,
                    digest_class,
                    &format!("{action} dispatch failed"),
                    &digest_summary,
                )
                .map_err(|batch_err| DispatchError::Resource(anyhow::Error::new(batch_err)))?;
            }
            Err(err)
        }
    }
}

#[derive(Debug)]
pub(crate) enum DispatchError {
    BadRequest(String),
    Connector(anyhow::Error),
    /// Admission rejected by a genuinely Open/HalfOpen breaker. The distinct
    /// `connector_unavailable` audit event is already recorded by the helper;
    /// `mediate_and_dispatch_action` must not also surface it as a normal
    /// `action.dispatch_failed`.
    ConnectorUnavailable(anyhow::Error),
    /// A write to an external connector timed out after the provider may have
    /// acted (candidate Gmail-write extension): delivery-unknown, not a
    /// confirmed failure. Distinct from `Resource` so callers can fence it.
    DeliveryUnknown(anyhow::Error),
    Resource(anyhow::Error),
}

/// How a mediation caller surfaces dispatch failures to the owner. D-068:
/// an authenticated API caller receives bad requests directly in its typed
/// response, so they are not duplicated into the failure digest. Detached
/// callers (durable workflow adapters) have no direct response surface, so
/// every failure class enters the failure lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailureSurface {
    DirectResponse,
    Detached,
}

/// `lyra.ui.preview`'s real implementation (build plan Step 5, extended by
/// Step 6 / D-043, hardened by D-045): shows the draft to the owner AND, in
/// the same dispatch, proposes it for approval — the two must never drift
/// apart (D-043's whole rationale: a separate propose action could let
/// "what was shown" and "what was proposed" diverge). D-045 extends that
/// guarantee to truncation: `propose_draft_creation` always binds approval
/// to the *full* `preview.body`, so if the message shown to the owner had
/// to be cut short, no approval may be proposed for it at all — the owner
/// must never be able to tap Approve on content they were not shown in
/// full.
///
/// A `Resource`-class `ProposalError` is fatal: it is returned as a typed
/// `DispatchError::Resource` and the outer `post_actions` layer audits and
/// batches it exactly once (`post_actions` already does this for every
/// returned `Resource`/`Connector` error, so this arm must not batch a
/// Resource error itself or it would be double-counted). A `Connector`-class
/// error returns `Ok(sent:true)` (an honest "propose failed, no approval
/// button" rather than a broken button), so it is batched here — and only
/// once the durable digest write succeeds does the preview get shown; if
/// that write fails it escalates to a typed `Resource` error (PI parent
pub(super) async fn dispatch_lyra_preview(
    state: &AppState,
    grant: &TaskGrant,
    action: &ActionId,
    bound_chat_id: i64,
    preview: &PreviewPayload,
) -> Result<Value, DispatchError> {
    let full = format!(
        "Draft preview\nSubject: {}\n\n{}",
        preview.subject, preview.body
    );
    let text = truncate_for_telegram(&full);
    if text != full {
        state
            .store
            .append_audit(
                "draft.proposal_failed",
                Some(&ActionId::new("email.create_draft")),
                None,
                Some("preview_truncated"),
                Some(grant.id),
                &[],
                &[],
            )
            .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?;
        guard_connector_dispatch(state, grant).await?;
        // AD-103/AD-141: admit + bound-timeout the Telegram send at the call
        // site; the helper records breaker health and the D-069 counter.
        call_with_connector(
            state,
            "telegram",
            action,
            grant,
            state
                .connectors
                .telegram()
                .send_reply(bound_chat_id, &truncate_with_notice(&full)),
        )
        .await?;
        return Ok(json!({"sent": true}));
    }
    match propose_draft_creation(state, grant, action, preview).await {
        Ok(action_request_id) => {
            guard_connector_dispatch(state, grant).await?;
            call_with_connector(
                state,
                "telegram",
                action,
                grant,
                state.connectors.telegram().send_reply_with_approval_button(
                    bound_chat_id,
                    &text,
                    action_request_id,
                ),
            )
            .await?;
        }
        Err(ProposalError::GmailUnavailable(c)) => {
            // A genuinely Open Gmail breaker surfaces as `GmailUnavailable`;
            // propagate it as `DispatchError::ConnectorUnavailable` so the
            // outer `mediate_and_dispatch_action` skips its own
            // `action.dispatch_failed` batch (the `connector_unavailable`
            // audit is already recorded by the helper).
            return Err(DispatchError::ConnectorUnavailable(c));
        }
        Err(err) => {
            if err.failure_class() == FailureClass::Resource {
                return Err(DispatchError::Resource(anyhow::Error::new(err)));
            }
            // Connector-class propose failures return `Ok(sent:true)`, so the
            // outer layer never sees an error to batch. Surface them durably
            // here, and only continue to show the preview once the digest
            // write succeeds. If the write fails, escalate to a typed
            // Resource error (the outer layer batches that store failure).
            batch_failure(
                state,
                FailureClass::Connector,
                "lyra.ui.preview proposal failed",
                &err.to_string(),
            )
            .map_err(|surface_err| DispatchError::Resource(anyhow::Error::new(surface_err)))?;
            state
                .store
                .append_audit(
                    "draft.proposal_failed",
                    Some(&ActionId::new("email.create_draft")),
                    None,
                    None,
                    Some(grant.id),
                    &[],
                    &[],
                )
                .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?;
            guard_connector_dispatch(state, grant).await?;
            call_with_connector(
                state,
                "telegram",
                action,
                grant,
                state.connectors.telegram().send_reply(bound_chat_id, &text),
            )
            .await?;
            return Ok(json!({"sent": true}));
        }
    }
    Ok(json!({"sent": true}))
}

/// `email.read_thread:selected_no_attachments`'s real implementation
/// (build plan Step 5): validate the shell's named selection token is
/// bound to *this* grant, atomically consume it (PRD §15 single-use), then
/// fetch the bounded, attachment-free thread from Gmail. Every validation
/// failure here is the shell's own contract violation (a foreign, unknown,
/// expired, wrong-type, or already-used token) — `400`, not `500`; only an
pub(super) async fn dispatch_read_selected_thread(
    state: &AppState,
    grant: &TaskGrant,
    action: &ActionId,
    payload: Option<&Value>,
) -> Result<Value, DispatchError> {
    let payload = payload.ok_or_else(|| {
        DispatchError::BadRequest(
            "email.read_thread:selected_no_attachments requires a payload".to_string(),
        )
    })?;
    let request: ReadThreadPayload = serde_json::from_value(payload.clone()).map_err(|_| {
        DispatchError::BadRequest(
            "email.read_thread:selected_no_attachments payload must be exactly \
             {\"selection_token_id\": string}"
                .to_string(),
        )
    })?;
    let token_id = Ulid::from_str(&request.selection_token_id).map_err(|_| {
        DispatchError::BadRequest("selection_token_id is not a valid id".to_string())
    })?;

    // gate() (in post_actions) has already validated token possession,
    // grant binding, type, and expiry. Re-read the token here only to obtain
    // the target id the Gmail fetch needs (D-055.1: validation now lives in
    // the pure gate, not dispatch).
    let token = state
        .store
        .find_selection_token(token_id)
        .map_err(|err| DispatchError::Resource(err.into()))?
        .ok_or_else(|| DispatchError::BadRequest("unknown selection token".to_string()))?;

    // Atomic single-use consume, post-allow (D-050 / D-055.3). A failed
    // consume is a denial, never a re-ask.
    let consumed = state
        .store
        .try_consume_selection_token(token_id)
        .map_err(|err| DispatchError::Resource(err.into()))?;
    if !consumed {
        return Err(DispatchError::BadRequest(
            "selection token has already been used".to_string(),
        ));
    }

    let gmail = state.connectors.gmail().ok_or_else(|| {
        DispatchError::Connector(anyhow::anyhow!(
            "selection token exists but no gmail connector is configured"
        ))
    })?;
    crate::spend::guard_connector_for(state, grant)
        .await
        .map_err(DispatchError::Resource)?;
    // AD-103/AD-141: admit + bound-timeout the Gmail fetch at the call site;
    // the helper records breaker health and the D-069 counter.
    let thread = call_with_connector(
        state,
        "gmail",
        action,
        grant,
        gmail.fetch_thread(&token.target_id),
    )
    .await?;

    Ok(json!({
        "thread_id": thread.thread_id,
        "messages": thread.messages.iter().map(|m| json!({
            "from": m.from,
            "subject": m.subject,
            "body_text": m.body_text,
        })).collect::<Vec<_>>(),
    }))
}

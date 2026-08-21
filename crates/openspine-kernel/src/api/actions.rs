// openspine:allow-large-module reason: action mediation and dispatch (gate, handler dispatch, lyra preview, approval flow, failure surfacing)
use super::authenticate;
use super::connector_breaker::call_with_connector;
use super::effect_executors::EffectDisposition;
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
use openspine_schemas::briefcase::CounterpartyRef;
use openspine_schemas::digest::canonical_json;
use openspine_schemas::disclosure_policy::PreparedQueryRef;
use openspine_schemas::egress::EgressClass;
use openspine_schemas::escalation::{surface_denial, EscalationEvent};
use openspine_schemas::event::TargetRef;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::owner_surface::OwnerSurfaceRef;
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

/// Responsibility receipt for an effective scoped Allow. This is an
/// admission receipt only: it names the reviewed authority and headroom, not
/// a provider effect outcome.
#[derive(Debug, Serialize)]
pub struct ResponsibilityReceipt {
    pub(crate) rule_id: String,
    pub(crate) rule_version: u32,
    pub(crate) target: Vec<TargetRef>,
    pub(crate) quota_remaining: u32,
    pub(crate) rate_remaining: u32,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) responsibility: Option<ResponsibilityReceipt>,
}
pub(super) async fn post_actions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ActionRequestBody>,
) -> Result<Json<ActionResponseBody>, (StatusCode, Json<Value>)> {
    let (grant, _pending_ref, owner_surface) = authenticate(&state, &headers).await?;
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
            &owner_surface,
            payload.as_ref(),
            FailureSurface::DirectResponse,
            skill_attribution.as_ref(),
            skill_context_token.map(|(id, _)| id),
            None,
            false,
        )
        .await
        .map_err(|err| match &err {
            DispatchError::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, Json(json!({"error": message})))
            }
            DispatchError::NoExecutor(id) => {
                tracing::error!(action = %id.0, "no registered executor");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "internal_error"})),
                )
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

/// Release the consult and fired-exception reservations for a refusal proven
/// to be pre-effect (`NotAttempted`), rearming the fired one-use token only
/// after its budget cancel succeeds. This is the `NotAttempted` settlement
/// primitive: [`settle_reservations`] delegates its cancel arm here, and every
/// pre-dispatch refusal (before any disclosure reservation is minted, and
/// before a disposition object exists) calls it directly. Disclosure
/// reservations are cancelled separately — by [`settle_reservations`] after
/// dispatch, or self-rolled-back inside disclosure enforcement on its own
/// pre-dispatch failures — so they are never live at a direct call here.
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

/// Settle every standing-rule reservation for one dispatched effect from its
/// typed [`EffectDisposition`] alone (T3, #198). This is the single source of
/// truth for finalize/retain/cancel; no caller re-interprets a generic
/// [`DispatchError`] to decide the reservation lifecycle.
///
/// | disposition | consult + disclosure | fired exception |
/// | --- | --- | --- |
/// | `ConfirmedSuccess` | finalize (commit budget) | finalize + mark attempted |
/// | `DeliveryUnknown` | retain `reserved`, note use | note use (no lapse) + mark attempted |
/// | `NotAttempted` / `ConfirmedFailure` | cancel (release budget) | cancel + rearm one-use token |
///
/// `DeliveryUnknown` deliberately retains: releasing budget for a write that
/// may have landed would under-count real effects, and the executor's pending-
/// write fence stays open for later reconciliation (D-157). `NotAttempted` and
/// `ConfirmedFailure` both release, because neither left an effect: nothing was
/// sent, or the provider proved nothing took hold and the executor already
/// resolved its fence.
fn settle_reservations(
    state: &AppState,
    disposition: EffectDisposition,
    consult_reservation: Option<&(String, u32, String)>,
    disclosure_reservations: &[crate::disclosure::DisclosureReservation],
    fired_reservation: Option<&(String, u32, String)>,
    fired_pending: Option<&str>,
    now: Timestamp,
) {
    match disposition {
        EffectDisposition::ConfirmedSuccess => {
            if let Some((rule_id, version, reservation_id)) = consult_reservation {
                if let Err(err) = state.store.finalize_standing_rule_reservation(
                    rule_id,
                    *version,
                    reservation_id,
                    now,
                ) {
                    tracing::error!(error = %err, reservation_id, "standing-rule reservation finalize failed after successful dispatch");
                }
            }
            for (rule_id, version, reservation_id) in disclosure_reservations {
                if let Err(err) = state.store.finalize_standing_rule_reservation(
                    rule_id,
                    *version,
                    reservation_id,
                    now,
                ) {
                    tracing::error!(error = %err, reservation_id, "disclosure reservation finalize failed after successful dispatch");
                }
            }
            if let Some((rule_id, version, reservation_id)) = fired_reservation {
                // D-161: a fired exception is accounted as an exception, so it
                // commits its usage without refreshing the lapse clock.
                if let Err(err) = state.store.finalize_standing_rule_exception_reservation(
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
        }
        EffectDisposition::DeliveryUnknown => {
            // Deliberately no finalize and no cancel: every reservation on this
            // path stays `reserved` and keeps counting against its window. A
            // retained `reserved` row still counts against quota and rate
            // everywhere headroom is computed (`status IN ('reserved','committed')`),
            // so the budget stays conservatively consumed; finalizing it to
            // `committed` would foreclose the later release that a fence
            // reconciler needs (D-157). Each retained rule is still *used*, so
            // refresh its lapse clock and re-evaluate the AD-010 drift trigger.
            for (rule_id, _, _) in consult_reservation
                .into_iter()
                .chain(disclosure_reservations.iter())
            {
                if let Err(use_err) = state.store.note_standing_rule_use(rule_id, now) {
                    tracing::error!(error = %use_err, rule_id, "standing-rule use not recorded after retained dispatch");
                }
            }
            if let Some((rule_id, _, reservation_id)) = fired_reservation {
                // D-161: silence does not refresh the lapse clock, even when its
                // effect outcome is ambiguous.
                if let Err(use_err) = state
                    .store
                    .note_standing_rule_use_with_lapse(rule_id, now, false)
                {
                    tracing::error!(error = %use_err, rule_id, "standing-rule exception use not recorded after retained dispatch");
                }
                let receipt = format!("delivery-unknown:{reservation_id}:{now}");
                if let Err(mark_err) = state
                    .store
                    .mark_fired_effect_attempted(reservation_id, &receipt)
                {
                    tracing::error!(error = %mark_err, reservation_id, "standing-rule fired delivery-unknown attempt not recorded");
                }
            }
        }
        EffectDisposition::NotAttempted | EffectDisposition::ConfirmedFailure => {
            // Neither left an effect: release the reservation and, for fired
            // defaults, rearm the one-use token only after the cancel succeeds.
            cleanup_pre_effect_reservations(
                state,
                consult_reservation,
                fired_reservation,
                fired_pending,
            );
            for (_, _, reservation_id) in disclosure_reservations {
                if let Err(cancel_err) =
                    state.store.cancel_standing_rule_reservation(reservation_id)
                {
                    tracing::error!(error = %cancel_err, reservation_id, "disclosure reservation cancel failed after non-effect dispatch");
                }
            }
        }
    }
}

/// Run the catalog-rated external-egress disclosure hook for one dispatched
/// action and return the D-107 reservations it minted plus an optional payload
/// rewrite for the executor. Two preparation shapes converge on the same
/// connector-agnostic core (`enforce_disclosure_egress`):
///
/// - `DirectMessage` (email/Telegram/future WhatsApp): the composed body is
///   addressed to one verified recipient and is NEVER generalized, so there is
///   no prepared-query token round-trip and no payload rewrite. Provenance is
///   derived from the classified briefcase sections alone.
/// - `Search`/`ForumBrowse`/`WebFormPost`: the free-text query is generalized
///   by redacting sensitive terms, minted/consumed as a one-use prepared query,
///   and the executor receives the generalized query in place of the raw one.
///
/// On any refusal this cleans up the pre-effect consult/fired reservations
/// before returning the same generic kernel-policy denial the worker sees for
/// every disclosure block, so no debug detail leaks.
#[allow(clippy::too_many_arguments)]
async fn enforce_rated_disclosure(
    state: &AppState,
    grant: &TaskGrant,
    action: &ActionId,
    egress_class: EgressClass,
    payload: Option<&Value>,
    consult_reservation: Option<&(String, u32, String)>,
    fired_reservation: Option<&(String, u32, String)>,
    fired_pending: Option<&str>,
) -> Result<(Vec<crate::disclosure::DisclosureReservation>, Option<Value>), DispatchError> {
    let briefcase = match state.store.find_briefcase(grant.id) {
        Ok(Some(briefcase)) => briefcase,
        Ok(None) => {
            cleanup_pre_effect_reservations(
                state,
                consult_reservation,
                fired_reservation,
                fired_pending,
            );
            return Err(DispatchError::BadRequest(
                "rated disclosure was blocked by kernel policy".into(),
            ));
        }
        Err(err) => {
            cleanup_pre_effect_reservations(
                state,
                consult_reservation,
                fired_reservation,
                fired_pending,
            );
            return Err(DispatchError::Resource(anyhow::Error::new(err)));
        }
    };
    let relationship = match briefcase.task_shape.counterparty {
        CounterpartyRef::Bound { relationship, .. } => relationship,
        CounterpartyRef::Unresolved { .. } => {
            cleanup_pre_effect_reservations(
                state,
                consult_reservation,
                fired_reservation,
                fired_pending,
            );
            return Err(DispatchError::BadRequest(
                "rated disclosure was blocked by kernel policy".into(),
            ));
        }
    };

    if egress_class == EgressClass::DirectMessage {
        // Messaging-send preparation: the recipient reads the body verbatim, so
        // it is not generalized and `sensitive_terms` stays empty. The single
        // verified recipient is the bound counterparty; its binding is already
        // validated by the selection-token dispatch path, so only its identity
        // is recorded here. Provenance is enforced over EVERY briefcase section
        // (fail-closed), never only those the body appears to cite.
        let composed_content = payload
            .and_then(|value| value.get("body").or_else(|| value.get("text")))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let recipient = match &briefcase.task_shape.counterparty {
            CounterpartyRef::Bound { identity_id, .. } => identity_id.to_string(),
            CounterpartyRef::Unresolved { .. } => String::new(),
        };
        let request = match crate::disclosure::prepare_messaging_disclosure(
            action.clone(),
            relationship,
            &recipient,
            composed_content,
            &briefcase.sections,
        ) {
            Ok(request) => request,
            Err(crate::disclosure::DisclosureError::Store(err)) => {
                cleanup_pre_effect_reservations(
                    state,
                    consult_reservation,
                    fired_reservation,
                    fired_pending,
                );
                return Err(DispatchError::Resource(anyhow::Error::new(err)));
            }
            Err(_) => {
                // Unclassified worker-visible section: fail closed with the
                // generic denial; detail stays kernel-side.
                cleanup_pre_effect_reservations(
                    state,
                    consult_reservation,
                    fired_reservation,
                    fired_pending,
                );
                return Err(DispatchError::BadRequest(
                    "rated disclosure was blocked by kernel policy".into(),
                ));
            }
        };
        let enforced =
            match crate::disclosure::enforce_disclosure_egress(state, grant, request).await {
                Ok(enforced) => enforced,
                Err(crate::disclosure::DisclosureError::Store(err)) => {
                    cleanup_pre_effect_reservations(
                        state,
                        consult_reservation,
                        fired_reservation,
                        fired_pending,
                    );
                    return Err(DispatchError::Resource(anyhow::Error::new(err)));
                }
                Err(crate::disclosure::DisclosureError::BudgetExhausted(_)) => {
                    cleanup_pre_effect_reservations(
                        state,
                        consult_reservation,
                        fired_reservation,
                        fired_pending,
                    );
                    return Err(DispatchError::BadRequest(
                        "rated disclosure was blocked by kernel policy".into(),
                    ));
                }
                Err(_) => {
                    cleanup_pre_effect_reservations(
                        state,
                        consult_reservation,
                        fired_reservation,
                        fired_pending,
                    );
                    return Err(DispatchError::BadRequest(
                        "rated disclosure was blocked by kernel policy".into(),
                    ));
                }
            };
        return Ok((enforced.reservations, None));
    }

    let selected_keys = payload
        .and_then(|value| value.get("briefcase_sections"))
        .and_then(Value::as_array)
        .map(|keys| {
            keys.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_default();
    if selected_keys.is_empty() {
        cleanup_pre_effect_reservations(
            state,
            consult_reservation,
            fired_reservation,
            fired_pending,
        );
        return Err(DispatchError::BadRequest(
            "rated disclosure was blocked by kernel policy".into(),
        ));
    }
    let selected_sections = briefcase
        .sections
        .iter()
        .filter(|section| selected_keys.contains(&section.key))
        .cloned()
        .collect::<Vec<_>>();
    if selected_sections.is_empty() {
        cleanup_pre_effect_reservations(
            state,
            consult_reservation,
            fired_reservation,
            fired_pending,
        );
        return Err(DispatchError::BadRequest(
            "rated disclosure was blocked by kernel policy".into(),
        ));
    }
    let kernel_provenance = match crate::disclosure::provenance_from_sections(&briefcase.sections) {
        Ok(provenance) => provenance,
        Err(crate::disclosure::DisclosureError::Store(err)) => {
            cleanup_pre_effect_reservations(
                state,
                consult_reservation,
                fired_reservation,
                fired_pending,
            );
            return Err(DispatchError::Resource(anyhow::Error::new(err)));
        }
        Err(_) => {
            // Unclassified worker-visible section: fail closed with the
            // generic denial; detail stays kernel-side.
            cleanup_pre_effect_reservations(
                state,
                consult_reservation,
                fired_reservation,
                fired_pending,
            );
            return Err(DispatchError::BadRequest(
                "rated disclosure was blocked by kernel policy".into(),
            ));
        }
    };
    // A composed payload may carry a previously minted token. Otherwise
    // this kernel boundary mints it from the selected, classified sections;
    // caller-supplied sensitivity terms are never accepted. Redaction is
    // always derived from every private/sensitive section in the grant.
    let prepared_ref = match payload
        .and_then(|value| value.get("prepared_query"))
        .and_then(|value| serde_json::from_value::<PreparedQueryRef>(value.clone()).ok())
    {
        Some(reference) => reference,
        None => {
            let raw_query = match payload
                .and_then(|value| value.get("query"))
                .and_then(Value::as_str)
            {
                Some(query) => query,
                None => {
                    cleanup_pre_effect_reservations(
                        state,
                        consult_reservation,
                        fired_reservation,
                        fired_pending,
                    );
                    return Err(DispatchError::BadRequest(
                        "rated disclosure requires a kernel-prepared generalized query".into(),
                    ));
                }
            };
            match crate::disclosure::prepare_disclosure_query(
                state,
                grant.id,
                action.clone(),
                raw_query.to_string(),
                relationship,
                egress_class,
                &briefcase.sections,
            )
            .await
            {
                Ok(reference) => reference,
                Err(crate::disclosure::DisclosureError::Store(err)) => {
                    cleanup_pre_effect_reservations(
                        state,
                        consult_reservation,
                        fired_reservation,
                        fired_pending,
                    );
                    // DB failure during preparation is infrastructure,
                    // not caller input: keep it in the kernel error lane.
                    return Err(DispatchError::Resource(anyhow::Error::new(err)));
                }
                Err(_) => {
                    cleanup_pre_effect_reservations(
                        state,
                        consult_reservation,
                        fired_reservation,
                        fired_pending,
                    );
                    return Err(DispatchError::BadRequest(
                        "rated disclosure preparation was rejected by kernel policy".into(),
                    ));
                }
            }
        }
    };
    let prepared = match state.store.consume_prepared_query(&prepared_ref) {
        Ok(Some(prepared)) => prepared,
        Ok(None) => {
            cleanup_pre_effect_reservations(
                state,
                consult_reservation,
                fired_reservation,
                fired_pending,
            );
            return Err(DispatchError::BadRequest(
                "prepared disclosure query is missing, stale, or already consumed".into(),
            ));
        }
        Err(err) => {
            cleanup_pre_effect_reservations(
                state,
                consult_reservation,
                fired_reservation,
                fired_pending,
            );
            return Err(DispatchError::Resource(anyhow::Error::new(err)));
        }
    };
    if !prepared.binding_matches(
        action,
        relationship,
        egress_class,
        grant.id,
        &kernel_provenance,
    ) {
        cleanup_pre_effect_reservations(
            state,
            consult_reservation,
            fired_reservation,
            fired_pending,
        );
        return Err(DispatchError::BadRequest(
            "prepared disclosure query binding mismatch".into(),
        ));
    }
    if prepared.digest != prepared_ref.digest {
        cleanup_pre_effect_reservations(
            state,
            consult_reservation,
            fired_reservation,
            fired_pending,
        );
        return Err(DispatchError::BadRequest(
            "prepared disclosure query digest mismatch".into(),
        ));
    }
    let enforced = match crate::disclosure::enforce_disclosure_egress(
        state,
        grant,
        crate::disclosure::DisclosureRequest {
            raw_query: prepared.generalized_query.clone(),
            sensitive_terms: Default::default(),
            action_id: action.clone(),
            relationship,
            provenance: kernel_provenance,
        },
    )
    .await
    {
        Ok(enforced) => enforced,
        Err(crate::disclosure::DisclosureError::Store(err)) => {
            cleanup_pre_effect_reservations(
                state,
                consult_reservation,
                fired_reservation,
                fired_pending,
            );
            // Infrastructure failure, not caller input: keep it in the
            // kernel error lane so failure batching sees it.
            return Err(DispatchError::Resource(anyhow::Error::new(err)));
        }
        Err(crate::disclosure::DisclosureError::BudgetExhausted(_)) => {
            cleanup_pre_effect_reservations(
                state,
                consult_reservation,
                fired_reservation,
                fired_pending,
            );
            // AD-151 policy-free outcome: the worker must not learn that a
            // standing policy exists or its budget state. Exhaustion detail
            // lives only in the disclosure.budget_exhausted audit.
            return Err(DispatchError::BadRequest(
                "rated disclosure was blocked by kernel policy".into(),
            ));
        }
        Err(_) => {
            cleanup_pre_effect_reservations(
                state,
                consult_reservation,
                fired_reservation,
                fired_pending,
            );
            return Err(DispatchError::BadRequest(
                "rated disclosure was blocked by kernel policy".into(),
            ));
        }
    };
    let generalized = enforced.query.generalized_query.clone();
    Ok((
        enforced.reservations,
        Some(json!({ "generalized_query": generalized })),
    ))
}

/// Shared mediation boundary for HTTP and durable workflow actions.
pub(crate) async fn mediate_and_dispatch_action(
    state: &AppState,
    grant: &TaskGrant,
    action: ActionId,
    owner_surface: &OwnerSurfaceRef,
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
        owner_surface,
        payload,
        surface,
        None,
        None,
        fired_pending,
        false,
    )
    .await
}

/// Headless-lane variant: `headless: true` prevents a standing rule from
/// downgrading a mandatory owner-approval decision (`ApprovalRequired`) to
/// `Allow` — the headless lane must always surface escalation rather than
/// silently self-approve.
pub(crate) async fn mediate_and_dispatch_action_headless(
    state: &AppState,
    grant: &TaskGrant,
    action: ActionId,
    owner_surface: &OwnerSurfaceRef,
    payload: Option<&Value>,
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
        owner_surface,
        payload,
        FailureSurface::Detached,
        None,
        None,
        None,
        true,
    )
    .await
}

#[cfg(test)]
pub(crate) async fn mediate_and_dispatch_action_with_attribution(
    state: &AppState,
    grant: &TaskGrant,
    action: ActionId,
    owner_surface: &OwnerSurfaceRef,
    payload: Option<&Value>,
    surface: FailureSurface,
    skill_attribution: Option<&SkillAttribution>,
) -> Result<(GateDecision, Option<String>, Option<Value>), DispatchError> {
    let (decision, deferral, result, _budget) =
        mediate_and_dispatch_action_with_attribution_and_token(
            state,
            grant,
            action,
            owner_surface,
            payload,
            surface,
            skill_attribution,
            None,
            None,
            false,
        )
        .await?;
    Ok((decision, deferral, result))
}

#[allow(clippy::too_many_arguments)]
async fn mediate_and_dispatch_action_with_attribution_and_token(
    state: &AppState,
    grant: &TaskGrant,
    action: ActionId,
    owner_surface: &OwnerSurfaceRef,
    payload: Option<&Value>,
    surface: FailureSurface,
    skill_attribution: Option<&SkillAttribution>,
    skill_context_token: Option<Ulid>,
    fired_pending: Option<&str>,
    headless: bool,
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
                now,
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
    let mut disclosure_reservations: Vec<crate::disclosure::DisclosureReservation> = Vec::new();
    // #128: for an action whose kernel-resolved context is constructible, the
    // authority that admits an effect is the reviewed scope the owner
    // approved, not the bare action key. Resolution happens here — before any
    // standing-rule consultation, any reservation, and any timer — and only
    // on the normal path (a fired dark-window default already carries its own
    // digest-bound waiver, and the headless lane re-gates its own approval).
    let scoped = if fired_pending.is_none()
        && !headless
        && matches!(decision, GateDecision::ApprovalRequired { .. })
    {
        super::scoped_admission::resolve_scoped_admission(
            state,
            grant,
            &action,
            payload_ref.as_ref(),
            now,
        )
        .await?
    } else {
        super::scoped_admission::ScopedAdmission::NotApplicable
    };
    // A construction failure consults no rule at all: falling back to the
    // action-keyed path would admit an effect that no reviewed scope covers.
    // The failure is already audited; the decision stays ApprovalRequired.
    let (scoped_resolved, scoped_unresolvable) = match scoped {
        super::scoped_admission::ScopedAdmission::Resolved(resolved) => (Some(resolved), false),
        super::scoped_admission::ScopedAdmission::Unresolvable => (None, true),
        super::scoped_admission::ScopedAdmission::NotApplicable => (None, false),
    };
    // Set only when a scoped rule admitted this effect: dispatch then goes to
    // the shared executor with the kernel-resolved, digest-bound request
    // rather than through the generic shell dispatch path.
    let mut scoped_admitted: Option<Box<super::scoped_admission::ScopedAdmissionContext>> = None;
    let fired_counterparty_erased = if fired_pending.is_some() {
        let briefcase = state
            .store
            .find_briefcase(grant.id)
            .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?;
        let erased = match briefcase
            .as_ref()
            .map(|briefcase| &briefcase.task_shape.counterparty)
        {
            Some(CounterpartyRef::Bound { identity_id, .. }) => state
                .store
                .is_counterparty_erased(*identity_id)
                .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?,
            _ => false,
        };
        if erased {
            state
                .store
                .append_audit(
                    "action.scope_context_unresolved",
                    Some(&action),
                    None,
                    Some(
                        "counterparty was erased at fired-pending mediation; ordinary owner review required",
                    ),
                    Some(grant.id),
                    &[],
                    &[],
                )
                .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?;
        }
        erased
    } else {
        false
    };
    if let Some(token) = fired_pending {
        // Fired dark-window default (owner silence): re-dispatch this action
        // with a digest-bound, one-use token. Consume it *before* the effect
        // so the re-dispatch is over-budget only against the fired waiver; the
        // effective Allow is audited only after the token is admitted, and the
        // reservation is finalized on success or cancelled on failure
        // (P1-5/P1-6/P1-11).
        if !fired_counterparty_erased && matches!(decision, GateDecision::ApprovalRequired { .. }) {
            if let Ok(Some((rule_id, version, reservation_id))) =
                state.store.consume_standing_rule_fired_pending(
                    token,
                    &action,
                    grant.id,
                    owner_surface,
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
    } else if let Some(resolved) = scoped_resolved {
        // Test-only deterministic seam (#177): the window between the
        // pre-transaction resolution read (`resolve_scoped_admission`, above)
        // and the reservation transaction opened inside `consult_scoped_rule`.
        // A reachability test arms a one-shot hook here to commit a
        // counterparty-erasure marker the in-transaction recheck in
        // `consult_and_reserve_scoped_rule` must observe; read-and-cleared, so
        // production builds carry neither the field nor this branch.
        #[cfg(test)]
        if let Some(hook) = state.pre_reserve_erasure_hook.lock().take() {
            hook();
        }
        // Scope-matched admission (#128). The selection, its audit, and its
        // headroom live in `scoped_admission` so this module does not grow a
        // second admission policy inline.
        let admitted = super::scoped_admission::consult_scoped_rule(
            state,
            grant,
            &action,
            &resolved,
            payload_ref.as_slice(),
            now,
        )?;
        let responsibility =
            admitted
                .reservation
                .as_ref()
                .map(|(rule_id, version, _)| ResponsibilityReceipt {
                    rule_id: rule_id.clone(),
                    rule_version: *version,
                    target: resolved.context.target_refs().to_vec(),
                    quota_remaining: admitted.quota_remaining,
                    rate_remaining: admitted.rate_remaining,
                });
        consult_reservation = admitted.reservation;
        if admitted.allow {
            decision = GateDecision::Allow;
            standing_budget = Some(StandingRuleBudgetInfo {
                quota_remaining: admitted.quota_remaining,
                rate_remaining: admitted.rate_remaining,
                // A scoped consultation never schedules a dark-window timer:
                // selection completes before scheduling would be reached, and
                // `email.create_draft` prohibits dark windows by policy.
                dark_window_scheduled: false,
                responsibility,
            });
            scoped_admitted = Some(resolved);
        }
    } else if !headless
        && !scoped_unresolvable
        && matches!(decision, GateDecision::ApprovalRequired { .. })
    {
        // Normal path: an active, non-expired, non-revoked rule covers this
        // action and still has budget → reserve it atomically and allow
        // without a fresh owner approval; otherwise keep ApprovalRequired and,
        // if a dark window is configured, let the gate schedule its timer.
        let ctx = PendingScheduleCtx {
            owner_surface: owner_surface.clone(),
            grant_id: grant.id,
            payload_ref: payload_ref.clone(),
            fingerprint: standing_rule_fingerprint(&action, grant.id, owner_surface, &payload_ref),
            // This is the action-keyed path: the rule carries no reviewed
            // scope, so there is no resolved context to bind the exception to.
            // The per-rule-version cap still applies.
            reviewed_scope_digest: None,
            compatibility_digest: None,
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
                    responsibility: None,
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
                                    owner_surface,
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
    let mut prepared_payload = payload.cloned();
    if let Some(egress_class) = state
        .action_catalog
        .egress_decl_for(&action)
        .and_then(|decl| decl.egress_class)
    {
        let (reservations, rewritten) = enforce_rated_disclosure(
            state,
            grant,
            &action,
            egress_class,
            payload,
            consult_reservation.as_ref(),
            fired_reservation.as_ref(),
            fired_pending,
        )
        .await?;
        disclosure_reservations = reservations;
        if let Some(rewritten) = rewritten {
            prepared_payload = Some(rewritten);
        }
    }

    // A scope-matched admission is the third caller of the shared
    // `gmail.create_draft` executor (#128 task 6): it hands over the
    // kernel-resolved, digest-bound request, never the shell's opaque
    // payload. Every other action keeps the generic dispatch path, which
    // still refuses to reconstruct a digest-bound context and still fails
    // closed exactly as #127 left it.
    let dispatched = match scoped_admitted.as_deref() {
        Some(admission) => {
            super::scoped_admission::dispatch_scoped_effect(state, grant, admission, owner_surface)
                .await
        }
        None => {
            super::connector_breaker::dispatch_allowed_action(
                state,
                grant,
                &action,
                owner_surface,
                prepared_payload.as_ref(),
            )
            .await
        }
    };
    // T3 (#198): settlement is determined SOLELY by the typed disposition the
    // dispatch path already computed at the provider boundary. No caller-side
    // re-interpretation of a generic error decides finalize/retain/cancel.
    settle_reservations(
        state,
        dispatched.disposition,
        consult_reservation.as_ref(),
        &disclosure_reservations,
        fired_reservation.as_ref(),
        fired_pending,
        now,
    );
    let executor_recorded = dispatched.executor_recorded;
    match dispatched.result {
        Ok(result) => Ok((GateDecision::Allow, None, Some(result), standing_budget)),
        Err(err) => {
            // AD-103: a `ConnectorUnavailable` (Open/HalfOpen breaker) already
            // appended the distinct `connector_unavailable` audit; do not also
            // record `action.dispatch_failed` or batch it (that would
            // double-count the operational outage). Settlement already ran
            // above (the breaker rejection is `NotAttempted`, so the
            // reservation was released).
            if matches!(err, DispatchError::ConnectorUnavailable(_)) {
                return Err(err);
            }
            // #244: when the dispatch executor already owns this effect's
            // record (it self-appended its typed audit event and, for a
            // failure, self-batched it into the owner digest), the mediation
            // handler must not re-append `action.dispatch_failed` or re-call
            // `batch_failure`. Doing so double-counts one scoped effect and
            // files a pre-effect `NotAttempted` refusal under the wrong,
            // Connector-class vocabulary. Settlement already ran above off the
            // typed disposition, so the reservation is resolved regardless.
            if executor_recorded {
                return Err(err);
            }
            let digest_class = match &err {
                DispatchError::Resource(_) | DispatchError::DeliveryUnknown(_) => {
                    FailureClass::Resource
                }
                DispatchError::Connector(_)
                | DispatchError::BadRequest(_)
                | DispatchError::NoExecutor(_) => FailureClass::Connector,
                DispatchError::ConnectorUnavailable(_) => unreachable!(),
            };
            let digest_summary = match &err {
                DispatchError::BadRequest(msg) => msg.clone(),
                DispatchError::NoExecutor(id) if state.is_execution_backed(id) => {
                    format!("{id}: executor registered but not reachable on this path")
                }
                DispatchError::NoExecutor(id) => format!("{id}: no registered executor"),
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

/// One dispatched effect's outcome, carrying BOTH the typed
/// [`EffectDisposition`] that drives reservation settlement (T3, #198) AND the
/// caller-facing [`Result`] surface that other layers audit, batch, and return.
/// The two are produced together at the provider boundary — the scoped executor
/// returns its disposition directly, and the generic connector path classifies
/// its own outcome — so settlement reads `disposition` alone and never
/// re-interprets `result` to decide finalize/retain/cancel.
pub(crate) struct DispatchedEffect {
    pub(crate) disposition: EffectDisposition,
    pub(crate) result: Result<Value, DispatchError>,
    /// The dispatch path's executor already appended its own audit event and,
    /// for a failure, already self-batched it into the owner digest. When set,
    /// the mediation error handler MUST NOT re-append `action.dispatch_failed`
    /// or re-call `batch_failure` — doing so double-counts one effect. The
    /// scoped executor owns the record for every disposition it returns; the
    /// generic path and the un-recorded scoped arms (registry miss, executor
    /// error) leave this false so the handler records them once.
    pub(crate) executor_recorded: bool,
}

#[derive(Debug)]
pub(crate) enum DispatchError {
    /// A catalogued action reached dispatch with no runnable executor and no
    /// non-effect stub declaration. Fail closed: never a successful stub.
    NoExecutor(ActionId),
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
    owner_surface: &OwnerSurfaceRef,
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
                .send_reply(owner_surface, &truncate_with_notice(&full)),
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
                    owner_surface,
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
                state.connectors.telegram().send_reply(owner_surface, &text),
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

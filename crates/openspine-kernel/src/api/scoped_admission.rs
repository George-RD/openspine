//! Kernel-boundary construction of the resolved action context, and the
//! third caller of the shared `gmail.create_draft` executor
//! (mine-and-match-reusable-authority-by-scope, #128, tasks 3/5/6).
//!
//! The shell supplies an *intent* (subject/body). Every value that enters the
//! reviewed scope is resolved here, kernel-side: the connector instance and
//! account role come from the kernel-authored selection token, the account
//! identity from the configured Gmail mailbox, the canonical target refs and
//! `target_digest` from a read-only Gmail thread fetch, and the bound
//! counterparty, workflow, and task shape from the grant's briefcase. A shell
//! payload can never supply `target_ref`, `target_digest`, account identity,
//! or counterparty — a shell-chosen scope would be self-granted authority.
//!
//! A read-only fetch before standing-rule consultation is deliberate and
//! safe: reads are not effects, consume no standing-rule budget, and schedule
//! no timer.
//!
//! Every construction failure — no registered descriptor, unresolvable
//! connector or account, missing required dimension, unbound counterparty —
//! is [`ScopedAdmission::Unresolvable`]: the decision stays
//! `ApprovalRequired`, nothing is dispatched, and a durable audit event names
//! the failure. The kernel never remaps a rule onto a successor connector or
//! account; an unresolvable one is a construction failure, not a
//! substitution (task 5).
//!
//! The generic shell dispatch path is untouched: it still receives an opaque
//! `payload: Option<&Value>`, still refuses to reconstruct a digest-bound
//! context from it, and still fails closed exactly as #127 left it.

use anyhow::anyhow;
use jiff::Timestamp;
use openspine_schemas::action::{ActionId, ActionRequest, GateDecision};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::briefcase::CounterpartyRef;
use openspine_schemas::digest::digest_of;
use openspine_schemas::event::{Connector, TargetRef, TargetRefKind};
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::resolved_context::{ResolvedActionContext, ResolvedActionContextInput};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use ulid::Ulid;

use super::actions::DispatchError;
use super::connector_breaker::call_with_connector;
use super::effect_executors::EffectOutcome;
use crate::pipeline::AppState;

/// The kernel-resolved context for one action plus the digest-bound request
/// the shared executor is handed if a scoped rule admits it.
pub(crate) struct ScopedAdmissionContext {
    pub(crate) context: ResolvedActionContext,
    pub(crate) request: ActionRequest,
}

/// Whether scoped admission applies to this action, and if so whether its
/// context could be resolved.
pub(crate) enum ScopedAdmission {
    /// The action has no registered Gmail draft implementation descriptor, so
    /// `ResolvedActionContext::try_new` could never succeed for it. The
    /// action-keyed standing-rule path is used unchanged.
    NotApplicable,
    /// Scoped admission applies but the context could not be constructed. The
    /// failure is audited; the caller must leave the decision at
    /// `ApprovalRequired` and consult no rule at all — falling back to the
    /// action-keyed path would admit an effect no reviewed scope covers.
    Unresolvable,
    /// A sealed, kernel-resolved context ready for scoped consultation.
    Resolved(Box<ScopedAdmissionContext>),
}

fn unresolvable(
    state: &AppState,
    grant: &TaskGrant,
    action: &ActionId,
    reason: &str,
) -> Result<ScopedAdmission, DispatchError> {
    state
        .store
        .append_audit(
            "action.scope_context_unresolved",
            Some(action),
            None,
            Some(reason),
            Some(grant.id),
            &[],
            &[],
        )
        .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?;
    Ok(ScopedAdmission::Unresolvable)
}

/// Resolve the kernel-owned context for `action`, or explain why it cannot be
/// resolved. Called before any standing-rule consultation.
pub(crate) async fn resolve_scoped_admission(
    state: &AppState,
    grant: &TaskGrant,
    action: &ActionId,
    payload_ref: Option<&ArtifactRef>,
    now: Timestamp,
) -> Result<ScopedAdmission, DispatchError> {
    // Scoped wiring is bound to the one registered implementation descriptor
    // (`gmail.draft.v1` → executor `gmail.create_draft`). This is a catalog
    // lookup, not a hardcoded action id: a second action becomes eligible by
    // registering its descriptor and resolver, which is a catalog change.
    let Some(implementation) = state
        .action_catalog
        .implementation_descriptor_for_action(action)
    else {
        return Ok(ScopedAdmission::NotApplicable);
    };
    if implementation.connector_kind != "gmail"
        || implementation.executor_id != "gmail.create_draft"
    {
        return Ok(ScopedAdmission::NotApplicable);
    }
    let implementation_id = implementation.implementation_id.clone();

    // From here every failure is a construction failure and fails closed.
    let Some(payload_ref) = payload_ref.cloned() else {
        return unresolvable(
            state,
            grant,
            action,
            "intent payload is not content-addressed; no payload digest to bind",
        );
    };
    let Some(gmail) = state.connectors.gmail() else {
        return unresolvable(
            state,
            grant,
            action,
            "no gmail connector is configured; the reviewed connector instance is unresolvable",
        );
    };

    // Connector instance and account role come from the kernel-authored
    // selection token, never from the shell.
    let Some(token_id) = grant.selection_tokens.first().copied() else {
        return unresolvable(
            state,
            grant,
            action,
            "grant carries no kernel-authored selection token",
        );
    };
    let token = state
        .store
        .find_selection_token(token_id)
        .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?;
    let Some(token) = token else {
        return unresolvable(
            state,
            grant,
            action,
            "selection token not found in the store",
        );
    };
    if !token.verified_source || token.expires_at <= now {
        return unresolvable(
            state,
            grant,
            action,
            "selection token is expired or not owner-verified",
        );
    }
    let Some(Connector::GmailPrimaryConnector) = token.connector else {
        return unresolvable(
            state,
            grant,
            action,
            "selection token names no gmail connector instance",
        );
    };
    let connector_instance_id = serde_json::to_value(Connector::GmailPrimaryConnector)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    let Some(account_role) = token.account_role else {
        return unresolvable(
            state,
            grant,
            action,
            "selection token names no account role",
        );
    };
    let account_identity_digest = digest_of(&json!({
        "connector_instance_id": connector_instance_id,
        "account_role": account_role,
        "mailbox_address": gmail.mailbox_address(),
    }));

    // The bound counterparty and task shape are the kernel's own, from the
    // briefcase this grant was packed with.
    let briefcase = state
        .store
        .find_briefcase(grant.id)
        .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?;
    let Some(briefcase) = briefcase else {
        return unresolvable(
            state,
            grant,
            action,
            "grant has no briefcase; the counterparty is unbound",
        );
    };
    if matches!(
        briefcase.task_shape.counterparty,
        CounterpartyRef::Unresolved { .. }
    ) {
        return unresolvable(
            state,
            grant,
            action,
            "briefcase counterparty is unresolved; reusable delegation requires an identity-bound counterparty",
        );
    }
    let task_shape_digest = digest_of(
        &serde_json::to_value(&briefcase.task_shape)
            .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?,
    );

    // Canonical target refs and the target digest come from a read-only Gmail
    // thread fetch. A read is not an effect: it consumes no standing-rule
    // budget and schedules no timer.
    if crate::spend::guard_connector_for(state, grant)
        .await
        .is_err()
    {
        return unresolvable(
            state,
            grant,
            action,
            "connector spend guard blocked the read-only target resolution",
        );
    }
    let thread = match call_with_connector(
        state,
        "gmail",
        action,
        grant,
        gmail.fetch_thread(&token.target_id),
    )
    .await
    {
        Ok(thread) => thread,
        Err(_) => {
            return unresolvable(
                state,
                grant,
                action,
                "gmail thread fetch failed; the reviewed target is unresolvable",
            );
        }
    };
    let Some(recipient) =
        crate::gmail::newest_non_owner_recipient(&thread, gmail.mailbox_address())
    else {
        return unresolvable(
            state,
            grant,
            action,
            "no non-owner recipient in the target thread",
        );
    };
    let target_digest = digest_of(&json!({
        "thread_id": token.target_id,
        "connector": "gmail_primary",
        "account_role": "owner_mailbox",
        "recipients": [recipient.recipient],
    }));
    // The reviewed scope must move when the people on the thread move, not
    // only when the thread id does. `target_digest` seals the recipient the
    // draft is addressed to; this seals the wider set of participants the
    // kernel can observe (every distinct sender in the fetched thread), so a
    // new participant posting into a reviewed thread changes the scope key
    // and returns the action to ordinary owner approval. Both are bound
    // dimensions on this descriptor. Kernel-resolved from the read-only
    // fetch — never shell-supplied.
    let participants = thread
        .messages
        .iter()
        .map(|message| message.from.trim())
        .filter(|from| !from.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(",");
    let mut bound_parameters = BTreeMap::new();
    bound_parameters.insert("thread_participants".to_string(), participants);
    let target_ref = TargetRef {
        kind: TargetRefKind::EmailThread,
        id: Some(token.target_id.clone()),
    };

    let input = ResolvedActionContextInput {
        connector_instance_id,
        account_role: Some(account_role),
        account_identity_digest: Some(account_identity_digest),
        target_refs: vec![target_ref.clone()],
        counterparty: Some(briefcase.task_shape.counterparty.clone()),
        // Kernel-resolved only: the thread participant set from the read-only
        // fetch above. A shell payload can never contribute a bound
        // parameter — that would be the one scope input it could steer, and a
        // shell-chosen scope is self-granted authority.
        bound_parameters,
        target_digest: Some(target_digest.clone()),
        payload_digest: Some(payload_ref.digest.clone()),
        workflow_id: Some(grant.workflow_id.clone()),
        task_shape_digest: Some(task_shape_digest),
    };
    let context = match ResolvedActionContext::try_new(
        &state.action_catalog,
        action,
        &implementation_id,
        input,
    ) {
        Ok(context) => context,
        Err(err) => {
            return unresolvable(state, grant, action, &format!("{err}"));
        }
    };

    // The digest-bound request handed to the shared executor. Its
    // re-derivations run against these kernel-resolved values, exactly as for
    // the per-instance approval and headless approved lanes.
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: action.clone(),
        target_ref: Some(target_ref),
        payload_ref: Some(payload_ref),
        target_digest: Some(target_digest),
        selection_token_id: None,
        params: Default::default(),
        skill_attribution: None,
        requested_at: now,
        schema_version: 1,
    };
    Ok(ScopedAdmission::Resolved(Box::new(
        ScopedAdmissionContext { context, request },
    )))
}

/// The outcome of a scoped consultation, reduced to what the mediation
/// boundary needs: whether the reviewed scope admitted the effect, the
/// reservation to finalize or cancel, and the Allow-only headroom.
pub(crate) struct ScopedAdmissionDecision {
    pub(crate) allow: bool,
    pub(crate) reservation: Option<(String, u32, String)>,
    pub(crate) quota_remaining: u32,
    pub(crate) rate_remaining: u32,
}

/// Select exactly one compatible scoped rule for `resolved` and, if one
/// admits, record the admission.
///
/// Selection is pure and completes before any budget moves, inside the store's
/// `BEGIN IMMEDIATE` so a concurrent activation cannot swap the chosen rule: 0
/// compatible rules → ordinary owner approval, exactly 1 → admit, 2+ → fail
/// closed with no tie-break by recency, narrowness, or ordering (any tie-break
/// is a policy the owner never reviewed). Budgets stay strictly per rule;
/// there is no aggregate per-action counter.
///
/// The `action.gated` audit names the admitting rule id, its version, and both
/// digests, so an auditor can reconstruct which reviewed responsibility spent
/// which budget. If that durable write fails the reservation this function
/// minted is cancelled before the error propagates — it owns that reservation
/// until it returns.
pub(crate) fn consult_scoped_rule(
    state: &AppState,
    grant: &TaskGrant,
    action: &ActionId,
    resolved: &ScopedAdmissionContext,
    payload_refs: &[ArtifactRef],
    now: Timestamp,
) -> Result<ScopedAdmissionDecision, DispatchError> {
    let consult = state
        .store
        .consult_and_reserve_scoped_rule(&resolved.context, now)
        .map_err(|err| DispatchError::Resource(anyhow::Error::new(err)))?;
    let reservation = match (consult.rule.as_ref(), consult.reservation_id.as_ref()) {
        (Some(rule), Some(reservation_id)) => {
            Some((rule.rule_id.clone(), rule.version, reservation_id.clone()))
        }
        _ => None,
    };
    if !consult.allow {
        return Ok(ScopedAdmissionDecision {
            allow: false,
            reservation,
            quota_remaining: 0,
            rate_remaining: 0,
        });
    }
    let note = consult
        .rule
        .as_ref()
        .map(|rule| {
            format!(
                "scoped standing-rule effective Allow admitted before effect \
                 (rule {} v{}, compatibility epoch {}, reviewed scope {})",
                rule.rule_id,
                rule.version,
                resolved.context.compatibility_digest(),
                resolved
                    .context
                    .reviewed_scope_digest()
                    .map_or_else(|| "unavailable".to_string(), |digest| digest.to_string()),
            )
        })
        .unwrap_or_default();
    if let Err(err) = state.store.append_audit(
        "action.gated",
        Some(action),
        Some(&GateDecision::Allow),
        Some(&note),
        Some(grant.id),
        &[],
        payload_refs,
    ) {
        if let Some((_, _, reservation_id)) = reservation.as_ref() {
            if let Err(cancel_err) = state.store.cancel_standing_rule_reservation(reservation_id) {
                tracing::error!(
                    error = %cancel_err,
                    reservation_id,
                    "scoped reservation cancel failed after admission audit failure"
                );
            }
        }
        return Err(DispatchError::Resource(anyhow::Error::new(err)));
    }
    // Headroom is returned only on an authorized Allow (AD-013/AD-106): a
    // denial must never expose remaining-capacity metadata.
    let (quota_remaining, rate_remaining) = consult.budget_info().unwrap_or((0, 0));
    Ok(ScopedAdmissionDecision {
        allow: true,
        reservation,
        quota_remaining,
        rate_remaining,
    })
}

/// Run the shared `gmail.create_draft` executor for a scope-matched
/// admission and map its truthful [`EffectOutcome`] onto the dispatch result
/// the caller's reservation lifecycle already keys off:
///
/// | outcome | dispatch result | reservation |
/// | --- | --- | --- |
/// | `Executed` | `Ok` | finalize |
/// | `DeliveryUnknown` | `DeliveryUnknown` | retain, fence left open |
/// | `RefusedPreEffect` | `Connector` | cancel |
/// | `FailedAfterAttempt` | `Connector` | cancel |
/// | registry miss | `NoExecutor` | cancel |
///
/// Retaining on `DeliveryUnknown` is the deliberately conservative
/// direction: releasing budget for a write that may have landed would
/// under-count real effects, whereas retaining over-counts at worst. An
/// executor error whose effect ordering is unknown maps to `Resource`, which
/// the caller also retains.
pub(crate) async fn dispatch_scoped_effect(
    state: &AppState,
    grant: &TaskGrant,
    admission: &ScopedAdmissionContext,
    bound_chat_id: i64,
) -> Result<Value, DispatchError> {
    let Some(executor) = state
        .effect_executors
        .lookup(admission.context.executor_id())
    else {
        return Err(DispatchError::NoExecutor(admission.request.action.clone()));
    };
    match executor(state, grant, &admission.request, bound_chat_id).await {
        Ok(EffectOutcome::Executed) => Ok(json!({"created": true})),
        Ok(EffectOutcome::DeliveryUnknown) => Err(DispatchError::DeliveryUnknown(anyhow!(
            "gmail draft write outcome is unknown; the reconciliation fence stays open"
        ))),
        Ok(EffectOutcome::RefusedPreEffect) => Err(DispatchError::Connector(anyhow!(
            "scope-matched draft creation was refused before any provider write"
        ))),
        Ok(EffectOutcome::FailedAfterAttempt) => Err(DispatchError::Connector(anyhow!(
            "scope-matched draft creation failed after an attempted write"
        ))),
        Err(err) => Err(DispatchError::Resource(err)),
    }
}

#[cfg(test)]
#[path = "scoped_admission_drift_tests.rs"]
mod drift_tests;
#[cfg(test)]
#[path = "scoped_admission_outcome_tests.rs"]
mod outcome_tests;
#[cfg(test)]
#[path = "scoped_admission_support.rs"]
pub(super) mod scoped_admission_support;
#[cfg(test)]
#[path = "scoped_admission_tests.rs"]
mod tests;

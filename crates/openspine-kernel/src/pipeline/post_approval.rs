//! Post-approval resolution registry (kernel registry refactor, D-053 /
//! kernel-readiness item 1): once `gate()` re-confirms an approved
//! [`ActionRequest`], the effect handler is resolved here by action id —
//! `artifact.activate` routes to artifact activation, `artifact.reconfirm`
//! routes to overlay reconfirmation, and `artifact.nominate_upstream` routes
//! to nomination finalization; every other approved action id (notably
//! `email.create_draft`, the only id step-6/5d ever mints) falls through to
//! the documented default, the original draft-creation path. Every approval
//! minted before artifact activation existed (5d) is a draft, so the default
//! arm is load-bearing, not a catch-all convenience.

use std::future::Future;
use std::pin::Pin;

use openspine_gate::{gate, ActionOrigin};
use openspine_schemas::action::{ActionRequest, GateDecision};
use openspine_schemas::grant::TaskGrant;

use super::approval::create_approved_draft;
use super::artifact_activation::activate_approved_artifact;
use super::artifact_nomination::finalize_nomination;
use super::artifact_reconfirmation::reinstate_artifact;
use super::plan_approval::resolve_approved_plan;
use super::AppState;

type PostApprovalFuture<'a> = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>;

pub(super) type PostApprovalHandler =
    for<'a> fn(&'a AppState, &'a TaskGrant, &'a ActionRequest, i64) -> PostApprovalFuture<'a>;

fn handle_activate_artifact<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    request: &'a ActionRequest,
    chat_id: i64,
) -> PostApprovalFuture<'a> {
    Box::pin(activate_approved_artifact(state, grant, request, chat_id))
}

fn handle_reconfirm_artifact<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    request: &'a ActionRequest,
    chat_id: i64,
) -> PostApprovalFuture<'a> {
    Box::pin(reinstate_artifact(state, grant, request, chat_id))
}

fn handle_nominate_upstream<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    request: &'a ActionRequest,
    chat_id: i64,
) -> PostApprovalFuture<'a> {
    Box::pin(finalize_nomination(state, grant, request, chat_id))
}

fn handle_create_approved_draft<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    request: &'a ActionRequest,
    chat_id: i64,
) -> PostApprovalFuture<'a> {
    Box::pin(create_approved_draft(state, grant, request, chat_id))
}

fn handle_resolve_approved_plan<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    request: &'a ActionRequest,
    chat_id: i64,
) -> PostApprovalFuture<'a> {
    Box::pin(resolve_approved_plan(state, grant, request, chat_id))
}

fn handle_headless_approved<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    request: &'a ActionRequest,
    chat_id: i64,
) -> PostApprovalFuture<'a> {
    Box::pin(async move {
        if request.params.get("headless").map(String::as_str) != Some("true") {
            return handle_unknown_approved_action(state, grant, request, chat_id).await;
        }
        let now = jiff::Timestamp::now();
        let outcome = gate(
            grant,
            request,
            ActionOrigin::Shell,
            &state.store,
            &state.action_catalog,
            &state.connectors,
            now,
        );
        if !matches!(outcome.decision, GateDecision::Allow) {
            state.store.append_audit(
                "headless.approval_gate_denied",
                Some(&request.action),
                Some(&outcome.decision),
                Some("approved headless request failed re-gate"),
                Some(grant.id),
                &[],
                &[],
            )?;
            return Ok(());
        }
        crate::api::connector_breaker::dispatch_allowed_action(
            state,
            grant,
            &request.action,
            chat_id,
            None,
        )
        .await
        .map_err(|err| anyhow::anyhow!("headless approved dispatch failed: {err:?}"))?;
        state.store.append_audit(
            "headless.approved_dispatched",
            Some(&request.action),
            Some(&GateDecision::Allow),
            None,
            Some(grant.id),
            &[],
            &[],
        )?;
        Ok(())
    })
}
fn handle_unknown_approved_action<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    request: &'a ActionRequest,
    chat_id: i64,
) -> PostApprovalFuture<'a> {
    Box::pin(async move {
        state.store.append_audit(
            "approval.resolution_refused",
            Some(&request.action),
            None,
            Some("no post-approval handler registered"),
            Some(grant.id),
            &[],
            &[],
        )?;
        super::notify_owner_best_effort(
            state,
            chat_id,
            "Approval recorded, but no resolver exists for that action.",
        )
        .await;
        Ok(())
    })
}

/// Every approval-bearing action has an explicit resolver. Unknown actions
/// are refused instead of falling through to draft creation.
const POST_APPROVAL_HANDLERS: &[(&str, PostApprovalHandler)] = &[
    ("artifact.activate", handle_activate_artifact),
    ("artifact.reconfirm", handle_reconfirm_artifact),
    ("artifact.nominate_upstream", handle_nominate_upstream),
    ("plan.execute", handle_resolve_approved_plan),
    ("email.create_draft", handle_create_approved_draft),
];

pub(super) fn resolve_post_approval_handler(request: &ActionRequest) -> PostApprovalHandler {
    if request.params.get("headless").map(String::as_str) == Some("true") {
        return handle_headless_approved;
    }
    POST_APPROVAL_HANDLERS
        .iter()
        .find(|(id, _)| *id == request.action.as_str())
        .map(|(_, handler)| *handler)
        .unwrap_or(handle_unknown_approved_action)
}

//! Post-approval resolution registry (kernel registry refactor, D-053 /
//! kernel-readiness item 1): once `gate()` re-confirms an approved
//! [`ActionRequest`], the effect handler is resolved here by action id —
//! `artifact.activate` routes to artifact activation; every other approved
//! action id (notably `email.create_draft`, the only id step-6/5d ever
//! mints) falls through to the documented default, the original
//! draft-creation path. Every approval minted before artifact activation
//! existed (5d) is a draft, so the default arm is load-bearing, not a
//! catch-all convenience.

use std::future::Future;
use std::pin::Pin;

use openspine_schemas::action::{ActionId, ActionRequest};
use openspine_schemas::grant::TaskGrant;

use super::approval::create_approved_draft;
use super::artifact_activation::activate_approved_artifact;
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
    ("plan.execute", handle_resolve_approved_plan),
    ("email.create_draft", handle_create_approved_draft),
];

pub(super) fn resolve_post_approval_handler(action: &ActionId) -> PostApprovalHandler {
    POST_APPROVAL_HANDLERS
        .iter()
        .find(|(id, _)| *id == action.as_str())
        .map(|(_, handler)| *handler)
        .unwrap_or(handle_unknown_approved_action)
}

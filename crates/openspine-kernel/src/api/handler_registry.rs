//! Action-handler registry (kernel registry refactor, part 3).
//!
//! `dispatch_allowed_action` resolves which kernel function serves a given
//! allowed action id through this registry. The table mirrors the seven
//! dispatch arms of the previous `match` one-to-one; a lookup miss returns
//! an honest stub rather than erroring, and `email.create_draft` /
//! `artifact.activate` are deliberately NOT registered (they run only via
//! the post-approval path, not the allowed-action path).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use openspine_schemas::action::ActionId;
use openspine_schemas::grant::TaskGrant;
use serde_json::{json, Value};

use crate::pipeline::AppState;

use super::actions::{
    dispatch_lyra_preview, dispatch_read_selected_thread, DispatchError, PreviewPayload,
    TelegramReplyPayload,
};
use super::artifact_nominate::dispatch_artifact_nominate;
use super::artifact_propose::dispatch_artifact_propose;
use super::connector_breaker::call_with_connector;
use super::plan::dispatch_plan_preview;

/// The boxed future every handler returns. Must be `Send` because dispatch
/// runs on the axum request task.
type HandlerFuture<'a> = Pin<Box<dyn Future<Output = Result<Value, DispatchError>> + Send + 'a>>;

/// A kernel action handler: given the bound app state, the grant that
/// authorized the action, the action id, the grant-bound chat id, and the
/// optional JSON payload, produce the action result (or a [`DispatchError`]).
pub(crate) type ActionHandler = for<'a> fn(
    &'a AppState,
    &'a TaskGrant,
    &'a ActionId,
    i64,
    Option<&'a Value>,
) -> HandlerFuture<'a>;

/// The kernel's action-handler registry. Every registered `ActionId` maps to
/// exactly one handler; the canonical set is [`Self::default_registrations`].
pub(crate) struct ActionHandlerRegistry {
    map: HashMap<&'static str, ActionHandler>,
}

impl ActionHandlerRegistry {
    /// The one-to-one mapping of allowed-action ids to kernel handlers.
    /// Mirrors the prior `match` in `dispatch_allowed_action`.
    pub(crate) fn default_registrations() -> Self {
        let mut map: HashMap<&'static str, ActionHandler> = HashMap::new();
        map.insert("openspine.status.read", handle_status_read as ActionHandler);
        map.insert(
            "telegram.reply:owner_channel",
            handle_telegram_reply as ActionHandler,
        );
        map.insert(
            "email.read_thread:selected_no_attachments",
            handle_read_selected_thread as ActionHandler,
        );
        map.insert("lyra.ui.preview", handle_lyra_preview as ActionHandler);
        map.insert("artifact.propose", handle_artifact_propose as ActionHandler);
        map.insert("plan.propose", handle_plan_propose as ActionHandler);
        map.insert(
            "artifact.nominate_upstream",
            handle_artifact_nominate as ActionHandler,
        );
        map.insert(
            "workflow.invoke:approved",
            handle_workflow_invoke as ActionHandler,
        );
        map.insert(
            "setup.workflow.start",
            handle_setup_workflow_start as ActionHandler,
        );
        ActionHandlerRegistry { map }
    }

    /// Resolve an action id to its handler, or `None` when the id is a known
    /// catalog entry with no Step 4 kernel-side implementation yet.
    pub(crate) fn lookup(&self, id: &str) -> Option<ActionHandler> {
        self.map.get(id).copied()
    }
}

fn handle_status_read<'a>(
    _state: &'a AppState,
    _grant: &'a TaskGrant,
    _action: &'a ActionId,
    _chat_id: i64,
    _payload: Option<&'a Value>,
) -> HandlerFuture<'a> {
    Box::pin(async move { Ok(json!({"status": "ok"})) })
}

fn handle_telegram_reply<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    action: &'a ActionId,
    bound_chat_id: i64,
    payload: Option<&'a Value>,
) -> HandlerFuture<'a> {
    Box::pin(async move {
        let p = payload.ok_or_else(|| {
            DispatchError::BadRequest("telegram.reply:owner_channel requires a payload".to_string())
        })?;
        let reply: TelegramReplyPayload = serde_json::from_value(p.clone()).map_err(|_| {
            DispatchError::BadRequest(
                "telegram.reply:owner_channel payload must be exactly {\"text\": string}"
                    .to_string(),
            )
        })?;
        crate::spend::guard_connector_for(state, grant)
            .await
            .map_err(DispatchError::Resource)?;
        // AD-103/AD-141: admit + bound-timeout the Telegram send at the call
        // site. The helper records both breaker health and the D-069 counter.
        call_with_connector(
            state,
            "telegram",
            action,
            grant,
            state
                .connectors
                .telegram()
                .send_reply(bound_chat_id, &reply.text),
        )
        .await?;
        Ok(json!({"sent": true}))
    })
}

fn handle_read_selected_thread<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    action: &'a ActionId,
    _chat_id: i64,
    payload: Option<&'a Value>,
) -> HandlerFuture<'a> {
    Box::pin(dispatch_read_selected_thread(state, grant, action, payload))
}

fn handle_lyra_preview<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    action: &'a ActionId,
    bound_chat_id: i64,
    payload: Option<&'a Value>,
) -> HandlerFuture<'a> {
    Box::pin(async move {
        let p = payload.ok_or_else(|| {
            DispatchError::BadRequest("lyra.ui.preview requires a payload".to_string())
        })?;
        let preview: PreviewPayload = serde_json::from_value(p.clone()).map_err(|_| {
            DispatchError::BadRequest(
                "lyra.ui.preview payload must be exactly {\"subject\": string, \"body\": string}"
                    .to_string(),
            )
        })?;
        dispatch_lyra_preview(state, grant, action, bound_chat_id, &preview).await
    })
}

fn handle_plan_propose<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    action: &'a ActionId,
    bound_chat_id: i64,
    payload: Option<&'a Value>,
) -> HandlerFuture<'a> {
    Box::pin(async move {
        let p = payload.ok_or_else(|| {
            DispatchError::BadRequest("plan.propose requires a Plan payload".to_string())
        })?;
        let plan: openspine_schemas::plan::Plan =
            serde_json::from_value(p.clone()).map_err(|_| {
                DispatchError::BadRequest("plan.propose payload is not a valid Plan".to_string())
            })?;
        dispatch_plan_preview(state, grant, action, bound_chat_id, &plan).await
    })
}

fn handle_artifact_propose<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    action: &'a ActionId,
    bound_chat_id: i64,
    payload: Option<&'a Value>,
) -> HandlerFuture<'a> {
    Box::pin(dispatch_artifact_propose(
        state,
        grant,
        action,
        bound_chat_id,
        payload,
    ))
}

fn handle_artifact_nominate<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    _action: &'a ActionId,
    bound_chat_id: i64,
    payload: Option<&'a Value>,
) -> HandlerFuture<'a> {
    Box::pin(dispatch_artifact_nominate(
        state,
        grant,
        bound_chat_id,
        payload,
    ))
}

fn handle_workflow_invoke<'a>(
    _state: &'a AppState,
    _grant: &'a TaskGrant,
    _action: &'a ActionId,
    _chat_id: i64,
    _payload: Option<&'a Value>,
) -> HandlerFuture<'a> {
    Box::pin(
        async move { Ok(json!({"stub": true, "note": "workflow.invoke not yet implemented"})) },
    )
}

fn handle_setup_workflow_start<'a>(
    _state: &'a AppState,
    _grant: &'a TaskGrant,
    _action: &'a ActionId,
    _chat_id: i64,
    _payload: Option<&'a Value>,
) -> HandlerFuture<'a> {
    Box::pin(async move {
        Ok(json!({"stub": true, "note": "setup.workflow.start not yet implemented"}))
    })
}

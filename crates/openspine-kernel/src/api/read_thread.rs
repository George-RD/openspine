//! Bounded Gmail thread read dispatch.

use super::actions::DispatchError;
use crate::pipeline::AppState;
use openspine_schemas::grant::TaskGrant;
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;
use ulid::Ulid;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadThreadPayload {
    selection_token_id: String,
}

/// Validate and consume the grant-bound selection token, then fetch the
/// bounded, attachment-free Gmail thread.
pub(super) async fn dispatch_read_selected_thread(
    state: &AppState,
    grant: &TaskGrant,
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

    // gate() has already validated token possession, grant binding, type, and
    // expiry. Re-read only to obtain the bounded Gmail target id.
    let token = state
        .store
        .find_selection_token(token_id)
        .map_err(|err| DispatchError::Resource(err.into()))?
        .ok_or_else(|| DispatchError::BadRequest("unknown selection token".to_string()))?;

    // Atomic single-use consume, post-allow (D-050 / D-055.3).
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
    let thread_result = gmail.fetch_thread(&token.target_id).await;
    crate::failure_surfacing::record_connector_outcome(
        &state.store,
        "gmail",
        thread_result.is_ok(),
    )
    .map_err(|err| DispatchError::Resource(err.into()))?;
    let thread = thread_result.map_err(|err| DispatchError::Connector(err.into()))?;

    Ok(json!({
        "thread_id": thread.thread_id,
        "messages": thread.messages.iter().map(|m| json!({
            "from": m.from,
            "subject": m.subject,
            "body_text": m.body_text,
        })).collect::<Vec<_>>(),
    }))
}

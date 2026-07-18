//! AD-103/AD-141: every external connector call (Telegram send, Gmail read,
//! approved Gmail draft write) runs through the connector's circuit breaker +
//! bounded timeout. Admission is paired with *each* connector future (not the
//! whole action), so a rate-limit/breaker permit counts a real connector call
//! and a malformed action that never reaches a connector costs nothing. A
//! genuinely Open/HalfOpen breaker blocks the call AFTER `gate()` has
//! authorized it, emitting the distinct `connector_unavailable` audit event —
//! operational failure, never a policy denial. Split out of `actions.rs` to
//! keep that file under the 500-line gate.
//!
//! The breaker outcome *and* the D-069 counter are recorded at the
//! connector-call site by [`call_with_connector`] itself — the handler never
//! strands a HalfOpen probe, because admission is taken immediately before the
//! future and resolved immediately after it returns (success closes the probe;
//! a real failure or timeout reopens it). A caller/validation error returned
//! *before* the connector future is never admitted, so it cannot strand the
//! breaker.

use anyhow::anyhow;
use openspine_schemas::action::ActionId;
use openspine_schemas::grant::TaskGrant;
use std::future::Future;
use tokio::time::timeout;

use super::actions::DispatchError;
use crate::connector_reality::{
    ConnectorCallError, ConnectorProbePermit, CONNECTOR_UNAVAILABLE_AUDIT_KIND,
};
use crate::pipeline::AppState;

/// Record a connector outcome in both the breaker registry and the D-069
/// counter store, at the single point where the real call result is known.
fn record_connector_outcome(
    state: &AppState,
    connector: &str,
    permit: ConnectorProbePermit,
    ok: bool,
) {
    state
        .connectors
        .record_connector_outcome_for_generation(connector, permit, ok);
    crate::failure_surfacing::record_connector_outcome_or_batch(state, connector, ok);
}

/// Run one connector future under admission + bounded timeout, recording the
/// breaker outcome + D-069 counter at the call site. On a genuinely
/// Open/HalfOpen breaker, emits the distinct `connector_unavailable` audit
/// event and returns [`DispatchError::ConnectorUnavailable`]; on a rate-limit
/// rejection returns [`DispatchError::Connector`]. Used by every kernel path
/// that touches an external connector (Telegram sends, Gmail reads) so
/// admission + breaker health + unavailable auditing are uniform.
pub(crate) async fn call_with_connector<F, T, E>(
    state: &AppState,
    connector: &str,
    action: &ActionId,
    grant: &TaskGrant,
    fut: F,
) -> Result<T, DispatchError>
where
    F: Future<Output = Result<T, E>>,
    E: Into<anyhow::Error>,
{
    let permit = match state
        .connectors
        .acquire_connector_with_generation(connector)
    {
        Ok(permit) => permit,
        Err(err) => {
            return Err(map_admission_error(state, action, grant, err));
        }
    };
    match timeout(state.connector_call_timeout, fut).await {
        Ok(inner) => {
            record_connector_outcome(state, connector, permit, inner.is_ok());
            inner.map_err(|err| DispatchError::Connector(err.into()))
        }
        Err(_elapsed) => {
            record_connector_outcome(state, connector, permit, false);
            tracing::error!(action = %action.0, connector, "connector call timed out");
            Err(DispatchError::Connector(anyhow!(
                "{connector} call timed out after {:?}",
                state.connector_call_timeout
            )))
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PreflightConnectorError {
    #[error("{connector} connector unavailable")]
    Unavailable { connector: String },
    #[error("{connector} connector rate-limited; retry after {retry_after:?}")]
    RateLimited {
        connector: String,
        retry_after: std::time::Duration,
    },
    #[error("{connector} preflight timed out")]
    Timeout { connector: String },
    #[error("connector preflight failed: {0}")]
    Connector(#[source] anyhow::Error),
    #[error("connector preflight resource failure: {0}")]
    Resource(#[source] anyhow::Error),
}

/// Run a connector effect that occurs before a task grant exists (polling,
/// credential/preflight reads, and startup notifications). It still receives
/// the same per-call rate-limit, breaker, timeout, and outcome treatment; it
/// simply records unavailable audit evidence without a grant id.
pub(crate) async fn call_with_connector_preflight<F, T, E>(
    state: &AppState,
    connector: &str,
    action: Option<&ActionId>,
    fut: F,
) -> Result<T, PreflightConnectorError>
where
    F: Future<Output = Result<T, E>>,
    E: Into<anyhow::Error>,
{
    let permit = match state
        .connectors
        .acquire_connector_with_generation(connector)
    {
        Ok(permit) => permit,
        Err(err) => {
            if matches!(err, ConnectorCallError::Unavailable { .. }) {
                state
                    .store
                    .append_audit(
                        CONNECTOR_UNAVAILABLE_AUDIT_KIND,
                        action,
                        None,
                        None,
                        None,
                        &[],
                        &[],
                    )
                    .map_err(|err| PreflightConnectorError::Resource(anyhow::Error::new(err)))?;
            }
            let mapped = match err {
                ConnectorCallError::Unavailable { .. } => PreflightConnectorError::Unavailable {
                    connector: connector.to_string(),
                },
                ConnectorCallError::RateLimited { retry_after, .. } => {
                    PreflightConnectorError::RateLimited {
                        connector: connector.to_string(),
                        retry_after,
                    }
                }
            };
            return Err(mapped);
        }
    };
    match timeout(state.connector_call_timeout, fut).await {
        Ok(inner) => {
            record_connector_outcome(state, connector, permit, inner.is_ok());
            inner.map_err(|err| PreflightConnectorError::Connector(err.into()))
        }
        Err(_) => {
            record_connector_outcome(state, connector, permit, false);
            Err(PreflightConnectorError::Timeout {
                connector: connector.to_string(),
            })
        }
    }
}

/// Like [`call_with_connector`] but for a *write* whose effect is not
/// confirmed by the returned value (e.g. `gmail.create_draft`). This
/// candidate Gmail-write extension treats timeout/no-response as
/// **delivery-unknown**: the write may have landed before the response was
/// lost, so the breaker records a failure while pending evidence remains.
pub(crate) async fn call_with_connector_write<F, T, E>(
    state: &AppState,
    connector: &str,
    action: &ActionId,
    grant: &TaskGrant,
    fut: F,
) -> Result<T, DispatchError>
where
    F: Future<Output = Result<T, E>>,
    E: Into<anyhow::Error>,
{
    let permit = match state
        .connectors
        .acquire_connector_with_generation(connector)
    {
        Ok(permit) => permit,
        Err(err) => {
            return Err(map_admission_error(state, action, grant, err));
        }
    };
    match timeout(state.connector_call_timeout, fut).await {
        Ok(inner) => {
            record_connector_outcome(state, connector, permit, inner.is_ok());
            inner.map_err(|err| map_write_error(err.into()))
        }
        Err(_elapsed) => {
            record_connector_outcome(state, connector, permit, false);
            tracing::error!(action = %action.0, connector, "connector write timed out (delivery-unknown)");
            Err(DispatchError::DeliveryUnknown(anyhow!(
                "{connector} write timed out after {:?}; delivery-unknown, fenced for retry",
                state.connector_call_timeout
            )))
        }
    }
}

/// Run the effect of one `gate()`-allowed action. Only reached after `Allow`
/// — a deny/approval-required decision never calls this. The handler itself
/// admits each connector call it makes via [`call_with_connector`].
pub(crate) async fn dispatch_allowed_action(
    state: &AppState,
    grant: &TaskGrant,
    action: &ActionId,
    bound_chat_id: i64,
    payload: Option<&serde_json::Value>,
) -> Result<serde_json::Value, DispatchError> {
    let id = action.0.as_str();
    match state.action_handlers.lookup(id) {
        Some(handler) => handler(state, grant, action, bound_chat_id, payload).await,
        None => Ok(serde_json::json!({
            "stub": true,
            "note": format!("{id} has no Step 4 kernel-side implementation yet"),
        })),
    }
}

/// Map a breaker admission rejection to a dispatch error. A genuinely
/// Open/HalfOpen breaker emits the distinct `connector_unavailable` audit
/// event and returns [`DispatchError::ConnectorUnavailable`]; a `RateLimited`
/// rejection returns an ordinary [`DispatchError::Connector`] (so it still
/// enters normal failure surfacing rather than being reported as an
/// availability outage). The outer `mediate_and_dispatch_action` skips its own
/// `action.dispatch_failed` batch for `ConnectorUnavailable` (already recorded
/// here) — so an Open breaker is never double-counted.
fn map_admission_error(
    state: &AppState,
    action: &ActionId,
    grant: &TaskGrant,
    err: ConnectorCallError,
) -> DispatchError {
    match err {
        ConnectorCallError::Unavailable { .. } => {
            if let Err(audit_err) = state.store.append_audit(
                CONNECTOR_UNAVAILABLE_AUDIT_KIND,
                Some(action),
                None,
                None,
                Some(grant.id),
                &[],
                &[],
            ) {
                return DispatchError::Resource(anyhow::Error::new(audit_err));
            }
            DispatchError::ConnectorUnavailable(anyhow!("{action} connector unavailable"))
        }
        ConnectorCallError::RateLimited { .. } => {
            DispatchError::Connector(anyhow!("{action} connector rate-limited: {err}"))
        }
    }
}

/// Map a resolved (non-timeout) write outcome to a dispatch error.
///
/// Candidate Gmail-write extension: a `gmail.create_draft` whose effect is not
/// confirmed by the returned value must retain durable pending evidence unless
/// the provider returns a *confirmed* response. A transport/no-response class
/// may mean the write landed before the response was lost, so it surfaces as
/// [`DispatchError::DeliveryUnknown`] rather than a confirmed failure. Only a
/// response the provider explicitly reports as failed (e.g. a definite `api`
/// error) resolves the pending row.
fn map_write_error(err: anyhow::Error) -> DispatchError {
    if let Some(gmail) = err.downcast_ref::<crate::gmail::GmailError>() {
        if matches!(
            gmail.class,
            crate::gmail::GmailFailureClass::Transport
                | crate::gmail::GmailFailureClass::MalformedResponse
        ) {
            return DispatchError::DeliveryUnknown(anyhow!(
                "gmail write outcome is unconfirmed (delivery-unknown): {gmail}"
            ));
        }
    }
    DispatchError::Connector(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmail_transport_write_is_delivery_unknown() {
        let err = map_write_error(anyhow::Error::new(crate::gmail::GmailError {
            status: None,
            class: crate::gmail::GmailFailureClass::Transport,
        }));
        assert!(matches!(err, DispatchError::DeliveryUnknown(_)));
    }

    #[test]
    fn gmail_malformed_success_response_is_delivery_unknown() {
        let err = map_write_error(anyhow::Error::new(crate::gmail::GmailError {
            status: Some(200),
            class: crate::gmail::GmailFailureClass::MalformedResponse,
        }));
        assert!(matches!(err, DispatchError::DeliveryUnknown(_)));
    }

    #[test]
    fn confirmed_gmail_api_write_failure_is_connector_error() {
        let err = map_write_error(anyhow::Error::new(crate::gmail::GmailError {
            status: Some(500),
            class: crate::gmail::GmailFailureClass::Api,
        }));
        assert!(matches!(err, DispatchError::Connector(_)));
    }
}

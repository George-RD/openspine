//! AD-138 failure taxonomy and routing primitives.
//!
//! Authority/escalation failures belong on the immediate owner-notification
//! lane. Connector/resource failures belong in the batched owner digest. The
//! routing decision is deliberately pure; effectful persistence is kept in
//! `store::failure_surfacing` and the verified-owner pipeline.

pub(crate) mod retry_worker;
use crate::pipeline::AppState;
use crate::store::{Store, StoreError};

/// AD-138's failure-routing taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailureClass {
    Authority,
    Escalation,
    GateDenial,
    Connector,
    Resource,
}

impl FailureClass {
    /// Whether this class must surface immediately to the owner.
    pub(crate) fn routes_immediately(self) -> bool {
        matches!(self, Self::Authority | Self::Escalation)
    }

    /// Stable storage/audit spelling for this class.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Authority => "authority",
            Self::Escalation => "escalation",
            Self::Connector => "connector",
            Self::Resource => "resource",
            Self::GateDenial => "gate_denial",
        }
    }

    /// Whether this class is accumulated in the owner digest.
    pub(crate) fn routes_to_digest(self) -> bool {
        !self.routes_immediately()
    }
}

/// Record a connector/resource failure in the digest substrate. `summary`
/// is the bounded, non-sensitive description shown in the owner list;
/// `detail` is the sensitive full text, encrypted into the artifact store
/// (D-012: never persisted as plaintext) and retrievable only by the
/// authenticated owner via `/digest <ULID>`. Encryption failing closed is
/// mandatory — a row is never inserted with `text_ref = NULL` when detail
/// was supplied (NULL is reserved for pre-migration legacy rows).
///
/// Callers must use the immediate notification path for `Authority`/
/// `Escalation` classes; this guard rejects those rather than silently
/// misrouting them.
pub(crate) fn batch_failure(
    state: &AppState,
    class: FailureClass,
    summary: &str,
    detail: &str,
) -> Result<(), StoreError> {
    if !class.routes_to_digest() {
        return Err(StoreError::FailureRouting(format!(
            "failure class {} belongs on immediate owner lane",
            class.as_str()
        )));
    }
    let text_ref = encrypt_digest_detail(&state.artifacts, detail)?;
    state
        .store
        .batch_digest_failure(class.as_str(), summary, &text_ref)?;
    Ok(())
}

/// Encrypt `detail` into the artifact store and verify the round-trip
/// (digest check) before returning its ref. Fails closed on any store error
/// so a digest row is never created without a durable, readable artifact.
fn encrypt_digest_detail(
    artifacts: &crate::artifact_store::ArtifactStore,
    detail: &str,
) -> Result<String, StoreError> {
    let artifact_ref = artifacts
        .put(detail.as_bytes())
        .map_err(StoreError::ArtifactStore)?;
    artifacts
        .get(&artifact_ref)
        .map_err(StoreError::ArtifactStore)?;
    Ok(artifact_ref.digest.to_string())
}

/// Record one connector outcome in AD-138's kernel-owned counters.
pub(crate) fn record_connector_outcome(
    store: &Store,
    connector: &str,
    success: bool,
) -> Result<(), StoreError> {
    store.increment_connector_outcome(connector, if success { "success" } else { "failure" })
}

/// Record a connector outcome and durably route counter persistence failures.
pub(crate) fn record_connector_outcome_or_batch(state: &AppState, connector: &str, success: bool) {
    if let Err(counter_err) = record_connector_outcome(&state.store, connector, success) {
        tracing::error!(error = %counter_err, connector, "failed to persist connector counter");
        if let Err(surface_err) = batch_failure(
            state,
            FailureClass::Resource,
            "Connector counter persistence failed",
            "Connector counter persistence failed",
        ) {
            tracing::error!(error = %surface_err, "connector counter failure surface append failed");
        }
    }
}

/// Best-effort durable telemetry for a Telegram callback acknowledgement.
///
/// A callback ack is pure control-plane bookkeeping (it only stops the
/// tapper's spinner); it must never abort the approval or notification it
/// Surface callback acknowledgement failures after the connector wrapper has
/// recorded the Telegram outcome and D-069 counter. This helper remains
/// best-effort so an ack failure never prevents approval processing.
pub(crate) fn record_callback_ack(state: &AppState, success: bool, error: Option<&str>) {
    if !success {
        if let Some(ack_err) = error {
            if let Err(surface_err) = batch_failure(
                state,
                FailureClass::Connector,
                "Telegram callback acknowledgement failed",
                ack_err,
            ) {
                tracing::error!(error = %surface_err, ack_error = ack_err, "callback ack failure surface append failed");
            }
        }
    }
}

/// Route an authority/escalation-class failure to the immediate
/// owner-notification lane (AD-138 / AD-133's surface). Rejects
/// connector/resource classes rather than silently misrouting them — those
/// belong in [`batch_failure`]'s digest instead. Best-effort by
/// construction: delegates to [`crate::pipeline::notify_owner_best_effort`],
/// which records the attempt before sending and never claims "notified"
/// before the send actually happens.
pub(crate) async fn notify_immediate_failure(
    state: &crate::pipeline::AppState,
    chat_id: i64,
    class: FailureClass,
    summary: &str,
) -> Result<(), StoreError> {
    if !class.routes_immediately() {
        return Err(StoreError::FailureRouting(format!(
            "failure class {} belongs in the digest, not the immediate lane",
            class.as_str()
        )));
    }
    match crate::pipeline::notify_owner_with_digest(state, chat_id, summary, &[], None).await {
        crate::pipeline::NotifyOutcome::Sent => Ok(()),
        outcome => {
            let detail = format!(
                "immediate {} owner notification failed: {outcome:?}",
                class.as_str()
            );
            batch_failure(
                state,
                FailureClass::Connector,
                "immediate owner notification failed",
                &detail,
            )?;
            Err(StoreError::FailureRouting(format!(
                "immediate owner notification failed: {outcome:?}"
            )))
        }
    }
}

#[cfg(test)]
mod tests;

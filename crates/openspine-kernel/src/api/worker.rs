//! Worker runtime HTTP handlers (AD-035 / AD-101 / AD-033).
//!
//! Both run only after `mediate_and_dispatch_action` has gated the caller's
//! request, so the invoker's grant has already authorized the action:
//!
//! * `worker.commission` — a master agent mints a narrowed sub-grant for a
//!   commissioned worker (caveat chain, AD-101), packs the worker's
//!   briefcase (D-085, the worker receives a briefcase — never the board),
//!   persists grant + dispatch atomically with a receipt (D-083), and spawns
//!   the sandboxed worker.
//! * `worker.report_result` — the worker's ONLY outbound channel (AD-035
//!   reply chokepoint). It records the structured result as a bus event; the
//!   worker never egresses directly.

use openspine_authority::worker_grant::mint_worker_grant;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::briefcase::{CounterpartyRef, TaskClass};
use openspine_schemas::digest::digest_of;
use openspine_schemas::event::Lane;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::worker::{WorkerCommissionSpec, WorkerResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::actions::DispatchError;
use super::handler_registry::HandlerFuture;
use crate::briefcase::SourcePool;
use crate::pipeline::AppState;
use crate::store::worker_dispatch::{
    commissioned_grant_for_receipt, record_worker_commissioned, record_worker_result,
    CommissionReceipt,
};
use crate::store::StoreError;
use std::str::FromStr;

/// Wire payload for `worker.commission` (carried as the action request
/// payload, which is artifact-referenced by the gate audit — never inline
/// plaintext beyond what the grant already covers).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerCommissionPayload {
    agent_id: String,
    allowed_actions: Vec<String>,
    #[serde(default)]
    bound_parameters: Vec<WorkerBoundParameterPayload>,
    expires_before: String,
    purpose: String,
    route_id: String,
    workflow_id: String,
    capability_pack_id: String,
    #[serde(default)]
    counterparty_channel: Option<String>,
    #[serde(default)]
    counterparty_identifier: Option<String>,
    /// Caller-provided stable id for this commission (a ULID the master
    /// generated). Propagated unchanged into `record_worker_commissioned`
    /// as the idempotency receipt, so a retried commission cannot mint a
    /// second grant and two intentional identical commissions keep distinct
    /// receipts (D-083).
    receipt: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerBoundParameterPayload {
    name: String,
    value: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerReportPayload {
    #[serde(default)]
    outcome: Option<String>,
    #[serde(default)]
    offered_slots: Vec<WorkerSlotPayload>,
    #[serde(default)]
    requests: Vec<WorkerRequestPayload>,
    /// Free-text notes reference, carried as an `ArtifactRef` (digest),
    /// never a bare ULID (Fit 7).
    #[serde(default)]
    notes_ref: Option<ArtifactRef>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerSlotPayload {
    id: String,
    label: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerRequestPayload {
    kind: String,
    /// Detail reference, carried as an `ArtifactRef` (digest), never a bare
    /// ULID (Fit 7).
    #[serde(default)]
    detail_ref: Option<ArtifactRef>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
struct WorkerCommissionResponse {
    worker_grant_id: String,
    parent_grant_id: String,
}

pub(crate) fn handle_worker_commission<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    _action: &'a ActionId,
    _chat_id: i64,
    payload: Option<&'a Value>,
) -> HandlerFuture<'a> {
    Box::pin(async move {
        let p: WorkerCommissionPayload = match payload {
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| {
                DispatchError::BadRequest(format!("worker.commission: invalid payload: {e}"))
            })?,
            None => {
                return Err(DispatchError::BadRequest(
                    "worker.commission requires a payload".to_string(),
                ))
            }
        };
        // Blocker 3: bind the receipt to THIS commissioning parent grant and
        // the canonical request digest. A receipt reused under a different
        // parent or request is a mismatch and must be rejected, never
        // silently resolved to the prior grant.
        let request_digest = digest_of(payload.unwrap_or(&Value::Null));
        match commissioned_grant_for_receipt(&state.store, grant.id, &request_digest, &p.receipt)
            .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?
        {
            CommissionReceipt::Match { worker_grant_id } => {
                return Ok(json!(WorkerCommissionResponse {
                    worker_grant_id: worker_grant_id.to_string(),
                    parent_grant_id: grant.id.to_string(),
                }));
            }
            CommissionReceipt::Mismatch => {
                return Err(DispatchError::BadRequest(
                    "worker.commission receipt already used by a different commission".to_string(),
                ));
            }
            CommissionReceipt::None => {}
        }
        let expires_before = match jiff::Timestamp::from_str(&p.expires_before) {
            Ok(t) => t,
            Err(e) => {
                return Err(DispatchError::BadRequest(format!(
                    "worker.commission: invalid expires_before: {e}"
                )))
            }
        };
        let key = crate::grant_hmac_key().ok_or_else(|| {
            DispatchError::Resource(anyhow::anyhow!("grant HMAC key unavailable"))
        })?;
        // D-085: task_class is derived from the parent (master) grant's route
        // lane — never caller-supplied. The two route lanes map to the two
        // briefcase task classes; anything without a resolvable lane defaults
        // to Conversation.
        let derived = {
            let registry = state.registry.read();
            registry
                .routes
                .iter()
                .find(|r| r.id == grant.route_id)
                .and_then(|r| r.when.lane)
                .map(|lane| match lane {
                    Lane::OwnerControl => TaskClass::Conversation,
                    Lane::ExternalCommunication => TaskClass::DraftApproval,
                    _ => TaskClass::Conversation,
                })
                .unwrap_or(TaskClass::Conversation)
        };
        let spec = WorkerCommissionSpec {
            agent_id: p.agent_id,
            allowed_actions: p
                .allowed_actions
                .iter()
                .map(|a| openspine_schemas::action::ActionId::new(a.clone()))
                .collect(),
            bound_parameters: p
                .bound_parameters
                .into_iter()
                .map(|b| openspine_schemas::worker::WorkerBoundParameter {
                    name: b.name,
                    value: b.value,
                })
                .collect(),
            expires_before,
            purpose: p.purpose,
            route_id: p.route_id,
            workflow_id: p.workflow_id,
            capability_pack_id: p.capability_pack_id,
            counterparty_channel: p.counterparty_channel,
            counterparty_identifier: p.counterparty_identifier,
            task_class: derived,
        };

        let worker = mint_worker_grant(grant, &spec, &key)
            .map_err(|e| DispatchError::BadRequest(format!("worker.commission rejected: {e}")))?;

        // D-085: the worker receives a packed briefcase, not the board.
        let counterparty = match (&spec.counterparty_channel, &spec.counterparty_identifier) {
            (Some(channel), Some(identifier)) => CounterpartyRef::Unresolved {
                channel: channel.clone(),
                identifier: identifier.clone(),
            },
            _ => CounterpartyRef::Unresolved {
                channel: format!("worker:{}", worker.id),
                identifier: worker.id.to_string(),
            },
        };
        let briefcase = crate::briefcase::pack_for_task(
            &worker,
            counterparty,
            serde_json::Value::Null,
            spec.task_class,
            &SourcePool::default(),
        )
        .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?;

        let pending_ref = state
            .artifacts
            .put(spec.purpose.as_bytes())
            .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?;
        let token_ref = state
            .artifacts
            .put(worker.task_token.as_bytes())
            .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?;

        let persisted = record_worker_commissioned(
            &state.store,
            grant.id,
            &worker,
            &pending_ref,
            &token_ref,
            0,
            &briefcase,
            &p.receipt,
            &request_digest,
        );
        let persisted_grant_id = match persisted {
            Ok(id) => id,
            Err(StoreError::Sqlite(rusqlite::Error::SqliteFailure(_, Some(msg))))
                if msg.contains("UNIQUE constraint failed") =>
            {
                match commissioned_grant_for_receipt(
                    &state.store,
                    grant.id,
                    &request_digest,
                    &p.receipt,
                )
                .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?
                {
                    CommissionReceipt::Match { worker_grant_id } => worker_grant_id,
                    CommissionReceipt::Mismatch => {
                        return Err(DispatchError::BadRequest(
                            "worker.commission receipt already used by a different commission"
                                .to_string(),
                        ));
                    }
                    CommissionReceipt::None => {
                        return Err(DispatchError::Resource(anyhow::anyhow!(
                            "worker commission lost receipt race but no row resolved"
                        )));
                    }
                }
            }
            Err(e) => return Err(DispatchError::Resource(anyhow::Error::new(e))),
        };
        // The initial receipt pre-check and the insert can race. Always
        // re-resolve the committed row, including the nominal Ok path, so a
        // loser cannot return another parent's grant as a successful match.
        match commissioned_grant_for_receipt(&state.store, grant.id, &request_digest, &p.receipt)
            .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?
        {
            CommissionReceipt::Match { worker_grant_id }
                if worker_grant_id == persisted_grant_id => {}
            CommissionReceipt::Match { .. } | CommissionReceipt::Mismatch => {
                return Err(DispatchError::BadRequest(
                    "worker.commission receipt already used by a different commission".to_string(),
                ));
            }
            CommissionReceipt::None => {
                return Err(DispatchError::Resource(anyhow::anyhow!(
                    "worker commission receipt disappeared after persistence"
                )));
            }
        }
        if persisted_grant_id != worker.id {
            return Ok(json!(WorkerCommissionResponse {
                worker_grant_id: persisted_grant_id.to_string(),
                parent_grant_id: grant.id.to_string(),
            }));
        }
        // suppress the already-commissioned grant (the worker can be
        // re-driven via the dispatch row).
        match state
            .sandbox
            .run_task(&state.kernel_endpoint, &worker.task_token)
            .await
        {
            Ok(()) => {
                state
                    .store
                    .append_audit(
                        "task.shell_completed",
                        None,
                        None,
                        None,
                        Some(persisted_grant_id),
                        &[],
                        &[],
                    )
                    .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?;
            }
            Err(err) => {
                tracing::error!(error = %err, grant_id = %persisted_grant_id, "worker shell failed");
                state
                    .store
                    .append_audit(
                        "task.shell_failed",
                        None,
                        None,
                        Some("worker shell failed"),
                        Some(persisted_grant_id),
                        &[],
                        &[],
                    )
                    .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?;
            }
        }

        Ok(json!(WorkerCommissionResponse {
            worker_grant_id: persisted_grant_id.to_string(),
            parent_grant_id: grant.id.to_string(),
        }))
    })
}

pub(crate) fn handle_worker_report_result<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    _action: &'a ActionId,
    _chat_id: i64,
    payload: Option<&'a Value>,
) -> HandlerFuture<'a> {
    Box::pin(async move {
        let p: WorkerReportPayload = match payload {
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| {
                DispatchError::BadRequest(format!("worker.report_result: invalid payload: {e}"))
            })?,
            None => {
                return Err(DispatchError::BadRequest(
                    "worker.report_result requires a payload".to_string(),
                ))
            }
        };
        let outcome = match p.outcome.as_deref() {
            Some("completed") => openspine_schemas::worker::WorkerOutcome::Completed,
            Some("failed") => openspine_schemas::worker::WorkerOutcome::Failed,
            Some("awaiting") => openspine_schemas::worker::WorkerOutcome::Awaiting,
            Some(other) => {
                return Err(DispatchError::BadRequest(format!(
                    "worker.report_result: unknown outcome {other:?}"
                )))
            }
            None => {
                return Err(DispatchError::BadRequest(
                    "worker.report_result: outcome is required".to_string(),
                ))
            }
        };
        // `notes_ref` is already a typed `ArtifactRef` (digest), never a bare
        // ULID (Fit 7) — deserialization validated the digest shape.
        let notes_ref = p.notes_ref;
        let offered_slots = p
            .offered_slots
            .into_iter()
            .map(|s| openspine_schemas::worker::WorkerSlot {
                id: s.id,
                label: s.label,
            })
            .collect();
        // `detail_ref` is already a typed `ArtifactRef` (digest), never a bare
        // ULID (Fit 7) — deserialization validated the digest shape.
        let requests = p
            .requests
            .into_iter()
            .map(|r| openspine_schemas::worker::WorkerRequest {
                kind: r.kind,
                detail_ref: r.detail_ref,
            })
            .collect::<Vec<_>>();
        let result = WorkerResult {
            outcome,
            offered_slots,
            requests,
            notes_ref,
        };

        // The worker's grant id is the bus aggregate the master consumes.
        record_worker_result(&state.store, grant.id, &result)
            .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?;

        Ok(json!({ "recorded": true, "worker_grant_id": grant.id.to_string() }))
    })
}

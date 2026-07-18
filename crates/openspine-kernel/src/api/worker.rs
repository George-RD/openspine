// openspine:allow-large-module reason: worker runtime handlers and supervision lifecycle share one audited boundary
//! Worker runtime HTTP handlers (AD-035 / AD-101 / AD-033 / AD-100 / AD-102).
//!
//! All run only after `mediate_and_dispatch_action` has gated the caller's
//! request, so the invoker's grant has already authorized the action:
//!
//! * `worker.commission` — a master agent mints a narrowed sub-grant for a
//!   commissioned worker (caveat chain, AD-101), packs the worker's
//!   briefcase (D-085, the worker receives a briefcase — never the board),
//!   persists grant + dispatch atomically with a receipt (D-083), and spawns
//!   the sandboxed worker. The worker is addressed by an identity tuple
//!   (AD-102) and bound to a trusted connector (AD-100) for restart caps.
//! * `worker.report_result` — the worker's ONLY outbound channel (AD-035
//!   reply chokepoint). It records the structured result as a bus event; the
//!   worker never egresses directly.
//! * `worker.failed` — the supervisor records a structured `worker_failed`
//!   event (AD-100). It NEVER mints or inherits a grant; continuation must
//!   re-compose through the normal pipeline.

use openspine_authority::worker_grant::mint_worker_grant;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::briefcase::{CounterpartyRef, TaskClass};
use openspine_schemas::digest::digest_of;
use openspine_schemas::event::Lane;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::worker::{
    WorkerCommissionSpec, WorkerFailureReason, WorkerIdentity, WorkerResult,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::actions::DispatchError;
use super::handler_registry::HandlerFuture;
use crate::briefcase::SourcePool;
use crate::pipeline::AppState;
use crate::sandbox::TaskRunError;
use crate::store::worker_dispatch::{
    commissioned_grant_for_receipt, record_worker_commissioned, record_worker_result,
    worker_dispatch_state, CommissionReceipt, WorkerDispatchState,
};
use crate::store::worker_supervision::{
    claim_conversation_in_flight, connector_restart_cap_available, record_worker_failed,
    release_conversation_in_flight_for_grant, worker_commission_admission,
};
use crate::store::StoreError;
use std::str::FromStr;

/// Per-connector restart-intensity cap (AD-100): at most `WORKER_RESTART_LIMIT`
/// recompositions within `WORKER_RESTART_WINDOW` for a single connector, so a
/// flaky external service cannot cause a restart storm. Beyond the cap the
/// supervisor fails closed and surfaces to the owner instead of restarting.
const WORKER_RESTART_WINDOW: std::time::Duration = std::time::Duration::from_secs(30);
/// Surface a cap-exhaustion decision to the owner. Failure handling remains
/// fail-closed: this notification is best-effort and never triggers a retry or
/// grant transfer.
async fn surface_restart_cap_exhausted(
    state: &AppState,
    chat_id: i64,
    connector: &str,
    restart_count: u32,
    restart_limit: u32,
) {
    if chat_id == 0 {
        return;
    }
    let detail = format!(
        "Worker restart cap exhausted for connector '{connector}' ({restart_count}/{restart_limit} in the active window); continuation is held for owner review."
    );
    let _ = crate::failure_surfacing::notify_immediate_failure(
        state,
        chat_id,
        crate::failure_surfacing::FailureClass::Escalation,
        &detail,
    )
    .await;
}
const WORKER_RESTART_LIMIT: u32 = 3;

/// RAII guard releasing a conversation's in-flight claim on drop (AD-102), so a
/// handler that returns on any path — success, bad request, or spawn failure —
/// never leaves the conversation permanently locked. The claim is keyed on the
/// commissioned worker's own grant id, matching the grant id the failure path
/// releases, so cleanup never clears a different holder.
struct ConversationClaimGuard<'a> {
    store: &'a crate::store::Store,
    owner: String,
    conversation: String,
    grant_id: ulid::Ulid,
    active: bool,
}

impl<'a> ConversationClaimGuard<'a> {
    /// Acquire the in-flight claim for `conversation` keyed on `grant_id` — the
    /// commissioned worker's own grant id. Coherent with failure cleanup, which
    /// releases the claim for that exact `grant_id`.
    fn acquire(
        store: &'a crate::store::Store,
        owner: &str,
        conversation: &str,
        grant_id: ulid::Ulid,
    ) -> Result<Self, DispatchError> {
        claim_conversation_in_flight(store, owner, conversation, grant_id).map_err(
            |e| match e {
                crate::store::StoreError::ConversationInFlight(_) => DispatchError::BadRequest(
                    "conversation already has an in-flight message (one message at a time)"
                        .to_string(),
                ),
                other => DispatchError::Resource(anyhow::Error::new(other)),
            },
        )?;
        Ok(Self {
            store,
            owner: owner.to_string(),
            conversation: conversation.to_string(),
            grant_id,
            active: true,
        })
    }
}

impl Drop for ConversationClaimGuard<'_> {
    fn drop(&mut self) {
        if self.active {
            let _ = release_conversation_in_flight_for_grant(
                self.store,
                &self.owner,
                &self.conversation,
                self.grant_id,
            );
        }
    }
}

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

/// Payload for `worker.failed` (AD-100). The failure target is the
/// authenticated worker grant (`grant.id`) — the payload names no grant, so a
/// supervisor can only fail the worker it holds, never an arbitrary dispatch.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerFailedPayload {
    reason: String,
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
    chat_id: i64,
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
        // D-108: reject a commission that cannot report its result before
        // connector supervision preflight, so capability errors are stable
        // regardless of route metadata.
        if !p
            .allowed_actions
            .iter()
            .any(|action| action == "worker.report_result")
        {
            return Err(DispatchError::BadRequest(
                "worker cannot report results".to_string(),
            ));
        }
        // Serialize cap admission through the precheck and durable commission
        // insert. This runs before minting the worker grant or writing token
        // artifacts; failure leaves no orphan authority.
        let _admission = worker_commission_admission();
        let preflight_connector = {
            let registry = state.registry.read();
            registry
                .routes
                .iter()
                .find(|r| r.id == grant.route_id)
                .and_then(|r| r.when.connector)
                .map(|c| {
                    serde_json::to_value(c)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_else(|| format!("{c:?}"))
                })
        }
        .ok_or_else(|| {
            DispatchError::BadRequest(
                "worker.commission requires a connector-bound route for supervision".to_string(),
            )
        })?;
        let available = connector_restart_cap_available(
            &state.store,
            &preflight_connector,
            jiff::Timestamp::now(),
            WORKER_RESTART_WINDOW,
            WORKER_RESTART_LIMIT,
        )
        .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?;
        if !available {
            drop(_admission);
            surface_restart_cap_exhausted(
                state,
                chat_id,
                &preflight_connector,
                WORKER_RESTART_LIMIT,
                WORKER_RESTART_LIMIT,
            )
            .await;
            return Err(DispatchError::BadRequest(format!(
                "worker.commission restart cap exhausted for connector {preflight_connector}"
            )));
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
        if !worker.effectively_allows(&openspine_schemas::action::ActionId::new(
            "worker.report_result",
        )) {
            return Err(DispatchError::BadRequest(
                "worker cannot report results".to_string(),
            ));
        }
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

        // AD-102: address this worker by identity tuple, never by process
        // handle. Derived from trusted grant context.
        let identity = WorkerIdentity {
            owner: grant.user.clone(),
            conversation: grant
                .thread_id
                .clone()
                .unwrap_or_else(|| grant.event_id.to_string()),
            task: worker.id.to_string(),
        };
        // AD-100: bind the restart cap bucket from a trusted connector. The
        // connector is the commissioning route's connector (kernel-owned),
        // never a caller-supplied string — fail closed if unbound so an
        // unbound route cannot masquerade as a real connector bucket.
        let connector = {
            let registry = state.registry.read();
            registry
                .routes
                .iter()
                .find(|r| r.id == worker.route_id)
                .and_then(|r| r.when.connector)
                .map(|c| {
                    serde_json::to_value(c)
                        .ok()
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_else(|| format!("{c:?}"))
                })
        };
        let connector = connector.ok_or_else(|| {
            DispatchError::BadRequest(
                "worker.commission requires a connector-bound route for supervision".to_string(),
            )
        })?;
        // AD-102: serialize this conversation — one message at a time. The
        // guard releases the claim on every handler exit path.
        let _claim = ConversationClaimGuard::acquire(
            &state.store,
            &identity.owner,
            &identity.conversation,
            worker.id,
        )?;

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
            chat_id,
            &briefcase,
            &p.receipt,
            &request_digest,
            &identity,
            &connector,
        );
        let persisted_grant_id = match persisted {
            Ok(id) => id,
            Err(StoreError::WorkerCannotReportResults) => {
                return Err(DispatchError::BadRequest(
                    "worker cannot report results".to_string(),
                ));
            }
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
            Err(StoreError::WorkerRestartCapExceeded(connector)) => {
                drop(_admission);
                surface_restart_cap_exhausted(
                    state,
                    chat_id,
                    &connector,
                    WORKER_RESTART_LIMIT,
                    WORKER_RESTART_LIMIT,
                )
                .await;
                return Err(DispatchError::BadRequest(format!(
                    "worker.commission restart cap exhausted for connector {connector}"
                )));
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
        drop(_admission);
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
                let dispatch_state = worker_dispatch_state(&state.store, persisted_grant_id)
                    .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?;
                match dispatch_state {
                    Some(WorkerDispatchState::Terminal) => {
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
                    Some(WorkerDispatchState::Dispatched) => {
                        let event = record_worker_failed(
                            &state.store,
                            persisted_grant_id,
                            WorkerFailureReason::ShellExited,
                            None,
                            jiff::Timestamp::now(),
                            WORKER_RESTART_WINDOW,
                            WORKER_RESTART_LIMIT,
                        )
                        .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?;
                        if !event.recomposition_permitted {
                            surface_restart_cap_exhausted(
                                state,
                                chat_id,
                                &event.connector,
                                event.restart_count,
                                event.restart_limit,
                            )
                            .await;
                        }
                    }
                    None => {
                        return Err(DispatchError::Resource(anyhow::anyhow!(
                            "worker shell exited without a dispatch row"
                        )));
                    }
                }
            }
            Err(err) => {
                let failure_reason = match &err {
                    TaskRunError::Startup(_) => WorkerFailureReason::StartupFailure,
                    TaskRunError::ShellExited(_) => WorkerFailureReason::ShellExited,
                    TaskRunError::Crashed(_) => WorkerFailureReason::Crash,
                };
                tracing::error!(error = %err, grant_id = %persisted_grant_id, "worker shell failed");
                match record_worker_failed(
                    &state.store,
                    persisted_grant_id,
                    failure_reason,
                    None,
                    jiff::Timestamp::now(),
                    WORKER_RESTART_WINDOW,
                    WORKER_RESTART_LIMIT,
                ) {
                    Ok(event) if !event.recomposition_permitted => {
                        surface_restart_cap_exhausted(
                            state,
                            chat_id,
                            &event.connector,
                            event.restart_count,
                            event.restart_limit,
                        )
                        .await;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            grant_id = %persisted_grant_id,
                            "worker failure event not recorded (dispatch may already be terminal)"
                        );
                    }
                }
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

pub(crate) fn handle_worker_failed<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    _action: &'a ActionId,
    chat_id: i64,
    payload: Option<&'a Value>,
) -> HandlerFuture<'a> {
    Box::pin(async move {
        let p: WorkerFailedPayload = match payload {
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| {
                DispatchError::BadRequest(format!("worker.failed: invalid payload: {e}"))
            })?,
            None => {
                return Err(DispatchError::BadRequest(
                    "worker.failed requires a payload".to_string(),
                ))
            }
        };
        // The failure target is the authenticated worker grant: `worker.failed`
        // is reached with the worker's own grant, so the event is always
        // recorded against `grant.id` — never an arbitrary dispatch row named
        // by the payload (the payload carries no grant id by design).
        let reason = match p.reason.as_str() {
            "shell_exited" => WorkerFailureReason::ShellExited,
            "crash" => WorkerFailureReason::Crash,
            "timeout" => WorkerFailureReason::Timeout,
            "lost" => WorkerFailureReason::Lost,
            "startup_failure" => WorkerFailureReason::StartupFailure,
            other => {
                return Err(DispatchError::BadRequest(format!(
                    "worker.failed: unknown reason {other:?}"
                )))
            }
        };
        // AD-100: record the structured failure event and charge the
        // per-connector restart ledger. Never mints or inherits a grant —
        // continuation requires re-composition through the normal pipeline.
        let event = record_worker_failed(
            &state.store,
            grant.id,
            reason,
            p.detail_ref.as_ref(),
            jiff::Timestamp::now(),
            WORKER_RESTART_WINDOW,
            WORKER_RESTART_LIMIT,
        )
        .map_err(|e| match e {
            StoreError::WorkerDispatchAlreadyFailed
            | StoreError::WorkerDispatchNotFound
            | StoreError::WorkerConnectorUnbound => {
                DispatchError::BadRequest(format!("worker.failed: {e}"))
            }
            other => DispatchError::Resource(anyhow::Error::new(other)),
        })?;
        if !event.recomposition_permitted {
            surface_restart_cap_exhausted(
                state,
                chat_id,
                &event.connector,
                event.restart_count,
                event.restart_limit,
            )
            .await;
        }
        Ok(json!({
            "recorded": true,
            "worker_grant_id": event.worker_grant_id.to_string(),
            "recomposition_permitted": event.recomposition_permitted,
            "restart_count": event.restart_count,
            "restart_limit": event.restart_limit,
        }))
    })
}

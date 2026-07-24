// openspine:allow-large-module reason: durable workflow substrate and production adapters share one invariant boundary
#![allow(dead_code)]

//! Deterministic, ledger-backed durable workflow execution (AD-104).
//!
//! [`WorkflowCtx`] is a pure check-then-record substrate: it never executes
//! outside-world work and never calls `gate()` itself. A caller calls
//! [`WorkflowCtx::begin_step`]; if the step is not yet durably recorded, the
//! caller performs the real work itself — gating any effect through the
//! existing production `gate()`/dispatch path — then calls
//! [`WorkflowCtx::complete_step`] with the outcome. This closes the
//! confused-deputy risk a closure-based wrapper would carry: WorkflowCtx
//! cannot authorize or execute anything a caller does not already do
//! through the existing authority boundary. No new `ActionCatalog` entries
//! are introduced by this substrate; it defines no action semantics of its
//! own.
//!
//! **Outbox intent, durably recorded before work happens.** `begin_step`
//! writes a `Pending` ledger row (the outbox intent) BEFORE returning
//! control to the caller — never after. [`StepState`] then distinguishes:
//!
//! - [`StepState::Fresh`] — no prior attempt exists. Safe to perform the
//!   work unconditionally.
//! - [`StepState::Resuming`] — a `Pending` row exists with no matching
//!   `Completed` row: an earlier attempt reached this step and was
//!   interrupted before recording its outcome. **A caller wrapping a
//!   non-idempotent effect (a model call, a connector call) MUST treat this
//!   as fail-closed** — the same stable `idempotency_key` is returned so a
//!   provider that DOES support idempotency-key deduplication could safely
//!   redo the call, but today's in-repo providers
//!   ([`crate::model_gateway::ProviderClient`], [`crate::gmail::GmailConnector`])
//!   accept no such key, so blindly redoing the work risks a duplicate
//!   real-world effect. `workflow_tests.rs`'s
//!   `resuming_a_real_provider_call_fails_closed_instead_of_reinvoking`
//!   test is both the proof and the required calling pattern. Kernel-internal
//!   reads with no external side effect ([`WorkflowCtx::now`],
//!   [`WorkflowCtx::random_u64`], [`WorkflowCtx::schedule_timer`]) treat
//!   `Fresh` and `Resuming` identically since redoing them is always safe.
//! - [`StepState::Replayed`] — a `Completed` row exists; here is the
//!   recorded outcome, no work needed.
//!
//! This is the Proposed D-0XX guarantee (see `IMPLEMENTATION-NOTES.md`): a
//! workflow run records each outside-world step — time, randomness,
//! model/connector calls, approvals, and timers — as a ledger-backed outbox
//! intent before the effect runs, then rehydrates by replaying recorded
//! outcomes after a crash. Replay never re-runs a recorded effect; a
//! non-idempotent effect resumed from a `Pending` (no `Completed`) row is
//! the caller's fail-closed obligation, not an automatic exactly-once
//! guarantee this substrate cannot provide without provider-side
//! idempotency-key support. Timers are a durable registry fired at-most-once
//! via a trusted-clock compare-and-swap; approval steps bind a digest over
//! action + target + payload so a stale approval diverges on replay.
//!
//! Inline step payloads are restricted to a closed, non-secret set of types
//! ([`WorkflowInlinePayload`]). A caller recording a private effect result
//! (a model output, a connector response body) uses
//! [`WorkflowCtx::begin_private_step`]/[`WorkflowCtx::complete_private_step`],
//! which store the real bytes in the encrypted `ArtifactStore` and ledger
//! only the resulting [`ArtifactRef`] — replay fetches and digest-verifies
//! the blob, failing closed if it is missing or tampered. Approval-kind
//! steps MUST bind their digest to [`ApprovalStepInputs`] (action + target
//! digest + payload digest, mirroring D-011's approval-binding fields), not
//! an arbitrary caller-chosen value — otherwise a stale approval could
//! replay after its target or payload changed.
//!
//! Timers are a durable registry (`workflow_timers`), not a ledger scan:
//! `Store::due_timers`/`Store::fire_due_timers` are the kernel-owned driver
//! function; [`run_timer_driver`] is a reusable sleep-until-deadline loop
//! spawned from `main()`'s startup `tokio::select!`. Firing is a DB-level
//! conditional transition (`UPDATE ... WHERE status = 'pending'`), so
//! at-most-once holds independent of application-level locking discipline.

use crate::api::actions::{mediate_and_dispatch_action, FailureSurface};
use crate::artifact_store::{ArtifactStore, ArtifactStoreError};
use crate::pipeline::AppState;
use crate::store::{Store, StoreError};
use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::audit::AuditEvent;
use openspine_schemas::digest::{canonical_json, Digest};
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::workflow::ApprovalSemantics;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

/// Closed, non-secret set of types [`WorkflowCtx`] may store inline in the
/// ledger. Anything else (private effect content) MUST go through
/// [`WorkflowCtx::begin_private_step`]/[`WorkflowCtx::complete_private_step`].
mod workflow_inline_sealed {
    pub trait Sealed {}
}

pub(crate) trait WorkflowInlinePayload:
    workflow_inline_sealed::Sealed + Serialize + DeserializeOwned
{
}
impl workflow_inline_sealed::Sealed for () {}
impl WorkflowInlinePayload for () {}
impl workflow_inline_sealed::Sealed for bool {}
impl WorkflowInlinePayload for bool {}
impl workflow_inline_sealed::Sealed for u32 {}
impl WorkflowInlinePayload for u32 {}
impl workflow_inline_sealed::Sealed for u64 {}
impl WorkflowInlinePayload for u64 {}
impl workflow_inline_sealed::Sealed for Timestamp {}
impl WorkflowInlinePayload for Timestamp {}
impl workflow_inline_sealed::Sealed for ArtifactRef {}
impl WorkflowInlinePayload for ArtifactRef {}
impl workflow_inline_sealed::Sealed for TimerSpec {}
impl WorkflowInlinePayload for TimerSpec {}
impl workflow_inline_sealed::Sealed for TransitionOutcome {}
impl WorkflowInlinePayload for TransitionOutcome {}
impl workflow_inline_sealed::Sealed for Digest {}
impl WorkflowInlinePayload for Digest {}
impl workflow_inline_sealed::Sealed for EntryBindingInputs {}
impl WorkflowInlinePayload for EntryBindingInputs {}
impl workflow_inline_sealed::Sealed for GatedDepartureInputs {}
impl WorkflowInlinePayload for GatedDepartureInputs {}

#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("workflow store error: {0}")]
    Store(#[from] StoreError),
    #[error("workflow serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("workflow step {ordinal} diverged: expected {expected}, got {actual}")]
    Divergence {
        ordinal: u64,
        expected: String,
        actual: String,
    },
    #[error("workflow step failed: {0}")]
    Step(String),
    #[error("invalid workflow run id {0:?}: must not contain ':'")]
    InvalidRunId(String),
    #[error("ledger failed verification, refusing to trust replay")]
    LedgerCorrupted,
    #[error("workflow ledger row {0} is malformed")]
    MalformedRecord(u64),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TimerSpec {
    timer_id: String,
    fires_at: Timestamp,
}

/// Canonical inputs an approval-kind step MUST bind its digest to (D-011):
/// Canonical inputs an approval-kind step MUST bind to (D-011): the action
/// being approved plus its target and payload digests. Both digests are
/// required (non-optional) so an approval can never be recorded unbound; a
/// stale approval — one whose target or payload changed after the owner
/// reviewed it — diverges on replay rather than silently reusing an approval
/// for content the owner never saw.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ApprovalStepInputs {
    pub action: String,
    pub target_digest: Digest,
    pub payload_digest: Digest,
}
/// Non-secret input digest bound for a plain state-machine transition step.
/// Only carries state ids (D-012: no plaintext).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TransitionStepInputs {
    pub from: String,
    pub to: String,
}

/// Non-secret digest-bound approval binding persisted atomically with an
/// entry into an approval-semantic state (D-011/D-012). Carries only ids and
/// digests; never plaintext content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EntryBindingInputs {
    pub target_state: String,
    pub request_id: String,
    pub action: String,
    pub payload_digest: Digest,
    pub target_digest: Digest,
}

/// Crash-safe entry transition input: binds the source edge plus the
/// approval binding so recovery cannot complete a different edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EntryTransitionInputs {
    pub from: String,
    pub binding: EntryBindingInputs,
}
/// Non-secret input for an approval-gated departure step. Binds the exact
/// edge (from/to), the entry-bound request id, and the D-011 action and
/// digests so a crashed-Pending departure cannot resume against a different
/// edge or approval (D-012: ids and digests only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GatedDepartureInputs {
    pub from: String,
    pub to: String,
    pub request_id: String,
    pub action: String,
    pub payload_digest: Digest,
    pub target_digest: Digest,
}
/// Gate-bound input digest for typed workflow dispatch. The concrete action
/// is selected by the adapter and the existing handler registry binds it to
/// the operation; no caller-supplied closure can diverge from the request.
/// The digest binds the actual payload and the grant's bound chat, so a
/// replayed step is tied to the same request content and target it recorded.
#[derive(Debug, Clone, Serialize)]
struct GatedStepDigest {
    action: String,
    grant_id: String,
    bound_chat_id: i64,
    inputs_digest: String,
    payload_digest: Option<String>,
}
/// Stable, immutable handle to a scheduled timer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimerHandle {
    run_id: String,
    timer_id: String,
    fires_at: Timestamp,
}

impl TimerHandle {
    pub fn fires_at(&self) -> Timestamp {
        self.fires_at
    }

    pub(crate) fn timer_id(&self) -> &str {
        &self.timer_id
    }
}

/// Append-only workflow step phases. Receipt is a durable pointer to an
/// encrypted artifact, written before the terminal completion event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "phase")]
enum StepRecord {
    Pending {
        step_id: String,
        input_digest: String,
    },
    Receipt {
        step_id: String,
        input_digest: String,
        artifact_ref: ArtifactRef,
    },
    Completed {
        step_id: String,
        input_digest: String,
        outcome: Outcome,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Outcome {
    Ok(serde_json::Value),
    Err(String),
}
/// Closed, non-secret result of an approval-gated state transition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct TransitionOutcome {
    pub target: String,
}

/// One logical step reconstructed by stable step ID, not physical adjacency.
struct StepEntry {
    step_id: String,
    kind: String,
    input_digest: String,
    pending_seq: u64,
    receipt: Option<ArtifactRef>,
    completed: Option<Outcome>,
}
/// Stable identity for one begun workflow step.
///
/// The handle is created by `begin_step` and is the only identity accepted by
/// receipt/completion methods. Its private fields prevent callers from
/// manufacturing an ambiguous kind+digest lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StepHandle {
    step_id: String,
    pending_seq: u64,
}

pub(crate) enum StepState<T> {
    Replayed {
        handle: StepHandle,
        outcome: Result<T, String>,
    },
    Fresh {
        handle: StepHandle,
        idempotency_key: String,
    },
    Resuming {
        handle: StepHandle,
        idempotency_key: String,
    },
}
pub struct WorkflowCtx<'a> {
    store: &'a Store,
    run_id: String,
    definition_id: String,
    definition_version: String,
    steps: Vec<StepEntry>,
    cursor: usize,
}

fn digest_inputs(inputs: &impl Serialize) -> Result<String, WorkflowError> {
    let value = serde_json::to_value(inputs)?;
    Ok(
        openspine_schemas::digest::digest_of_bytes(canonical_json(&value).as_bytes())
            .as_str()
            .to_string(),
    )
}

impl<'a> WorkflowCtx<'a> {
    /// Rehydrate one run from a chain-verified ledger snapshot. Fails
    /// closed if the ledger's hash chain does not verify, if this
    /// aggregate's rows are not a gap-free 1..=N sequence, or if a row's
    /// payload cannot be decoded — a malformed row is never silently
    /// skipped.
    /// Rehydrate using the pre-registry placeholder definition identity.
    /// `implement-workflow-state-machines` will supply real identities.
    pub(crate) fn new(store: &'a Store, run_id: impl Into<String>) -> Result<Self, WorkflowError> {
        Self::new_with_definition(store, run_id, "workflow", "v1")
    }

    /// Rehydrate a run using its immutable workflow definition identity and
    /// version. These values are part of every deterministic step ID.
    pub(crate) fn new_with_definition(
        store: &'a Store,
        run_id: impl Into<String>,
        definition_id: impl Into<String>,
        definition_version: impl Into<String>,
    ) -> Result<Self, WorkflowError> {
        let run_id = run_id.into();
        let definition_id = definition_id.into();
        let definition_version = definition_version.into();
        if run_id.contains(':') || definition_id.contains(':') || definition_version.contains(':') {
            return Err(WorkflowError::InvalidRunId(run_id));
        }
        let aggregate = format!("workflow_run:{run_id}");
        let rows = match store.verify_and_replay_aggregate(&aggregate) {
            Ok(rows) => rows,
            Err(StoreError::LedgerCorrupted) => return Err(WorkflowError::LedgerCorrupted),
            Err(err) => return Err(WorkflowError::Store(err)),
        };
        for (index, event) in rows.iter().enumerate() {
            let expected = index as u64 + 1;
            if event.aggregate_seq != expected {
                return Err(WorkflowError::MalformedRecord(expected));
            }
        }
        let steps = Self::pair_steps(&rows)?;
        let cursor = 0;
        Ok(Self {
            store,
            run_id,
            definition_id,
            definition_version,
            steps,
            cursor,
        })
    }

    /// Group raw ledger rows into logical steps. A `Pending` row is always
    /// immediately followed by its own `Completed` row if (and only if) the
    /// step finished — nothing else ever writes to this aggregate between
    /// them, since a `WorkflowCtx` drives its own writes strictly in order.
    /// Group raw ledger rows by the stable step ID carried in each record.
    /// Physical append order is irrelevant: concurrent contexts may append
    /// Pending(A), Pending(B), Completed(A), and replay must still pair A
    /// with A rather than relying on adjacency.
    fn pair_steps(rows: &[AuditEvent]) -> Result<Vec<StepEntry>, WorkflowError> {
        use std::collections::{HashMap, HashSet};
        let mut pending: HashMap<String, StepEntry> = HashMap::new();
        let mut completed_ids = HashSet::new();
        for event in rows {
            let kind = event.kind.to_string();
            match Self::decode_record(event)? {
                StepRecord::Pending {
                    step_id,
                    input_digest,
                } => {
                    if pending.contains_key(&step_id) || completed_ids.contains(&step_id) {
                        return Err(WorkflowError::MalformedRecord(event.aggregate_seq));
                    }
                    pending.insert(
                        step_id.clone(),
                        StepEntry {
                            step_id,
                            kind,
                            input_digest,
                            pending_seq: event.aggregate_seq,
                            receipt: None,
                            completed: None,
                        },
                    );
                }
                StepRecord::Receipt {
                    step_id,
                    input_digest,
                    artifact_ref,
                } => {
                    let entry = pending
                        .get_mut(&step_id)
                        .ok_or(WorkflowError::MalformedRecord(event.aggregate_seq))?;
                    if entry.kind != kind
                        || entry.input_digest != input_digest
                        || entry.receipt.is_some()
                        || entry.completed.is_some()
                    {
                        return Err(WorkflowError::MalformedRecord(event.aggregate_seq));
                    }
                    entry.receipt = Some(artifact_ref);
                }
                StepRecord::Completed {
                    step_id,
                    input_digest,
                    outcome,
                } => {
                    let entry = pending
                        .get_mut(&step_id)
                        .ok_or(WorkflowError::MalformedRecord(event.aggregate_seq))?;
                    if entry.kind != kind
                        || entry.input_digest != input_digest
                        || entry.completed.is_some()
                    {
                        return Err(WorkflowError::MalformedRecord(event.aggregate_seq));
                    }
                    entry.completed = Some(outcome);
                    completed_ids.insert(step_id);
                }
            }
        }
        let mut steps: Vec<_> = pending.into_values().collect();
        steps.sort_by_key(|entry| entry.pending_seq);
        Ok(steps)
    }
    /// Borrow the underlying store (state-machine authorization reads the
    /// digest-bound request/approval rows directly).
    pub(crate) fn store(&self) -> &Store {
        self.store
    }

    /// Recover the approval binding persisted when the run entered
    pub(crate) fn entry_binding_for_state(
        &self,
        state: &str,
    ) -> Result<Option<EntryBindingInputs>, WorkflowError> {
        for entry in self.steps.iter().rev() {
            if entry.kind != Self::ENTRY_BINDING_STEP_KIND || entry.completed.is_none() {
                continue;
            }
            let Some(Outcome::Ok(value)) = entry.completed.as_ref() else {
                continue;
            };
            let binding = serde_json::from_value::<EntryBindingInputs>(value.clone())
                .map_err(|_| WorkflowError::MalformedRecord(entry.pending_seq))?;
            if binding.target_state == state {
                return Ok(Some(binding));
            }
        }
        Ok(None)
    }

    /// Last completed declarative transition target across reserved state
    /// machine kinds. Malformed completed outcomes fail closed.
    pub(crate) fn last_completed_transition_target(&self) -> Result<Option<String>, WorkflowError> {
        for entry in self.steps.iter().rev() {
            if entry.completed.is_none()
                || !(entry.kind == Self::TRANSITION_STEP_KIND
                    || entry.kind == Self::APPROVAL_STEP_KIND
                    || entry.kind == Self::ENTRY_BINDING_STEP_KIND)
            {
                continue;
            }
            let Some(Outcome::Ok(value)) = entry.completed.as_ref() else {
                continue;
            };
            let target = match entry.kind.as_str() {
                Self::ENTRY_BINDING_STEP_KIND => {
                    serde_json::from_value::<EntryBindingInputs>(value.clone())
                        .map_err(|_| WorkflowError::MalformedRecord(entry.pending_seq))?
                        .target_state
                }
                _ => {
                    serde_json::from_value::<TransitionOutcome>(value.clone())
                        .map_err(|_| WorkflowError::MalformedRecord(entry.pending_seq))?
                        .target
                }
            };
            return Ok(Some(target));
        }
        Ok(None)
    }

    /// Validate every completed declarative record against the manifest and
    /// current Store-backed approval binding before state reconstruction.
    pub(crate) fn validate_state_machine_replay(
        &self,
        manifest: &openspine_schemas::workflow::WorkflowManifest,
    ) -> Result<(), WorkflowError> {
        let mut current = manifest.initial_state.clone().ok_or_else(|| {
            WorkflowError::Step("declarative workflow has no initial state".into())
        })?;
        let mut current_binding: Option<EntryBindingInputs> = None;
        for entry in &self.steps {
            let Some(Outcome::Ok(value)) = entry.completed.as_ref() else {
                continue;
            };
            match entry.kind.as_str() {
                Self::DEFINITION_STEP_KIND => {
                    serde_json::from_value::<Digest>(value.clone())
                        .map_err(|_| WorkflowError::MalformedRecord(entry.pending_seq))?;
                }
                Self::TRANSITION_STEP_KIND => {
                    let outcome = serde_json::from_value::<TransitionOutcome>(value.clone())
                        .map_err(|_| WorkflowError::MalformedRecord(entry.pending_seq))?;
                    let source = manifest
                        .states
                        .iter()
                        .find(|state| state.id == current)
                        .ok_or(WorkflowError::MalformedRecord(entry.pending_seq))?;
                    let target = manifest
                        .states
                        .iter()
                        .find(|state| state.id == outcome.target)
                        .ok_or(WorkflowError::MalformedRecord(entry.pending_seq))?;
                    if source.approval == ApprovalSemantics::Required
                        || target.approval == ApprovalSemantics::Required
                        || !manifest
                            .transitions
                            .iter()
                            .any(|edge| edge.from == current && edge.to == outcome.target)
                    {
                        return Err(WorkflowError::Divergence {
                            ordinal: entry.pending_seq,
                            expected: current,
                            actual: outcome.target,
                        });
                    }
                    current = outcome.target;
                    current_binding = None;
                }
                Self::ENTRY_BINDING_STEP_KIND => {
                    let binding = serde_json::from_value::<EntryBindingInputs>(value.clone())
                        .map_err(|_| WorkflowError::MalformedRecord(entry.pending_seq))?;
                    let source = manifest
                        .states
                        .iter()
                        .find(|state| state.id == current)
                        .ok_or(WorkflowError::MalformedRecord(entry.pending_seq))?;
                    let target = manifest
                        .states
                        .iter()
                        .find(|state| state.id == binding.target_state)
                        .ok_or(WorkflowError::MalformedRecord(entry.pending_seq))?;
                    if source.approval == ApprovalSemantics::Required
                        || target.approval != ApprovalSemantics::Required
                        || !manifest
                            .transitions
                            .iter()
                            .any(|edge| edge.from == current && edge.to == binding.target_state)
                    {
                        return Err(WorkflowError::Divergence {
                            ordinal: entry.pending_seq,
                            expected: current,
                            actual: binding.target_state,
                        });
                    }
                    current = binding.target_state.clone();
                    current_binding = Some(binding);
                }
                Self::APPROVAL_STEP_KIND => {
                    let outcome = serde_json::from_value::<TransitionOutcome>(value.clone())
                        .map_err(|_| WorkflowError::MalformedRecord(entry.pending_seq))?;
                    let source = manifest
                        .states
                        .iter()
                        .find(|state| state.id == current)
                        .ok_or(WorkflowError::MalformedRecord(entry.pending_seq))?;
                    let target = manifest
                        .states
                        .iter()
                        .find(|state| state.id == outcome.target)
                        .ok_or(WorkflowError::MalformedRecord(entry.pending_seq))?;
                    if source.approval != ApprovalSemantics::Required
                        || target.approval == ApprovalSemantics::Required
                        || !manifest
                            .transitions
                            .iter()
                            .any(|edge| edge.from == current && edge.to == outcome.target)
                    {
                        return Err(WorkflowError::Divergence {
                            ordinal: entry.pending_seq,
                            expected: current,
                            actual: outcome.target,
                        });
                    }
                    let binding = current_binding
                        .take()
                        .ok_or(WorkflowError::MalformedRecord(entry.pending_seq))?;
                    let expected_digest = digest_inputs(&GatedDepartureInputs {
                        from: current.clone(),
                        to: outcome.target.clone(),
                        request_id: binding.request_id.clone(),
                        action: binding.action.clone(),
                        payload_digest: binding.payload_digest.clone(),
                        target_digest: binding.target_digest.clone(),
                    })?;
                    if entry.input_digest != expected_digest {
                        return Err(WorkflowError::Divergence {
                            ordinal: entry.pending_seq,
                            expected: entry.input_digest.clone(),
                            actual: expected_digest,
                        });
                    }
                    current = outcome.target;
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn decode_record(event: &AuditEvent) -> Result<StepRecord, WorkflowError> {
        let payload = event
            .payload_json
            .as_deref()
            .ok_or(WorkflowError::MalformedRecord(event.aggregate_seq))?;
        serde_json::from_str(payload)
            .map_err(|_| WorkflowError::MalformedRecord(event.aggregate_seq))
    }
    /// Advance the cursor for a wrapper that has reconstructed its state
    /// independently. Ordinary callers must replay from cursor zero.
    pub(crate) fn resume_after_completed_steps(&mut self) {
        self.cursor = self
            .steps
            .iter()
            .position(|entry| entry.completed.is_none())
            .unwrap_or(self.steps.len());
    }

    /// Check whether the step at the current cursor position has already
    /// been durably completed. Never executes anything. On a genuinely
    /// fresh step, durably appends the `Pending` outbox intent BEFORE
    /// returning — the caller's subsequent work is covered by that intent
    /// record regardless of whether `complete_step` ever runs.
    /// Legacy approval kind retained for archived generic workflow callers.
    /// It is deliberately not a declarative state-machine transition kind.
    const LEGACY_APPROVAL_STEP_KIND: &'static str = "workflow.approval_legacy";
    /// Private kind used only by the state-machine departure adapter.
    const APPROVAL_STEP_KIND: &'static str = "workflow.approval";
    /// Reserved kind for declarative state-machine transitions (findings of
    /// `implement-workflow-state-machines`): only
    /// [`Self::begin_transition_step`] may emit it.
    pub(crate) const TRANSITION_STEP_KIND: &'static str = "workflow.transition";
    /// Reserved kind for a crash-safe entry into an approval-semantic state
    /// that simultaneously binds the approval request, action, and digests
    /// (D-011). Only [`Self::begin_entry_binding_step`] may emit it.
    pub(crate) const ENTRY_BINDING_STEP_KIND: &'static str = "workflow.entry_binding";
    /// Reserved kind recording the immutable manifest digest a run is bound
    /// to. Only [`Self::begin_definition_step`] may emit it.
    pub(crate) const DEFINITION_STEP_KIND: &'static str = "workflow.definition";
    /// Closed, non-sensitive outcome code persisted when a gated step's
    /// effect fails. Never carries provider internals or plaintext content.
    const GATED_STEP_FAILED: &'static str = "workflow.gated_step_failed";
    /// Closed, non-sensitive private-step failure code. Private error
    /// messages never enter the workflow ledger.
    const PRIVATE_STEP_FAILED: &'static str = "workflow.private_step_failed";

    /// Check whether the step at the current cursor position has already
    /// been durably completed. Never executes anything. On a genuinely
    /// fresh step, durably appends the `Pending` outbox intent BEFORE
    /// returning — the caller's subsequent work is covered by that intent
    /// record regardless of whether `complete_step` ever runs.
    ///
    /// Generic steps reject the reserved approval kind; approval-bound steps
    /// MUST use [`Self::begin_approval_step`].
    pub(crate) fn begin_step<T>(
        &mut self,
        kind: &str,
        inputs: &impl Serialize,
    ) -> Result<StepState<T>, WorkflowError>
    where
        T: WorkflowInlinePayload,
    {
        if Self::is_reserved_kind(kind) {
            return Err(WorkflowError::Step(format!(
                "the workflow kind {kind} is reserved for a typed adapter"
            )));
        }
        self.begin_step_raw::<T>(kind, inputs)
    }

    /// Typed approval-gated departure adapter (D-011): binds the exact
    /// departure edge, the entry-bound request id, and the digest-bound
    /// action/payload/target so a crashed-Pending departure cannot resume
    /// against a different edge or approval.
    fn begin_gated_departure_step(
        &mut self,
        inputs: &GatedDepartureInputs,
    ) -> Result<StepState<TransitionOutcome>, WorkflowError> {
        self.begin_step_raw::<TransitionOutcome>(Self::APPROVAL_STEP_KIND, inputs)
    }

    /// Authorize and append a declarative departure as one kernel-owned
    /// boundary. Callers cannot manufacture the reserved approval record
    /// without a currently valid Store-backed D-011 approval.
    pub(crate) fn begin_authorized_gated_departure_step(
        &mut self,
        from: &str,
        target: &str,
        binding: &EntryBindingInputs,
    ) -> Result<StepState<TransitionOutcome>, WorkflowError> {
        let request_id = binding
            .request_id
            .parse::<ulid::Ulid>()
            .map_err(|_| WorkflowError::Step("malformed approval request id".to_string()))?;
        if binding.target_state != from {
            return Err(WorkflowError::Step(
                "approval binding source does not match departure source".to_string(),
            ));
        }
        let request = self
            .store
            .find_action_request(request_id)?
            .ok_or_else(|| WorkflowError::Step("approval request is missing".to_string()))?;
        if request.action.as_str() != binding.action {
            return Err(WorkflowError::Step(
                "approval request action does not match binding".to_string(),
            ));
        }
        let approval = self
            .store
            .find_approval_for_request(request_id)?
            .ok_or_else(|| WorkflowError::Step("approval record is missing".to_string()))?;
        let request_payload = request
            .payload_ref
            .as_ref()
            .map(|reference| reference.digest.clone())
            .ok_or_else(|| WorkflowError::Step("approval payload digest is missing".to_string()))?;
        let request_target = request
            .target_digest
            .clone()
            .ok_or_else(|| WorkflowError::Step("approval target digest is missing".to_string()))?;
        if request_payload != binding.payload_digest || request_target != binding.target_digest {
            return Err(WorkflowError::Step(
                "approval request digests do not match binding".to_string(),
            ));
        }
        if !approval.matches(
            &binding.payload_digest,
            &binding.target_digest,
            Timestamp::now(),
        ) {
            return Err(WorkflowError::Step(
                "approval record does not match binding".to_string(),
            ));
        }
        let inputs = GatedDepartureInputs {
            from: from.to_string(),
            to: target.to_string(),
            request_id: binding.request_id.clone(),
            action: binding.action.clone(),
            payload_digest: binding.payload_digest.clone(),
            target_digest: binding.target_digest.clone(),
        };
        self.begin_gated_departure_step(&inputs)
    }

    /// Legacy typed approval adapter. It uses a non-state-machine kind so
    /// generic callers cannot manufacture a declarative departure record.
    pub(crate) fn begin_approval_step<T: WorkflowInlinePayload>(
        &mut self,
        action: &str,
        target_digest: Digest,
        payload_digest: Digest,
    ) -> Result<StepState<T>, WorkflowError> {
        let inputs = ApprovalStepInputs {
            action: action.to_string(),
            target_digest,
            payload_digest,
        };
        self.begin_step_raw::<T>(Self::LEGACY_APPROVAL_STEP_KIND, &inputs)
    }
    fn is_reserved_kind(kind: &str) -> bool {
        matches!(
            kind,
            Self::LEGACY_APPROVAL_STEP_KIND
                | Self::APPROVAL_STEP_KIND
                | Self::TRANSITION_STEP_KIND
                | Self::ENTRY_BINDING_STEP_KIND
                | Self::DEFINITION_STEP_KIND
        )
    }

    /// Typed state-machine transition adapter. Binds the source and target
    /// state ids into the step input digest so a crashed-Pending transition
    /// cannot resume against a different edge.
    pub(crate) fn begin_transition_step(
        &mut self,
        from: &str,
        to: &str,
    ) -> Result<StepState<TransitionOutcome>, WorkflowError> {
        let inputs = TransitionStepInputs {
            from: from.to_string(),
            to: to.to_string(),
        };
        self.begin_step_raw::<TransitionOutcome>(Self::TRANSITION_STEP_KIND, &inputs)
    }
    /// binding (request id, action, payload/target digests). Departure may
    /// only proceed against the exact binding persisted here.
    pub(crate) fn begin_entry_binding_step(
        &mut self,
        from: &str,
        binding: &EntryBindingInputs,
    ) -> Result<StepState<EntryBindingInputs>, WorkflowError> {
        let inputs = EntryTransitionInputs {
            from: from.to_string(),
            binding: binding.clone(),
        };
        self.begin_step_raw::<EntryBindingInputs>(Self::ENTRY_BINDING_STEP_KIND, &inputs)
    }

    /// Typed run-definition adapter: binds the run to an immutable manifest
    /// digest recorded once at run start and verified on resumption.
    pub(crate) fn begin_definition_step<T: WorkflowInlinePayload>(
        &mut self,
        manifest_digest: &Digest,
    ) -> Result<StepState<T>, WorkflowError> {
        self.begin_step_raw::<T>(Self::DEFINITION_STEP_KIND, manifest_digest)
    }

    fn begin_step_raw<T: Serialize + DeserializeOwned>(
        &mut self,
        kind: &str,
        inputs: &impl Serialize,
    ) -> Result<StepState<T>, WorkflowError> {
        let input_digest = digest_inputs(inputs)?;
        let step_id = format!(
            "{}:{}:{}",
            self.definition_id, self.definition_version, self.cursor
        );
        let existing_index = self.steps.iter().position(|entry| entry.step_id == step_id);
        if let Some(existing_index) = existing_index {
            let entry = &self.steps[existing_index];
            if entry.kind != kind {
                return Err(WorkflowError::Divergence {
                    ordinal: entry.pending_seq,
                    expected: entry.kind.clone(),
                    actual: kind.to_string(),
                });
            }
            if entry.input_digest != input_digest {
                return Err(WorkflowError::Divergence {
                    ordinal: entry.pending_seq,
                    expected: entry.input_digest.clone(),
                    actual: input_digest,
                });
            }
            let handle = StepHandle {
                step_id: entry.step_id.clone(),
                pending_seq: entry.pending_seq,
            };
            self.cursor += 1;
            return match entry.completed.clone() {
                Some(outcome) => {
                    let seq = entry.pending_seq;
                    Ok(StepState::Replayed {
                        handle,
                        outcome: match outcome {
                            Outcome::Ok(value) => Ok(serde_json::from_value(value)
                                .map_err(|_| WorkflowError::MalformedRecord(seq))?),
                            Outcome::Err(message) => Err(message),
                        },
                    })
                }
                None => Ok(StepState::Resuming {
                    idempotency_key: format!("{}:{}", self.run_id, entry.step_id),
                    handle,
                }),
            };
        }
        if self.cursor < self.steps.len() {
            return Err(WorkflowError::Divergence {
                ordinal: self.steps[self.cursor].pending_seq,
                expected: self.steps[self.cursor].step_id.clone(),
                actual: step_id,
            });
        }
        let record = StepRecord::Pending {
            step_id: step_id.clone(),
            input_digest: input_digest.clone(),
        };
        let payload = serde_json::to_string(&record)?;
        let (event, inserted) =
            self.store
                .append_workflow_step_if_absent(&self.run_id, kind, &payload, &step_id)?;
        let pending_seq = event.aggregate_seq;
        let persisted_digest = match Self::decode_record(&event)? {
            StepRecord::Pending {
                step_id: persisted_id,
                input_digest: persisted_digest,
            } if persisted_id == step_id && event.kind.to_string() == kind => persisted_digest,
            _ => {
                return Err(WorkflowError::Divergence {
                    ordinal: pending_seq,
                    expected: step_id,
                    actual: event.kind.to_string(),
                });
            }
        };
        if persisted_digest != input_digest {
            return Err(WorkflowError::Divergence {
                ordinal: pending_seq,
                expected: persisted_digest,
                actual: input_digest,
            });
        }
        self.steps.push(StepEntry {
            step_id: step_id.clone(),
            kind: kind.to_string(),
            input_digest: persisted_digest,
            pending_seq,
            receipt: None,
            completed: None,
        });
        let handle = StepHandle {
            step_id: step_id.clone(),
            pending_seq,
        };
        self.cursor += 1;
        if inserted {
            Ok(StepState::Fresh {
                idempotency_key: format!("{}:{}", self.run_id, step_id),
                handle,
            })
        } else {
            Ok(StepState::Resuming {
                idempotency_key: format!("{}:{}", self.run_id, step_id),
                handle,
            })
        }
    }
    fn entry_index(&self, handle: &StepHandle) -> Result<usize, WorkflowError> {
        self.steps
            .iter()
            .position(|entry| {
                entry.step_id == handle.step_id && entry.pending_seq == handle.pending_seq
            })
            .ok_or_else(|| WorkflowError::Step("unknown workflow step handle".to_string()))
    }
    /// Persist a protected receipt pointer before writing terminal completion.
    pub(crate) fn record_receipt(
        &mut self,
        handle: &StepHandle,
        artifact_ref: ArtifactRef,
    ) -> Result<(), WorkflowError> {
        let entry_index = self.entry_index(handle)?;
        let entry = &self.steps[entry_index];
        if entry.completed.is_some() || entry.receipt.is_some() {
            return Err(WorkflowError::Divergence {
                ordinal: entry.pending_seq,
                expected: "pending step without receipt".to_string(),
                actual: "receipt already recorded or step completed".to_string(),
            });
        }
        let requested_ref = artifact_ref.clone();
        let payload = serde_json::to_string(&StepRecord::Receipt {
            step_id: handle.step_id.clone(),
            input_digest: entry.input_digest.clone(),
            artifact_ref: requested_ref.clone(),
        })?;
        let (event, _inserted) = self.store.append_workflow_receipt(
            &self.run_id,
            &entry.kind,
            &payload,
            &handle.step_id,
        )?;
        let canonical = event
            .payload_json
            .as_deref()
            .ok_or(WorkflowError::MalformedRecord(entry.pending_seq))
            .and_then(|raw| {
                serde_json::from_str::<StepRecord>(raw)
                    .map_err(|_| WorkflowError::MalformedRecord(entry.pending_seq))
            })?;
        let receipt = match canonical {
            StepRecord::Receipt {
                step_id,
                input_digest,
                artifact_ref: canonical_ref,
            } if step_id == handle.step_id
                && input_digest == entry.input_digest
                && canonical_ref == requested_ref =>
            {
                canonical_ref
            }
            _ => {
                return Err(WorkflowError::Divergence {
                    ordinal: entry.pending_seq,
                    expected: "matching workflow receipt".to_string(),
                    actual: "conflicting workflow receipt".to_string(),
                });
            }
        };
        self.steps[entry_index].receipt = Some(receipt);
        Ok(())
    }
    pub(crate) fn complete_step<T>(
        &mut self,
        handle: &StepHandle,
        outcome: Result<T, String>,
    ) -> Result<(), WorkflowError>
    where
        T: WorkflowInlinePayload,
    {
        let entry_index = self.entry_index(handle)?;
        let entry = &self.steps[entry_index];
        if entry.completed.is_some() {
            return Err(WorkflowError::Step(
                "workflow step already completed".to_string(),
            ));
        }
        let value = match &outcome {
            Ok(v) => Outcome::Ok(serde_json::to_value(v)?),
            Err(message) => Outcome::Err(message.clone()),
        };
        if let Some(receipt) = &entry.receipt {
            let expected = serde_json::to_value(receipt)?;
            if value != Outcome::Ok(expected) {
                return Err(WorkflowError::MalformedRecord(entry.pending_seq));
            }
        }
        let record = StepRecord::Completed {
            step_id: handle.step_id.clone(),
            input_digest: entry.input_digest.clone(),
            outcome: value.clone(),
        };
        let payload = serde_json::to_string(&record)?;
        let (event, _inserted) = self.store.append_workflow_completion(
            &self.run_id,
            &entry.kind,
            &payload,
            &handle.step_id,
        )?;
        let canonical = event
            .payload_json
            .as_deref()
            .ok_or(WorkflowError::MalformedRecord(entry.pending_seq))
            .and_then(|raw| {
                serde_json::from_str::<StepRecord>(raw)
                    .map_err(|_| WorkflowError::MalformedRecord(entry.pending_seq))
            })?;
        match canonical {
            StepRecord::Completed {
                step_id,
                input_digest,
                outcome,
            } if step_id == handle.step_id
                && input_digest == entry.input_digest
                && outcome == value =>
            {
                self.steps[entry_index].completed = Some(outcome);
                Ok(())
            }
            _ => Err(WorkflowError::Divergence {
                ordinal: entry.pending_seq,
                expected: "matching workflow completion".to_string(),
                actual: "conflicting workflow completion".to_string(),
            }),
        }
    }
    /// Like [`Self::begin_step`], but for outcomes that may carry private
    /// content: the ledger stores only an [`ArtifactRef`]; replay fetches
    /// and digest-verifies the real bytes from the encrypted artifact
    /// store, failing closed if the blob is missing or tampered.
    pub(crate) fn begin_private_step<T: Serialize + DeserializeOwned>(
        &mut self,
        kind: &str,
        inputs: &impl Serialize,
        artifacts: &ArtifactStore,
    ) -> Result<StepState<T>, WorkflowError> {
        match self.begin_step::<ArtifactRef>(kind, inputs)? {
            StepState::Replayed { handle, outcome } => match outcome {
                Ok(artifact_ref) => {
                    let bytes = artifacts
                        .get(&artifact_ref)
                        .map_err(|err: ArtifactStoreError| WorkflowError::Step(err.to_string()))?;
                    let value = serde_json::from_slice(&bytes)?;
                    Ok(StepState::Replayed {
                        handle,
                        outcome: Ok(value),
                    })
                }
                Err(message) => Ok(StepState::Replayed {
                    handle,
                    outcome: Err(message),
                }),
            },
            StepState::Fresh {
                handle,
                idempotency_key,
            } => Ok(StepState::Fresh {
                handle,
                idempotency_key,
            }),
            StepState::Resuming {
                handle,
                idempotency_key,
            } => {
                let entry_index = self.entry_index(&handle)?;
                if let Some(receipt) = self.steps[entry_index].receipt.clone() {
                    let bytes = artifacts
                        .get(&receipt)
                        .map_err(|err: ArtifactStoreError| WorkflowError::Step(err.to_string()))?;
                    let value = serde_json::from_slice(&bytes)?;
                    self.complete_step(&handle, Ok::<_, String>(receipt))?;
                    Ok(StepState::Replayed {
                        handle,
                        outcome: Ok(value),
                    })
                } else {
                    Ok(StepState::Resuming {
                        handle,
                        idempotency_key,
                    })
                }
            }
        }
    }
    pub(crate) fn complete_private_step<T: Serialize>(
        &mut self,
        handle: &StepHandle,
        outcome: Result<T, String>,
        artifacts: &ArtifactStore,
    ) -> Result<(), WorkflowError> {
        let stored: Result<ArtifactRef, String> = match outcome {
            Ok(value) => {
                let bytes = serde_json::to_vec(&value)?;
                let receipt = artifacts
                    .put(&bytes)
                    .map_err(|err: ArtifactStoreError| WorkflowError::Step(err.to_string()))?;
                self.record_receipt(handle, receipt.clone())?;
                Ok(receipt)
            }
            Err(_message) => Err(Self::PRIVATE_STEP_FAILED.to_string()),
        };
        self.complete_step(handle, stored)
    }
    /// Run one action through the production gate/dispatcher and persist only
    /// an encrypted artifact reference. The action is fixed by typed wrappers;
    /// no arbitrary effect closure can diverge from the authorized request.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn run_gated_step(
        &mut self,
        state: &AppState,
        grant: &TaskGrant,
        artifacts: &ArtifactStore,
        action: ActionId,
        bound_chat_id: i64,
        payload: Option<&Value>,
        inputs: &impl Serialize,
    ) -> Result<StepState<ArtifactRef>, WorkflowError> {
        let payload_digest = payload.map(|p| {
            openspine_schemas::digest::digest_of_bytes(canonical_json(p).as_bytes())
                .as_str()
                .to_string()
        });
        let gated = GatedStepDigest {
            action: action.to_string(),
            grant_id: grant.id.to_string(),
            bound_chat_id,
            inputs_digest: digest_inputs(inputs)?,
            payload_digest,
        };
        match self.begin_step::<ArtifactRef>(&action.to_string(), &gated)? {
            StepState::Replayed { handle, outcome } => Ok(StepState::Replayed { handle, outcome }),
            StepState::Resuming { handle, .. } => {
                let entry_index = self.entry_index(&handle)?;
                let Some(receipt) = self.steps[entry_index].receipt.clone() else {
                    return Err(WorkflowError::Step(format!(
                        "gated step {action} has no durable receipt; refusing to re-dispatch"
                    )));
                };
                artifacts
                    .get(&receipt)
                    .map_err(|err: ArtifactStoreError| WorkflowError::Step(err.to_string()))?;
                self.complete_step(&handle, Ok::<_, String>(receipt.clone()))?;
                Ok(StepState::Replayed {
                    handle,
                    outcome: Ok(receipt),
                })
            }
            StepState::Fresh { handle, .. } => {
                let dispatched = mediate_and_dispatch_action(
                    state,
                    grant,
                    action.clone(),
                    bound_chat_id,
                    payload,
                    FailureSurface::Detached,
                    None,
                )
                .await;
                // The persisted/replayed outcome is a closed, non-sensitive
                // code on failure (never provider internals or plaintext). The
                // detailed error is logged for diagnostics only.
                let outcome: Result<ArtifactRef, String> = match dispatched {
                    Ok((openspine_schemas::action::GateDecision::Allow, _, Some(value), _)) => {
                        match serde_json::to_vec(&value) {
                            Ok(bytes) => match artifacts.put(&bytes) {
                                Ok(receipt) => {
                                    match self.record_receipt(&handle, receipt.clone()) {
                                        Ok(()) => Ok(receipt),
                                        Err(err) => {
                                            tracing::warn!(error = %err, "gated step receipt failed");
                                            Err(Self::GATED_STEP_FAILED.to_string())
                                        }
                                    }
                                }
                                Err(err) => {
                                    tracing::warn!(error = %err, "gated step artifact store failed");
                                    Err(Self::GATED_STEP_FAILED.to_string())
                                }
                            },
                            Err(err) => {
                                tracing::warn!(error = %err, "gated step serialize failed");
                                Err(Self::GATED_STEP_FAILED.to_string())
                            }
                        }
                    }
                    Ok((decision, _, _, _)) => {
                        tracing::warn!(decision = ?decision, "gated step not executed");
                        Err(Self::GATED_STEP_FAILED.to_string())
                    }
                    Err(err) => {
                        tracing::warn!(error = ?err, "gated step dispatch failed");
                        Err(Self::GATED_STEP_FAILED.to_string())
                    }
                };
                // Completion append errors propagate typed (Store, etc.); the
                // returned Outcome is exactly what replay will return.
                self.complete_step(&handle, outcome.clone())?;
                Ok(StepState::Replayed { handle, outcome })
            }
        }
    }

    pub(crate) async fn run_status_read_step(
        &mut self,
        state: &AppState,
        grant: &TaskGrant,
        artifacts: &ArtifactStore,
        bound_chat_id: i64,
        inputs: &impl Serialize,
    ) -> Result<StepState<ArtifactRef>, WorkflowError> {
        self.run_gated_step(
            state,
            grant,
            artifacts,
            ActionId::new("openspine.status.read"),
            bound_chat_id,
            None,
            inputs,
        )
        .await
    }

    pub(crate) fn now(&mut self) -> Result<Timestamp, WorkflowError> {
        match self.begin_step::<Timestamp>("workflow.time_read", &())? {
            StepState::Replayed { outcome, .. } => match outcome {
                Ok(ts) => Ok(ts),
                Err(message) => Err(WorkflowError::Step(message)),
            },
            StepState::Fresh { handle, .. } | StepState::Resuming { handle, .. } => {
                let ts = Timestamp::now();
                self.complete_step(&handle, Ok::<_, String>(ts))?;
                Ok(ts)
            }
        }
    }

    /// Kernel-mediated randomness — a pure read, always safe to redo.
    pub(crate) fn random_u64(&mut self) -> Result<u64, WorkflowError> {
        match self.begin_step::<u64>("workflow.random_read", &())? {
            StepState::Replayed { outcome, .. } => match outcome {
                Ok(value) => Ok(value),
                Err(message) => Err(WorkflowError::Step(message)),
            },
            StepState::Fresh { handle, .. } | StepState::Resuming { handle, .. } => {
                let value = rand::random();
                self.complete_step(&handle, Ok::<_, String>(value))?;
                Ok(value)
            }
        }
    }

    /// Durably schedule an AD-012 dark-window timer.
    pub(crate) fn schedule_timer(
        &mut self,
        fires_at: Timestamp,
    ) -> Result<TimerHandle, WorkflowError> {
        match self.begin_step::<TimerSpec>("workflow.timer_scheduled", &fires_at)? {
            StepState::Replayed { outcome, .. } => match outcome {
                Ok(spec) => Ok(TimerHandle {
                    run_id: self.run_id.clone(),
                    timer_id: spec.timer_id,
                    fires_at: spec.fires_at,
                }),
                Err(message) => Err(WorkflowError::Step(message)),
            },
            StepState::Fresh { handle, .. } | StepState::Resuming { handle, .. } => {
                let entry_index = self.entry_index(&handle)?;
                let entry = &self.steps[entry_index];
                let (event, _inserted) = self.store.schedule_workflow_timer_step(
                    &self.run_id,
                    &handle.step_id,
                    handle.pending_seq,
                    "workflow.timer_scheduled",
                    &entry.input_digest,
                    fires_at,
                )?;
                let spec: TimerSpec = match Self::decode_record(&event)? {
                    StepRecord::Completed {
                        step_id,
                        input_digest,
                        outcome: Outcome::Ok(value),
                    } if step_id == handle.step_id && input_digest == entry.input_digest => {
                        serde_json::from_value(value)?
                    }
                    _ => {
                        return Err(WorkflowError::Divergence {
                            ordinal: handle.pending_seq,
                            expected: "matching timer completion".to_string(),
                            actual: "conflicting timer completion".to_string(),
                        });
                    }
                };
                self.steps[entry_index].completed = Some(Outcome::Ok(serde_json::to_value(&spec)?));
                Ok(TimerHandle {
                    run_id: self.run_id.clone(),
                    timer_id: spec.timer_id,
                    fires_at: spec.fires_at,
                })
            }
        }
    }
    /// Observe whether `timer` has durably fired. Consumers only schedule
    /// and observe (D-074): firing belongs exclusively to the kernel timer
    /// driver's trusted-clock `fire_due_timers` path, so no caller-supplied
    /// timestamp can claim a timer before its real deadline.
    pub(crate) fn poll_timer(&self, timer: &TimerHandle) -> Result<bool, WorkflowError> {
        if timer.run_id != self.run_id {
            return Ok(false);
        }
        Ok(self.store.workflow_timer_fired(timer.timer_id())?)
    }
}

/// Reusable kernel-owned timer driver: sleeps until the earliest pending
/// timer's deadline (or `poll_interval`, whichever is sooner), fires every
/// due timer, and repeats forever. Spawned from `main()`'s startup
/// `tokio::select!`.
pub(crate) async fn run_timer_driver(
    store: &Store,
    poll_interval: std::time::Duration,
) -> anyhow::Result<()> {
    loop {
        let now = Timestamp::now();
        run_timer_driver_iteration(store, now)?;
        let sleep_for = store
            .next_timer_deadline()
            .ok()
            .flatten()
            .and_then(|deadline| {
                let remaining = deadline.duration_since(now);
                std::time::Duration::try_from(remaining).ok()
            })
            .filter(|remaining| *remaining < poll_interval)
            .unwrap_or(poll_interval);
        tokio::time::sleep(sleep_for).await;
    }
}

pub(crate) fn run_timer_driver_iteration(store: &Store, now: Timestamp) -> anyhow::Result<()> {
    store.observe_runtime_clock(now.as_millisecond())?;
    if let Err(err) = store.fire_due_timers(now) {
        tracing::error!("workflow timer driver: {err}");
    }
    Ok(())
}

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "workflow_crash_tests.rs"]
mod crash_tests;

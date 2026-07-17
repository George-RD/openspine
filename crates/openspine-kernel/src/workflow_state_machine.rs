use std::collections::HashMap;
use std::str::FromStr;

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::approval::ApprovalRecord;
use openspine_schemas::digest::{digest_of, Digest};
use openspine_schemas::workflow::{ApprovalSemantics, WorkflowManifest, WorkflowStep};
use ulid::Ulid;

use crate::model_gateway::{GatewayTierMap, ProviderClient};
use crate::store::{Store, StoreError};
use crate::workflow::{
    EntryBindingInputs, StepState, TransitionOutcome, WorkflowCtx, WorkflowError,
};

#[derive(Debug, thiserror::Error)]
pub enum WorkflowStateMachineError {
    #[error("workflow definition is invalid: {0}")]
    InvalidDefinition(String),
    #[error("workflow has no current state")]
    NoCurrentState,
    #[error("workflow transition to {0} is not declared from the current state")]
    UnknownTransition(String),
    #[error("approval is required before leaving state {0}")]
    ApprovalRequired(String),
    #[error("entering approval state {0} requires an action request id binding")]
    ApprovalEntryBindingRequired(String),
    #[error("approval request was not found")]
    ApprovalMissing,
    #[error("approval request action does not match the declared action")]
    ApprovalActionMismatch,
    #[error("approval request has no digest-bound payload or target")]
    ApprovalDigestMissing,
    #[error("approval does not match the request's payload and target digests")]
    ApprovalDigestMismatch,
    #[error("departure must match the approval request bound at entry, not a caller-supplied id")]
    ApprovalBindingMismatch,
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("workflow error: {0}")]
    Workflow(#[from] WorkflowError),
}

/// Durable execution wrapper for a declarative [`WorkflowManifest`].
///
/// Each `transition_to` writes exactly ONE durable step that advances the
/// current state, so there is no crash window between "entered a state" and
/// "bound its approval semantics". Approval departures are authorized
/// against Store-backed D-011 approvals before any durable write.
pub struct WorkflowStateMachine<'a> {
    ctx: WorkflowCtx<'a>,
    manifest: WorkflowManifest,
    current_state: Option<String>,
}

impl<'a> WorkflowStateMachine<'a> {
    pub fn new(
        store: &'a Store,
        run_id: impl Into<String>,
        manifest: WorkflowManifest,
    ) -> Result<Self, WorkflowStateMachineError> {
        manifest
            .validate()
            .map_err(WorkflowStateMachineError::InvalidDefinition)?;
        let mut ctx = WorkflowCtx::new_with_definition(
            store,
            run_id,
            manifest.id.clone(),
            manifest.version.to_string(),
        )?;
        Self::bind_manifest_digest(&mut ctx, &manifest)?;
        ctx.validate_state_machine_replay(&manifest)?;
        let current_state = Self::reconstruct_current_state(&ctx, &manifest)?;
        ctx.resume_after_completed_steps();
        Ok(Self {
            ctx,
            manifest,
            current_state,
        })
    }

    fn bind_manifest_digest(
        ctx: &mut WorkflowCtx<'_>,
        manifest: &WorkflowManifest,
    ) -> Result<(), WorkflowStateMachineError> {
        let manifest_digest = manifest_digest(manifest)?;
        let definition_step = ctx
            .begin_definition_step::<Digest>(&manifest_digest)
            .map_err(|error| match error {
                WorkflowError::Divergence { .. } => WorkflowStateMachineError::InvalidDefinition(
                    format!("manifest binding diverged: {error}"),
                ),
                other => WorkflowStateMachineError::Workflow(other),
            })?;
        match definition_step {
            StepState::Replayed { outcome, .. } => {
                let recorded = outcome.map_err(|_| {
                    WorkflowStateMachineError::InvalidDefinition(
                        "run definition step recorded a failure".to_string(),
                    )
                })?;
                if recorded != manifest_digest {
                    return Err(WorkflowStateMachineError::InvalidDefinition(format!(
                        "manifest digest changed since this run started: expected {recorded}, got {manifest_digest}"
                    )));
                }
            }
            StepState::Fresh { handle, .. } | StepState::Resuming { handle, .. } => {
                ctx.complete_step(&handle, Ok::<_, String>(manifest_digest))?;
            }
        }
        Ok(())
    }

    fn reconstruct_current_state(
        ctx: &WorkflowCtx<'_>,
        manifest: &WorkflowManifest,
    ) -> Result<Option<String>, WorkflowStateMachineError> {
        match ctx.last_completed_transition_target()? {
            None => Ok(manifest.initial_state.clone()),
            Some(target) => {
                if !manifest.states.iter().any(|state| state.id == target) {
                    return Err(WorkflowStateMachineError::InvalidDefinition(format!(
                        "recovered transition target {target} is not a declared state"
                    )));
                }
                Ok(Some(target))
            }
        }
    }

    pub fn current_state(&self) -> Option<&str> {
        self.current_state.as_deref()
    }

    pub fn manifest(&self) -> &WorkflowManifest {
        &self.manifest
    }

    /// Select the provider for a declared step using the current active
    /// provider as the safe fallback for the static tier map.
    pub fn provider_for_step<'p>(
        &self,
        step_id: &str,
        tier_map: &GatewayTierMap,
        active_provider_id: &str,
        pool: &'p HashMap<String, ProviderClient>,
    ) -> Option<&'p ProviderClient> {
        let tier = self.manifest.reasoning_tier_for_step(step_id);
        tier_map.resolve(tier, active_provider_id, pool)
    }

    pub fn step(&self, step_id: &str) -> Option<&WorkflowStep> {
        self.manifest
            .states
            .iter()
            .flat_map(|state| state.steps.iter())
            .find(|step| step.id == step_id)
    }

    /// Transition to a declared target. Exactly one durable step advances
    /// the state, chosen by the source/target approval semantics, so entry
    /// and approval binding are never split across a crash window.
    pub fn transition_to(
        &mut self,
        target: &str,
        action_request_id: Option<Ulid>,
    ) -> Result<(), WorkflowStateMachineError> {
        let from = self
            .current_state
            .clone()
            .ok_or(WorkflowStateMachineError::NoCurrentState)?;
        if !self
            .manifest
            .transitions
            .iter()
            .any(|transition| transition.from == from && transition.to == target)
        {
            return Err(WorkflowStateMachineError::UnknownTransition(
                target.to_string(),
            ));
        }
        let source_approval = self
            .manifest
            .states
            .iter()
            .find(|state| state.id == from)
            .map(|state| state.approval)
            .ok_or_else(|| WorkflowStateMachineError::InvalidDefinition(from.clone()))?;
        let target_state = self
            .manifest
            .states
            .iter()
            .find(|state| state.id == target)
            .ok_or_else(|| WorkflowStateMachineError::InvalidDefinition(target.to_string()))?;

        let departure = if source_approval == ApprovalSemantics::Required {
            // Always authorize the source before selecting the advancing step;
            // target semantics must never bypass the source approval gate.
            Some(self.authorize_departure(&from, action_request_id)?)
        } else {
            None
        };
        if target_state.approval == ApprovalSemantics::Required {
            // Schema validation rejects required -> required edges because a
            // single transition call has one request id and cannot safely
            // carry two distinct approval gates.
            let binding = self.prepare_entry_binding(
                &from,
                target,
                target_state.approval_action.as_ref(),
                action_request_id,
            )?;
            self.record_entry_binding(&from, binding)?;
        } else if let Some((_request_id, binding)) = departure {
            self.record_gated_departure(&from, target, &binding)?;
        } else {
            self.record_plain_transition(&from, target)?;
        }

        self.current_state = Some(target.to_string());
        Ok(())
    }
    fn authorize_departure(
        &self,
        state_id: &str,
        action_request_id: Option<Ulid>,
    ) -> Result<(Ulid, EntryBindingInputs), WorkflowStateMachineError> {
        let binding = self.ctx.entry_binding_for_state(state_id)?.ok_or(
            WorkflowStateMachineError::ApprovalRequired(state_id.to_string()),
        )?;
        let request_id = action_request_id.ok_or(WorkflowStateMachineError::ApprovalRequired(
            state_id.to_string(),
        ))?;
        let binding_id = Ulid::from_str(&binding.request_id).map_err(|_| {
            WorkflowStateMachineError::InvalidDefinition(format!(
                "entry binding for {state_id} has a malformed request id"
            ))
        })?;
        if request_id != binding_id {
            return Err(WorkflowStateMachineError::ApprovalBindingMismatch);
        }
        let request = self
            .ctx
            .store()
            .find_action_request(request_id)?
            .ok_or(WorkflowStateMachineError::ApprovalMissing)?;
        if request.action.as_str() != binding.action {
            return Err(WorkflowStateMachineError::ApprovalActionMismatch);
        }
        let approval: ApprovalRecord = self
            .ctx
            .store()
            .find_approval_for_request(request_id)?
            .ok_or(WorkflowStateMachineError::ApprovalMissing)?;
        if !approval.matches(
            &binding.payload_digest,
            &binding.target_digest,
            Timestamp::now(),
        ) {
            return Err(WorkflowStateMachineError::ApprovalDigestMismatch);
        }
        Ok((request_id, binding))
    }

    fn prepare_entry_binding(
        &self,
        from: &str,
        target: &str,
        approval_action: Option<&ActionId>,
        action_request_id: Option<Ulid>,
    ) -> Result<EntryBindingInputs, WorkflowStateMachineError> {
        let _ = from;
        let approval_action = approval_action
            .ok_or_else(|| WorkflowStateMachineError::InvalidDefinition(target.to_string()))?;
        let request_id = action_request_id.ok_or(
            WorkflowStateMachineError::ApprovalEntryBindingRequired(target.to_string()),
        )?;
        let request = self
            .ctx
            .store()
            .find_action_request(request_id)?
            .ok_or(WorkflowStateMachineError::ApprovalMissing)?;
        if request.action != *approval_action {
            return Err(WorkflowStateMachineError::ApprovalActionMismatch);
        }
        let payload_digest = request
            .payload_ref
            .as_ref()
            .map(|reference| reference.digest.clone())
            .ok_or(WorkflowStateMachineError::ApprovalDigestMissing)?;
        let target_digest = request
            .target_digest
            .clone()
            .ok_or(WorkflowStateMachineError::ApprovalDigestMissing)?;
        Ok(EntryBindingInputs {
            target_state: target.to_string(),
            request_id: request_id.to_string(),
            action: approval_action.as_str().to_string(),
            payload_digest,
            target_digest,
        })
    }

    fn record_entry_binding(
        &mut self,
        from: &str,
        binding: EntryBindingInputs,
    ) -> Result<(), WorkflowStateMachineError> {
        match self.ctx.begin_entry_binding_step(from, &binding)? {
            StepState::Replayed { outcome, .. } => {
                let recorded = outcome.map_err(|_| {
                    WorkflowStateMachineError::InvalidDefinition(binding.target_state.clone())
                })?;
                if recorded != binding {
                    return Err(WorkflowStateMachineError::ApprovalBindingMismatch);
                }
            }
            StepState::Fresh { handle, .. } | StepState::Resuming { handle, .. } => {
                self.ctx.complete_step(&handle, Ok::<_, String>(binding))?;
            }
        }
        Ok(())
    }

    fn record_gated_departure(
        &mut self,
        from: &str,
        target: &str,
        binding: &EntryBindingInputs,
    ) -> Result<(), WorkflowStateMachineError> {
        match self
            .ctx
            .begin_authorized_gated_departure_step(from, target, binding)?
        {
            StepState::Replayed { outcome, .. } => {
                let recorded = outcome.map_err(|_| {
                    WorkflowStateMachineError::InvalidDefinition(target.to_string())
                })?;
                if recorded.target != target {
                    return Err(WorkflowStateMachineError::ApprovalBindingMismatch);
                }
                self.current_state = Some(recorded.target);
            }
            StepState::Fresh { handle, .. } | StepState::Resuming { handle, .. } => {
                self.ctx.complete_step(
                    &handle,
                    Ok::<_, String>(TransitionOutcome {
                        target: target.to_string(),
                    }),
                )?;
            }
        }
        Ok(())
    }

    fn record_plain_transition(
        &mut self,
        from: &str,
        target: &str,
    ) -> Result<(), WorkflowStateMachineError> {
        match self.ctx.begin_transition_step(from, target)? {
            StepState::Replayed { outcome, .. } => {
                let recorded = outcome.map_err(|_| {
                    WorkflowStateMachineError::InvalidDefinition(target.to_string())
                })?;
                if recorded.target != target {
                    return Err(WorkflowStateMachineError::InvalidDefinition(format!(
                        "replayed transition target {} diverges from declared {target}",
                        recorded.target
                    )));
                }
                self.current_state = Some(recorded.target);
            }
            StepState::Fresh { handle, .. } | StepState::Resuming { handle, .. } => {
                self.ctx.complete_step(
                    &handle,
                    Ok::<_, String>(TransitionOutcome {
                        target: target.to_string(),
                    }),
                )?;
            }
        }
        Ok(())
    }
}

fn manifest_digest(manifest: &WorkflowManifest) -> Result<Digest, WorkflowStateMachineError> {
    // Bind the run to the complete immutable manifest, including purpose and
    // legacy fields, so any mid-run definition drift fails closed.
    let value = serde_json::to_value(manifest).map_err(|error| {
        WorkflowStateMachineError::InvalidDefinition(format!(
            "workflow manifest could not be serialized: {error}"
        ))
    })?;
    Ok(digest_of(&value))
}

#[cfg(test)]
#[path = "workflow_state_machine_tests.rs"]
mod tests;

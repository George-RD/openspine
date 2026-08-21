// openspine:allow-large-module reason: owner-review decision routing, evaluation-binding checks, transactional activation, and lifecycle refusal mapping share one owner-decision audit boundary.
//! Principal- and digest-bound owner-review decision routing.

use super::owner_review::{resume_standing_rule_revalidated, ResumeOutcome};
use super::owner_review_surface::{
    persist_owner_review, render_decision_receipt, OwnerReviewRenderer, OwnerReviewSurfaceError,
    RenderedOwnerReview, TelegramOwnerReviewRenderer, TerminalOwnerReviewRenderer,
};
use super::AppState;
use jiff::Timestamp;
use openspine_gate::{gate, ActionOrigin};
use openspine_schemas::action::{ActionId, ActionRequest, GateDecision};
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::digest::{canonical_json, digest_of_bytes, Digest};
use openspine_schemas::grant::{GrantLimits, GrantMode, TaskGrant};
use openspine_schemas::owner_review::{
    DecisionIntent, OwnerReviewEvaluationBinding, OwnerReviewEvaluationEpochs, OwnerReviewRequest,
    OwnerReviewState,
};
use openspine_schemas::owner_surface::{OwnerSurfaceKind, OwnerSurfaceRef};
use openspine_schemas::reviewed_scope::ReviewedActionScope;
use openspine_schemas::standing_rule::{ReviewedScopeBinding, StandingRuleManifest};
use rand::Rng;
use ulid::Ulid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OwnerReviewDecisionOutcome {
    Inspected(RenderedOwnerReview),
    Committed {
        receipt: String,
        replacement: Option<RenderedOwnerReview>,
        replayed: bool,
    },
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum OwnerReviewDecisionError {
    #[error("owner review does not exist")]
    UnknownReview,
    #[error("owner surface principal does not match the review owner")]
    PrincipalMismatch,
    #[error("owner review has expired")]
    Expired,
    #[error("rendered decision digest does not match the stored review")]
    DigestMismatch,
    #[error("miner evaluation binding refused: {0}")]
    EvaluationBindingRefused(String),
    #[error("decision intent is not available on this review")]
    IntentNotPermitted,
    #[error("Narrow requires a complete, strictly narrower reviewed scope")]
    MissingNarrowedScope,
    #[error("lifecycle intent requires a review-bound standing rule")]
    MissingLifecycleSubject,
    #[error("Edit requires submission of a replacement proposal")]
    EditRequiresReplacement,
    #[error("lifecycle transition refused: {0}")]
    LifecycleRefused(String),
    #[error("reviewed action has no ready executor")]
    ExecutorUnavailable,
    #[error(transparent)]
    Store(#[from] crate::store::StoreError),
    #[error(transparent)]
    Artifact(#[from] crate::artifact_store::ArtifactStoreError),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    #[error(transparent)]
    Surface(#[from] OwnerReviewSurfaceError),
    #[error(transparent)]
    Narrow(#[from] openspine_schemas::owner_review::OwnerReviewNarrowError),
}

pub(crate) fn load_owner_review_for_surface(
    state: &AppState,
    surface: &OwnerSurfaceRef,
    review_id: Ulid,
    digest_token: &str,
) -> Result<OwnerReviewRequest, OwnerReviewDecisionError> {
    let row = state
        .store
        .owner_review_row(review_id)?
        .ok_or(OwnerReviewDecisionError::UnknownReview)?;
    if row.owner_principal_id != surface.principal_id() {
        return Err(OwnerReviewDecisionError::PrincipalMismatch);
    }
    let bytes = state.artifacts.get(&row.artifact_ref)?;
    let review: OwnerReviewRequest = serde_json::from_slice(&bytes)?;
    if review.id != review_id || !review.binding_is_valid() {
        return Err(OwnerReviewDecisionError::DigestMismatch);
    }
    let full = review.binding_digest().as_str();
    let short = &full["sha256:".len()..][..12];
    if digest_token != full && digest_token.to_ascii_lowercase() != short {
        return Err(OwnerReviewDecisionError::DigestMismatch);
    }
    Ok(review)
}

#[allow(dead_code)]
pub(crate) fn submit_owner_review_callback(
    state: &AppState,
    surface: &OwnerSurfaceRef,
    review_id: Ulid,
    digest_token: &str,
    intent: DecisionIntent,
    now: Timestamp,
) -> Result<OwnerReviewDecisionOutcome, OwnerReviewDecisionError> {
    let review = match load_owner_review_for_surface(state, surface, review_id, digest_token) {
        Ok(review) => review,
        Err(error) => {
            state.store.record_owner_review_refusal(
                review_id,
                surface.principal_id(),
                intent,
                &error.to_string(),
            )?;
            return Err(error);
        }
    };
    submit_owner_review_decision(
        state,
        surface,
        review_id,
        review.binding_digest(),
        intent,
        None,
        now,
    )
}

#[allow(dead_code)]
pub(crate) fn submit_owner_review_decision(
    state: &AppState,
    surface: &OwnerSurfaceRef,
    review_id: Ulid,
    rendered_binding_digest: &Digest,
    intent: DecisionIntent,
    narrowed_scope: Option<ReviewedActionScope>,
    now: Timestamp,
) -> Result<OwnerReviewDecisionOutcome, OwnerReviewDecisionError> {
    let outcome = submit_owner_review_decision_inner(
        state,
        surface,
        review_id,
        rendered_binding_digest,
        intent,
        narrowed_scope,
        now,
    );
    if let Err(error) = &outcome {
        state.store.record_owner_review_refusal(
            review_id,
            surface.principal_id(),
            intent,
            &error.to_string(),
        )?;
    }
    outcome
}

pub(crate) async fn submit_owner_review_callback_async(
    state: &AppState,
    surface: &OwnerSurfaceRef,
    review_id: Ulid,
    digest_token: &str,
    intent: DecisionIntent,
    now: Timestamp,
) -> Result<OwnerReviewDecisionOutcome, OwnerReviewDecisionError> {
    let review = match load_owner_review_for_surface(state, surface, review_id, digest_token) {
        Ok(review) => review,
        Err(error) => {
            state.store.record_owner_review_refusal(
                review_id,
                surface.principal_id(),
                intent,
                &error.to_string(),
            )?;
            return Err(error);
        }
    };
    submit_owner_review_decision_async(
        state,
        surface,
        review_id,
        review.binding_digest(),
        intent,
        None,
        now,
    )
    .await
}

pub(crate) async fn submit_owner_review_decision_async(
    state: &AppState,
    surface: &OwnerSurfaceRef,
    review_id: Ulid,
    rendered_binding_digest: &Digest,
    intent: DecisionIntent,
    narrowed_scope: Option<ReviewedActionScope>,
    now: Timestamp,
) -> Result<OwnerReviewDecisionOutcome, OwnerReviewDecisionError> {
    let outcome = submit_owner_review_decision_inner_async(
        state,
        surface,
        review_id,
        rendered_binding_digest,
        intent,
        narrowed_scope,
        now,
    )
    .await;
    if let Err(error) = &outcome {
        state.store.record_owner_review_refusal(
            review_id,
            surface.principal_id(),
            intent,
            &error.to_string(),
        )?;
    }
    outcome
}

fn submit_owner_review_decision_inner(
    state: &AppState,
    surface: &OwnerSurfaceRef,
    review_id: Ulid,
    rendered_binding_digest: &Digest,
    intent: DecisionIntent,
    narrowed_scope: Option<ReviewedActionScope>,
    now: Timestamp,
) -> Result<OwnerReviewDecisionOutcome, OwnerReviewDecisionError> {
    let row = state
        .store
        .owner_review_row(review_id)?
        .ok_or(OwnerReviewDecisionError::UnknownReview)?;
    if row.owner_principal_id != surface.principal_id() {
        return Err(OwnerReviewDecisionError::PrincipalMismatch);
    }
    if now >= row.expires_at {
        if row.state == OwnerReviewState::Pending {
            state.store.transition_owner_review(
                review_id,
                OwnerReviewState::Pending,
                OwnerReviewState::Expired,
                "decision_after_expiry",
            )?;
        }
        return Err(OwnerReviewDecisionError::Expired);
    }
    let bytes = state.artifacts.get(&row.artifact_ref)?;
    let review: OwnerReviewRequest = serde_json::from_slice(&bytes)?;
    if review.id != row.id
        || !review.binding_is_valid()
        || review.binding_digest() != rendered_binding_digest
    {
        return Err(OwnerReviewDecisionError::DigestMismatch);
    }
    if !intent.is_permitted(&review.available_decisions, &review.lifecycle_controls) {
        return Err(OwnerReviewDecisionError::IntentNotPermitted);
    }
    if intent == DecisionIntent::Inspect {
        state.store.record_owner_review_inspection(
            review.id,
            surface.principal_id(),
            rendered_binding_digest,
        )?;
        let rendered = match surface.kind() {
            OwnerSurfaceKind::TelegramPrivate => {
                TelegramOwnerReviewRenderer::render(state, &review)?
            }
            OwnerSurfaceKind::LocalTerminal | OwnerSurfaceKind::WebOrMobile => {
                TerminalOwnerReviewRenderer::render(state, &review)?
            }
        };
        return Ok(OwnerReviewDecisionOutcome::Inspected(rendered));
    }
    let intent_name = format!("{intent:?}");
    if state
        .store
        .owner_review_last_decision(review_id)?
        .is_some_and(|(previous_intent, previous_digest)| {
            previous_intent == intent_name && previous_digest == *rendered_binding_digest
        })
    {
        let receipt = render_decision_receipt(
            intent,
            review_id,
            rendered_binding_digest,
            true,
            "the previously committed outcome remains in force",
        );
        return Ok(OwnerReviewDecisionOutcome::Committed {
            receipt,
            replacement: None,
            replayed: true,
        });
    }
    match intent {
        DecisionIntent::Approve => {
            if review.evaluation_binding.is_some() {
                return Err(OwnerReviewDecisionError::EvaluationBindingRefused(
                    "evaluated miner reviews require the asynchronous activation path".to_string(),
                ));
            }
            if !state.is_execution_backed(review.reviewed_scope.action_id()) {
                return Err(OwnerReviewDecisionError::ExecutorUnavailable);
            }
            let manifest = standing_rule_from_review(&review);
            state.store.approve_owner_review_and_activate_rule(
                review.id,
                surface.principal_id(),
                rendered_binding_digest,
                &manifest,
                now,
            )?;
            Ok(OwnerReviewDecisionOutcome::Committed {
                receipt: render_decision_receipt(
                    intent,
                    review.id,
                    rendered_binding_digest,
                    false,
                    &format!(
                        "activated standing rule {}; each matching effect still crosses fresh grant admission and gate",
                        manifest.id
                    ),
                ),
                replacement: None,
                replayed: false,
            })
        }
        DecisionIntent::Reject => commit_disposition(
            state,
            &review,
            surface.principal_id(),
            rendered_binding_digest,
            intent,
            OwnerReviewState::Rejected,
            "rejected exact stored review",
        ),
        DecisionIntent::Narrow => {
            let narrowed_scope =
                narrowed_scope.ok_or(OwnerReviewDecisionError::MissingNarrowedScope)?;
            let replacement = review.narrowed_review(Ulid::new(), narrowed_scope)?;
            let rendered = persist_owner_review(
                state,
                &replacement,
                row.owner_principal_id,
                row.expires_at,
                now,
                Some((review.id, rendered_binding_digest)),
            )?;
            let receipt = render_decision_receipt(
                intent,
                review.id,
                rendered_binding_digest,
                false,
                &format!(
                    "created narrowed review {} with binding digest {}",
                    replacement.id,
                    replacement.binding_digest().as_str()
                ),
            );
            Ok(OwnerReviewDecisionOutcome::Committed {
                receipt,
                replacement: Some(rendered),
                replayed: false,
            })
        }
        DecisionIntent::Pause
        | DecisionIntent::Resume
        | DecisionIntent::Expire
        | DecisionIntent::Revoke => commit_lifecycle(
            state,
            &review,
            row.state,
            surface.principal_id(),
            rendered_binding_digest,
            intent,
            now,
        ),
        DecisionIntent::Edit => Err(OwnerReviewDecisionError::EditRequiresReplacement),
        DecisionIntent::Inspect => unreachable!("Inspect returned before mutation routing"),
    }
}

async fn submit_owner_review_decision_inner_async(
    state: &AppState,
    surface: &OwnerSurfaceRef,
    review_id: Ulid,
    rendered_binding_digest: &Digest,
    intent: DecisionIntent,
    narrowed_scope: Option<ReviewedActionScope>,
    now: Timestamp,
) -> Result<OwnerReviewDecisionOutcome, OwnerReviewDecisionError> {
    let row = state
        .store
        .owner_review_row(review_id)?
        .ok_or(OwnerReviewDecisionError::UnknownReview)?;
    if row.owner_principal_id != surface.principal_id() {
        return Err(OwnerReviewDecisionError::PrincipalMismatch);
    }
    if now >= row.expires_at {
        if row.state == OwnerReviewState::Pending {
            state.store.transition_owner_review(
                review_id,
                OwnerReviewState::Pending,
                OwnerReviewState::Expired,
                "decision_after_expiry",
            )?;
        }
        return Err(OwnerReviewDecisionError::Expired);
    }
    let bytes = state.artifacts.get(&row.artifact_ref)?;
    let review: OwnerReviewRequest = serde_json::from_slice(&bytes)?;
    if review.id != row.id
        || !review.binding_is_valid()
        || review.binding_digest() != rendered_binding_digest
    {
        return Err(OwnerReviewDecisionError::DigestMismatch);
    }
    if !intent.is_permitted(&review.available_decisions, &review.lifecycle_controls) {
        return Err(OwnerReviewDecisionError::IntentNotPermitted);
    }
    if intent == DecisionIntent::Inspect {
        state.store.record_owner_review_inspection(
            review.id,
            surface.principal_id(),
            rendered_binding_digest,
        )?;
        let rendered = match surface.kind() {
            OwnerSurfaceKind::TelegramPrivate => {
                TelegramOwnerReviewRenderer::render(state, &review)?
            }
            OwnerSurfaceKind::LocalTerminal | OwnerSurfaceKind::WebOrMobile => {
                TerminalOwnerReviewRenderer::render(state, &review)?
            }
        };
        return Ok(OwnerReviewDecisionOutcome::Inspected(rendered));
    }
    let intent_name = format!("{intent:?}");
    if state
        .store
        .owner_review_last_decision(review_id)?
        .is_some_and(|(previous_intent, previous_digest)| {
            previous_intent == intent_name && previous_digest == *rendered_binding_digest
        })
    {
        let receipt = render_decision_receipt(
            intent,
            review_id,
            rendered_binding_digest,
            true,
            "the previously committed outcome remains in force",
        );
        return Ok(OwnerReviewDecisionOutcome::Committed {
            receipt,
            replacement: None,
            replayed: true,
        });
    }
    let evaluation = if matches!(intent, DecisionIntent::Approve | DecisionIntent::Narrow) {
        verify_miner_evaluation_binding(state, &review)?
    } else {
        None
    };
    match intent {
        DecisionIntent::Approve => {
            let Some(evaluation) = evaluation else {
                return submit_owner_review_decision_inner(
                    state,
                    surface,
                    review_id,
                    rendered_binding_digest,
                    intent,
                    narrowed_scope,
                    now,
                );
            };
            if !state.is_execution_backed(&evaluation.manifest.action_id) {
                return Err(OwnerReviewDecisionError::ExecutorUnavailable);
            }
            let manifest_id = evaluation.manifest.id.clone();
            let manifest_version = evaluation.manifest.version;
            let owner_grant =
                mint_owner_activation_grant(state, &evaluation.grant, review.id, surface, now)?;
            super::artifact_activation::activate_approved_artifact(
                state,
                &owner_grant,
                &evaluation.request,
                surface,
                Some(crate::store::activation::OwnerReviewApprovalCommit {
                    review_id: review.id,
                    owner_principal_id: surface.principal_id(),
                    binding_digest: rendered_binding_digest.clone(),
                    rule_id: manifest_id.clone(),
                    rule_version: manifest_version,
                    evaluation_binding: review.evaluation_binding.clone(),
                }),
            )
            .await
            .map_err(|error| {
                OwnerReviewDecisionError::EvaluationBindingRefused(format!(
                    "evaluated artifact activation failed: {error}"
                ))
            })?;
            Ok(OwnerReviewDecisionOutcome::Committed {
                receipt: render_decision_receipt(
                    intent,
                    review.id,
                    rendered_binding_digest,
                    false,
                    &format!(
                        "activated evaluated standing rule {manifest_id}; each matching effect still crosses fresh grant admission and gate"
                    ),
                ),
                replacement: None,
                replayed: false,
            })
        }
        DecisionIntent::Narrow if evaluation.is_some() => {
            let narrowed_scope =
                narrowed_scope.ok_or(OwnerReviewDecisionError::MissingNarrowedScope)?;
            dispatch_narrowed_miner_proposal(
                state,
                surface,
                &review,
                evaluation.expect("evaluation presence was matched"),
                rendered_binding_digest,
                narrowed_scope,
                row.expires_at,
                now,
            )
            .await
        }
        _ => submit_owner_review_decision_inner(
            state,
            surface,
            review_id,
            rendered_binding_digest,
            intent,
            narrowed_scope,
            now,
        ),
    }
}

fn standing_rule_from_review(review: &OwnerReviewRequest) -> StandingRuleManifest {
    StandingRuleManifest {
        id: format!("owner-review-{}", review.id),
        schema_version: 1,
        version: review.review_version,
        lifecycle_state: Lifecycle::Active,
        action_id: review.reviewed_scope.action_id().clone(),
        description: review.description.clone(),
        quota: review.limits.quota,
        rate: review.limits.rate,

        expires_after_secs: review.limits.expires_after_secs,
        dark_window: None,
        reviewed_scope: Some(ReviewedScopeBinding::derive_from(
            review.reviewed_scope.clone(),
            review.compatibility_digest.clone(),
        )),
    }
}
struct MinerEvaluationApproval {
    request: ActionRequest,
    grant: TaskGrant,
    manifest: StandingRuleManifest,
}

fn verify_miner_evaluation_binding(
    state: &AppState,
    review: &OwnerReviewRequest,
) -> Result<Option<MinerEvaluationApproval>, OwnerReviewDecisionError> {
    let Some(binding) = review.evaluation_binding.as_ref() else {
        return Ok(None);
    };
    if binding.artifact_kind != "standing_rule"
        || binding.replay_verdict_id == binding.judge_verdict_id
    {
        return Err(OwnerReviewDecisionError::EvaluationBindingRefused(
            "evaluation identity is malformed".to_string(),
        ));
    }
    let row = state
        .store
        .find_proposed_artifact(
            &binding.artifact_kind,
            &binding.artifact_id,
            binding.artifact_version,
        )?
        .ok_or_else(|| {
            OwnerReviewDecisionError::EvaluationBindingRefused(
                "proposed artifact is absent".to_string(),
            )
        })?;
    if row.state != Lifecycle::ReviewRequired {
        return Err(OwnerReviewDecisionError::EvaluationBindingRefused(
            "proposed artifact is not review_required".to_string(),
        ));
    }
    if review.proposal_digest != binding.proposal_digest
        || row.yaml_digest != binding.proposal_digest.as_str()
        || row.action_request_id != Some(binding.action_request_id)
    {
        return Err(OwnerReviewDecisionError::EvaluationBindingRefused(
            "proposed artifact identity does not match the review".to_string(),
        ));
    }
    let request = state
        .store
        .find_action_request(binding.action_request_id)?
        .ok_or_else(|| {
            OwnerReviewDecisionError::EvaluationBindingRefused(
                "activation request is absent".to_string(),
            )
        })?;
    if request.action != ActionId::new("artifact.activate")
        || request.task_grant_id != row.task_grant_id
    {
        return Err(OwnerReviewDecisionError::EvaluationBindingRefused(
            "activation request identity does not match the proposal".to_string(),
        ));
    }
    let payload_ref = request.payload_ref.as_ref().ok_or_else(|| {
        OwnerReviewDecisionError::EvaluationBindingRefused(
            "activation request payload is absent".to_string(),
        )
    })?;
    if payload_ref.digest != binding.proposal_digest {
        return Err(OwnerReviewDecisionError::EvaluationBindingRefused(
            "activation payload digest does not match the review".to_string(),
        ));
    }
    let yaml_bytes = state.artifacts.get(payload_ref)?;
    if digest_of_bytes(&yaml_bytes) != binding.proposal_digest {
        return Err(OwnerReviewDecisionError::EvaluationBindingRefused(
            "activation payload bytes changed".to_string(),
        ));
    }
    let parsed = crate::artifact_loader::parse_proposal(
        &binding.artifact_kind,
        std::str::from_utf8(&yaml_bytes).map_err(|error| {
            OwnerReviewDecisionError::EvaluationBindingRefused(format!(
                "activation payload is not UTF-8: {error}"
            ))
        })?,
    )
    .map_err(|error| {
        OwnerReviewDecisionError::EvaluationBindingRefused(format!(
            "activation payload does not parse: {error}"
        ))
    })?;
    let manifest = match parsed {
        crate::artifact_loader::ParsedProposal::StandingRule(manifest) => manifest,
        _ => {
            return Err(OwnerReviewDecisionError::EvaluationBindingRefused(
                "evaluation payload is not a standing-rule manifest".to_string(),
            ))
        }
    };
    let expected_scope = ReviewedScopeBinding::derive_from(
        review.reviewed_scope.clone(),
        review.compatibility_digest.clone(),
    );
    if manifest.id.as_str() != binding.artifact_id
        || manifest.version != binding.artifact_version
        || manifest.action_id != review.reviewed_scope.action_id().clone()
        || manifest.reviewed_scope.as_ref() != Some(&expected_scope)
    {
        return Err(OwnerReviewDecisionError::EvaluationBindingRefused(
            "manifest identity or scope does not match the reviewed scope".to_string(),
        ));
    }
    let verdicts = state.store.eval_verdicts_for_artifact(
        &binding.artifact_kind,
        &binding.artifact_id,
        binding.artifact_version,
    )?;
    let replay = verdicts
        .iter()
        .find(|verdict| verdict.id == binding.replay_verdict_id)
        .ok_or_else(|| {
            OwnerReviewDecisionError::EvaluationBindingRefused(
                "replay verdict is absent".to_string(),
            )
        })?;
    let judge = verdicts
        .iter()
        .find(|verdict| verdict.id == binding.judge_verdict_id)
        .ok_or_else(|| {
            OwnerReviewDecisionError::EvaluationBindingRefused(
                "judge verdict is absent".to_string(),
            )
        })?;
    if replay.verdict != "pass" || judge.verdict != "pass" {
        return Err(OwnerReviewDecisionError::EvaluationBindingRefused(
            "replay and judge verdicts must both be pass".to_string(),
        ));
    }
    if !verdict_matches_binding(replay, binding) || !verdict_matches_binding(judge, binding) {
        return Err(OwnerReviewDecisionError::EvaluationBindingRefused(
            "evaluation verdict identity does not match the review".to_string(),
        ));
    }
    let descriptor_version = state
        .action_catalog
        .delegation_descriptor_for(&manifest.action_id)
        .map(|descriptor| descriptor.descriptor_version);
    let implementation_version = state
        .action_catalog
        .implementation_descriptor_for_action(&manifest.action_id)
        .map(|descriptor| descriptor.implementation_version);
    let policy_version = crate::overlay_eval_gate::eval_input::policy_epoch(
        state
            .registry
            .read()
            .policies
            .iter()
            .map(|(id, policy)| (id, policy.version)),
    );
    let live = state.store.live_epochs_for_standing_rule(
        &manifest,
        descriptor_version,
        implementation_version,
        policy_version,
    );
    // Presence, exact identity, and lifecycle state are deliberately checked
    // above. `reject_stale_eval_verdicts` treats an empty verdict set as
    // current, so it must never be the first gate for a miner review.
    state.store.reject_stale_eval_verdicts(
        &binding.artifact_kind,
        &binding.artifact_id,
        binding.artifact_version,
        &live,
    )?;
    let (grant, _, _) = state
        .store
        .find_task_grant_by_id(request.task_grant_id)?
        .ok_or_else(|| {
            OwnerReviewDecisionError::EvaluationBindingRefused(
                "proposal task grant is absent".to_string(),
            )
        })?;
    Ok(Some(MinerEvaluationApproval {
        request,
        grant,
        manifest,
    }))
}
fn mint_owner_activation_grant(
    state: &AppState,
    source: &TaskGrant,
    review_id: Ulid,
    surface: &OwnerSurfaceRef,
    now: Timestamp,
) -> Result<TaskGrant, OwnerReviewDecisionError> {
    let (_, pending_message_ref, _) =
        state
            .store
            .find_task_grant_by_id(source.id)?
            .ok_or_else(|| {
                OwnerReviewDecisionError::EvaluationBindingRefused(
                    "proposal task grant is absent".to_string(),
                )
            })?;
    let mut task_token_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut task_token_bytes);
    let task_token = task_token_bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let id = Ulid::new();
    let activation = ActionId::new("artifact.activate");
    let mut grant = TaskGrant {
        id,
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user: state.owner.principal_id,
        purpose: "owner_approved_artifact_activation".to_string(),
        issued_by: "kernel".to_string(),
        issued_at: now,
        expires_at: now + std::time::Duration::from_secs(300),
        event_id: review_id,
        route_id: "owner_review".to_string(),
        agent_id: "kernel".to_string(),
        workflow_id: "owner_review".to_string(),
        capability_pack_id: "owner_review".to_string(),
        authority_sources: vec!["owner_review".to_string()],
        selection_tokens: vec![],
        allowed_actions: vec![activation],
        approval_required_actions: vec![],
        denied_actions: vec![],
        allowed_egress_classes: vec![],
        output_channels: vec![],
        limits: GrantLimits {
            max_model_calls: 0,
            max_artifacts: 1,
            max_runtime_seconds: 300,
        },
        task_token,
        root_grant_id: id,
        parent_grant_id: None,
        mode: GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
        persona_id: None,
    };
    let key = crate::grant_hmac_key().ok_or_else(|| {
        OwnerReviewDecisionError::EvaluationBindingRefused(
            "grant key unavailable for owner activation".to_string(),
        )
    })?;
    grant.seal_root(&key);
    state
        .store
        .insert_task_grant(&grant, &pending_message_ref, surface)?;
    Ok(grant)
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_narrowed_miner_proposal(
    state: &AppState,
    surface: &OwnerSurfaceRef,
    review: &OwnerReviewRequest,
    evaluation: MinerEvaluationApproval,
    rendered_binding_digest: &Digest,
    narrowed_scope: ReviewedActionScope,
    expires_at: Timestamp,
    now: Timestamp,
) -> Result<OwnerReviewDecisionOutcome, OwnerReviewDecisionError> {
    let replacement = review.narrowed_review(Ulid::new(), narrowed_scope)?;
    let mut manifest = evaluation.manifest;
    manifest.id = format!("{}-narrowed-{}", manifest.id, replacement.id);
    manifest.version = replacement.review_version;
    manifest.lifecycle_state = Lifecycle::Proposed;
    manifest.reviewed_scope = Some(ReviewedScopeBinding::derive_from(
        replacement.reviewed_scope.clone(),
        replacement.compatibility_digest.clone(),
    ));
    let yaml = serde_yaml::to_string(&manifest).map_err(|error| {
        OwnerReviewDecisionError::EvaluationBindingRefused(format!(
            "narrowed standing-rule serialization failed: {error}"
        ))
    })?;
    let payload = serde_json::json!({
        "kind": "standing_rule",
        "yaml": yaml,
    });
    let payload_bytes = canonical_json(&payload);
    let payload_ref = state.artifacts.put(payload_bytes.as_bytes())?;
    let proposal_action = ActionId::new("artifact.propose");
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: evaluation.grant.id,
        action: proposal_action.clone(),
        target_ref: None,
        payload_ref: Some(payload_ref.clone()),
        target_digest: None,
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        skill_attribution: None,
        requested_at: now,
        schema_version: 1,
    };
    let gate_outcome = gate(
        &evaluation.grant,
        &request,
        ActionOrigin::Shell,
        &state.store,
        &state.action_catalog,
        &state.connectors,
        now,
    );
    state.store.append_audit(
        "action.gated",
        Some(&proposal_action),
        Some(&gate_outcome.decision),
        None,
        Some(evaluation.grant.id),
        &[],
        std::slice::from_ref(&payload_ref),
    )?;
    if !matches!(gate_outcome.decision, GateDecision::Allow) {
        return Err(OwnerReviewDecisionError::EvaluationBindingRefused(
            "narrowed artifact proposal was not allowed by the action gate".to_string(),
        ));
    }
    if !crate::spend::admit_spend(
        state,
        crate::spend::SpendLane::from_grant(&evaluation.grant),
        now,
    )
    .await?
    {
        return Err(OwnerReviewDecisionError::EvaluationBindingRefused(
            "narrowed artifact proposal exceeded the spend cap".to_string(),
        ));
    }
    let receipt = crate::api::artifact_propose::dispatch_artifact_propose_core(
        state,
        &evaluation.grant,
        &proposal_action,
        surface,
        Some(&payload),
        false,
    )
    .await
    .map_err(|error| {
        OwnerReviewDecisionError::EvaluationBindingRefused(format!(
            "narrowed artifact proposal dispatch failed: {error:?}"
        ))
    })?;
    let receipt_epochs = receipt.epochs.clone();
    let evaluation_binding = OwnerReviewEvaluationBinding {
        artifact_kind: receipt.artifact_kind,
        artifact_id: receipt.artifact_id,
        artifact_version: receipt.artifact_version,
        proposal_digest: receipt.artifact_digest,
        action_request_id: receipt.action_request_id,
        replay_verdict_id: receipt.replay_verdict_id,
        judge_verdict_id: receipt.judge_verdict_id,
        epochs: OwnerReviewEvaluationEpochs {
            proposal_digest: receipt_epochs
                .proposal_digest
                .and_then(|value| Digest::parse(value).ok()),
            compatibility_digest: receipt_epochs
                .compatibility_digest
                .and_then(|value| Digest::parse(value).ok()),
            reviewed_scope_digest: receipt_epochs
                .reviewed_scope_digest
                .and_then(|value| Digest::parse(value).ok()),
            evidence_set_digest: receipt_epochs
                .evidence_set_digest
                .and_then(|value| Digest::parse(value).ok()),
            descriptor_version: receipt_epochs.descriptor_version,
            implementation_version: receipt_epochs.implementation_version,
            policy_version: receipt_epochs.policy_version,
        },
    };
    let mut replacement = replacement;
    replacement.proposal_digest = evaluation_binding.proposal_digest.clone();
    let replacement = replacement.with_evaluation_binding(evaluation_binding);
    let rendered = persist_owner_review(
        state,
        &replacement,
        surface.principal_id(),
        expires_at,
        now,
        Some((review.id, rendered_binding_digest)),
    )?;
    Ok(OwnerReviewDecisionOutcome::Committed {
        receipt: render_decision_receipt(
            DecisionIntent::Narrow,
            review.id,
            rendered_binding_digest,
            false,
            &format!(
                "created narrowed evaluated standing-rule proposal {}; review it before activation",
                replacement.id
            ),
        ),
        replacement: Some(rendered),
        replayed: false,
    })
}

fn verdict_matches_binding(
    verdict: &crate::store::eval_verdict_store::EvalVerdict,
    binding: &OwnerReviewEvaluationBinding,
) -> bool {
    verdict.artifact_kind == binding.artifact_kind
        && verdict.artifact_id == binding.artifact_id
        && verdict.artifact_version == binding.artifact_version
        && verdict.artifact_digest == binding.proposal_digest.as_str()
        && verdict_epochs_match(&verdict.epochs, &binding.epochs)
}

fn verdict_epochs_match(
    recorded: &crate::store::eval_verdict_store::VerdictEpochs,
    bound: &OwnerReviewEvaluationEpochs,
) -> bool {
    fn digest_matches(recorded: &Option<String>, bound: &Option<Digest>) -> bool {
        match (recorded, bound) {
            (None, None) => true,
            (Some(recorded), Some(bound)) => recorded == bound.as_str(),
            _ => false,
        }
    }
    digest_matches(&recorded.proposal_digest, &bound.proposal_digest)
        && digest_matches(&recorded.compatibility_digest, &bound.compatibility_digest)
        && digest_matches(
            &recorded.reviewed_scope_digest,
            &bound.reviewed_scope_digest,
        )
        && digest_matches(&recorded.evidence_set_digest, &bound.evidence_set_digest)
        && recorded.descriptor_version == bound.descriptor_version
        && recorded.implementation_version == bound.implementation_version
        && recorded.policy_version == bound.policy_version
}

fn commit_disposition(
    state: &AppState,
    review: &OwnerReviewRequest,
    owner_principal_id: Ulid,
    digest: &Digest,
    intent: DecisionIntent,
    to: OwnerReviewState,
    detail: &str,
) -> Result<OwnerReviewDecisionOutcome, OwnerReviewDecisionError> {
    state.store.commit_owner_review_decision(
        review.id,
        intent,
        owner_principal_id,
        digest,
        Some((OwnerReviewState::Pending, to)),
    )?;
    Ok(OwnerReviewDecisionOutcome::Committed {
        receipt: render_decision_receipt(intent, review.id, digest, false, detail),
        replacement: None,
        replayed: false,
    })
}

fn commit_lifecycle(
    state: &AppState,
    review: &OwnerReviewRequest,
    review_state: OwnerReviewState,
    owner_principal_id: Ulid,
    digest: &Digest,
    intent: DecisionIntent,
    now: Timestamp,
) -> Result<OwnerReviewDecisionOutcome, OwnerReviewDecisionError> {
    let (rule_id, version) = state
        .store
        .owner_review_standing_rule(review.id)?
        .ok_or(OwnerReviewDecisionError::MissingLifecycleSubject)?;
    let lifecycle_replayed = match intent {
        DecisionIntent::Pause => match state.store.pause_standing_rule(&rule_id, now)? {
            crate::store::PauseStandingRuleOutcome::Paused => false,
            crate::store::PauseStandingRuleOutcome::AlreadyPaused => true,
            crate::store::PauseStandingRuleOutcome::Refused => {
                return Err(OwnerReviewDecisionError::LifecycleRefused(
                    "standing rule pause refused".into(),
                ));
            }
        },
        DecisionIntent::Resume => {
            match resume_standing_rule_revalidated(state, &rule_id, version, now)
                .map_err(|error| OwnerReviewDecisionError::LifecycleRefused(error.to_string()))?
            {
                ResumeOutcome::Resumed => false,
                ResumeOutcome::AlreadyActive => true,
                ResumeOutcome::Refused(reason) => {
                    return Err(OwnerReviewDecisionError::LifecycleRefused(
                        reason.audit_kind().into(),
                    ));
                }
            }
        }
        DecisionIntent::Expire | DecisionIntent::Revoke => {
            !state.store.revoke_standing_rule(&rule_id, now)?
        }
        _ => unreachable!("caller restricted lifecycle intents"),
    };
    let review_transition = match intent {
        DecisionIntent::Expire => Some((review_state, OwnerReviewState::Expired)),
        DecisionIntent::Revoke => Some((review_state, OwnerReviewState::Revoked)),
        DecisionIntent::Pause | DecisionIntent::Resume => None,
        _ => unreachable!("caller restricted lifecycle intents"),
    };
    let review_changed = state.store.commit_owner_review_decision(
        review.id,
        intent,
        owner_principal_id,
        digest,
        review_transition,
    )?;
    let changed = match intent {
        DecisionIntent::Pause | DecisionIntent::Resume => !lifecycle_replayed,
        DecisionIntent::Expire | DecisionIntent::Revoke => !lifecycle_replayed || review_changed,
        _ => unreachable!("caller restricted lifecycle intents"),
    };
    let detail = if !lifecycle_replayed {
        format!("standing rule {rule_id} transitioned")
    } else if review_changed {
        format!("review lifecycle transitioned; standing rule {rule_id} was already revoked")
    } else {
        format!("standing rule {rule_id} already had the requested state")
    };
    Ok(OwnerReviewDecisionOutcome::Committed {
        receipt: render_decision_receipt(intent, review.id, digest, !changed, &detail),
        replacement: None,
        replayed: !changed,
    })
}

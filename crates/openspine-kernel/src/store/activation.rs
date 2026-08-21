//! Durable, atomic commit for a normal artifact activation (AD-070): the
//! learned-artifact provenance row, the proposal's `Approved -> Active`
//! transition, and the activation audit all land in one SQLite transaction so
//! a crash between staging the overlay file and publishing it leaves a
//! consistent, recoverable state (startup republishes from the committed
//! `pending_yaml_digest`).

use openspine_schemas::digest::Digest;
use openspine_schemas::owner_review::OwnerReviewEvaluationBinding;
use rusqlite::{params, OptionalExtension};
use ulid::Ulid;

use super::learned_artifacts::LearnedArtifact;
use super::{Store, StoreError};

/// The owner-review transition committed together with an evaluated artifact
/// activation. Keeping this in `ActivationCommit` prevents an active rule
/// from being left behind if the review transition fails.
pub struct OwnerReviewApprovalCommit {
    pub review_id: Ulid,
    pub owner_principal_id: Ulid,
    pub binding_digest: Digest,
    pub rule_id: String,
    pub rule_version: u32,
    pub evaluation_binding: Option<OwnerReviewEvaluationBinding>,
}

/// Inputs for an atomic artifact-activation commit.
pub struct ActivationCommit {
    pub learned: LearnedArtifact,
    pub proposed_id: Ulid,
    pub grant_id: Option<Ulid>,
    pub payload_ref: Option<openspine_schemas::artifact::ArtifactRef>,
    pub dangling: bool,
    pub superseded_old_version: Option<u32>,
    pub standing_rule: Option<(
        openspine_schemas::standing_rule::StandingRuleManifest,
        Option<Ulid>,
    )>,
    pub owner_review_approval: Option<OwnerReviewApprovalCommit>,
    /// Live catalog/policy epochs at activation time, for the #133 verdict
    /// currency re-check. `None` on any axis means the caller could not
    /// resolve it, and that axis is simply not compared.
    pub live_descriptor_version: Option<u32>,
    pub live_implementation_version: Option<u32>,
    pub live_policy_version: Option<u32>,
}
type EvaluatedProposalRow = (String, String, i64, String, String, Option<String>, String);

fn verify_evaluated_activation_in_tx(
    tx: &rusqlite::Transaction<'_>,
    proposed_id: Ulid,
    binding: &OwnerReviewEvaluationBinding,
) -> Result<(), StoreError> {
    let row: Option<EvaluatedProposalRow> = tx
        .query_row(
            "SELECT kind, artifact_id, version, state, yaml_digest,
                    action_request_id, task_grant_id
             FROM proposed_artifacts WHERE id = ?1",
            params![proposed_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()?;
    let Some((kind, artifact_id, version, state, yaml_digest, request_id, grant_id)) = row else {
        return Err(StoreError::ProposedArtifactLifecycle(
            "evaluated proposal is absent".into(),
        ));
    };
    if state != "review_required" {
        return Err(StoreError::ProposedArtifactLifecycle(
            "evaluated proposal is not review_required".into(),
        ));
    }
    let expected_request_id = binding.action_request_id.to_string();
    if kind != binding.artifact_kind
        || artifact_id != binding.artifact_id
        || version != i64::from(binding.artifact_version)
        || yaml_digest != binding.proposal_digest.as_str()
        || request_id.as_deref() != Some(expected_request_id.as_str())
    {
        return Err(StoreError::ProposedArtifactLifecycle(
            "evaluated proposal identity does not match owner review".into(),
        ));
    }
    let request_json: String = tx.query_row(
        "SELECT request_json FROM action_requests WHERE id = ?1",
        params![binding.action_request_id.to_string()],
        |row| row.get(0),
    )?;
    let request: openspine_schemas::action::ActionRequest = serde_json::from_str(&request_json)?;
    if request.action != openspine_schemas::action::ActionId::new("artifact.activate")
        || request.task_grant_id.to_string() != grant_id
        || request.payload_ref.as_ref().map(|value| &value.digest) != Some(&binding.proposal_digest)
    {
        return Err(StoreError::ProposedArtifactLifecycle(
            "evaluated activation request identity does not match owner review".into(),
        ));
    }
    let verdicts = Store::eval_verdicts_for_artifact_conn(
        tx,
        &binding.artifact_kind,
        &binding.artifact_id,
        binding.artifact_version,
    )?;
    let replay = verdicts
        .iter()
        .find(|verdict| verdict.id == binding.replay_verdict_id)
        .ok_or_else(|| StoreError::ProposedArtifactLifecycle("replay verdict is absent".into()))?;
    let judge = verdicts
        .iter()
        .find(|verdict| verdict.id == binding.judge_verdict_id)
        .ok_or_else(|| StoreError::ProposedArtifactLifecycle("judge verdict is absent".into()))?;
    for (name, verdict) in [("replay", replay), ("judge", judge)] {
        if verdict.verdict != "pass" {
            return Err(StoreError::ProposedArtifactLifecycle(format!(
                "{name} evaluation verdict is not pass"
            )));
        }
        if verdict.artifact_digest != binding.proposal_digest.as_str() {
            return Err(StoreError::ProposedArtifactLifecycle(format!(
                "{name} evaluation digest does not match owner review"
            )));
        }
        if !epoch_digests_match(&verdict.epochs, &binding.epochs)
            || !epoch_versions_match(&verdict.epochs, &binding.epochs)
        {
            return Err(StoreError::ProposedArtifactLifecycle(format!(
                "{name} evaluation identity is stale or mismatched"
            )));
        }
    }
    Ok(())
}

fn epoch_digests_match(
    recorded: &super::eval_verdict_store::VerdictEpochs,
    bound: &openspine_schemas::owner_review::OwnerReviewEvaluationEpochs,
) -> bool {
    fn matches(recorded: &Option<String>, bound: &Option<Digest>) -> bool {
        match (recorded, bound) {
            (None, None) => true,
            (Some(recorded), Some(bound)) => recorded == bound.as_str(),
            _ => false,
        }
    }
    matches(&recorded.proposal_digest, &bound.proposal_digest)
        && matches(&recorded.compatibility_digest, &bound.compatibility_digest)
        && matches(
            &recorded.reviewed_scope_digest,
            &bound.reviewed_scope_digest,
        )
        && matches(&recorded.evidence_set_digest, &bound.evidence_set_digest)
}

fn epoch_versions_match(
    recorded: &super::eval_verdict_store::VerdictEpochs,
    bound: &openspine_schemas::owner_review::OwnerReviewEvaluationEpochs,
) -> bool {
    recorded.descriptor_version == bound.descriptor_version
        && recorded.implementation_version == bound.implementation_version
        && recorded.policy_version == bound.policy_version
}

impl Store {
    /// Commit an artifact activation atomically with its optional owner-review
    /// approval transition.
    pub fn commit_artifact_activation(&self, input: ActivationCommit) -> Result<bool, StoreError> {
        let ActivationCommit {
            learned,
            proposed_id,
            grant_id,
            payload_ref,
            dangling,
            superseded_old_version,
            standing_rule,
            owner_review_approval,
            live_descriptor_version,
            live_implementation_version,
            live_policy_version,
        } = input;
        let provenance_json = serde_json::to_string(&learned.provenance)
            .map_err(|err| StoreError::LearnedArtifact(format!("provenance json: {err}")))?;
        // Incomplete scope bindings are a structural refusal and can be
        // rejected before opening the activation transaction.
        if let Some((manifest, _)) = standing_rule.as_ref() {
            self.reject_incomplete_scope_binding(manifest)?;
        }
        self.with_immediate_tx(|tx| {
            let mut owner_eval_checked = false;
            if let Some(approval) = owner_review_approval.as_ref() {
                if let Some(binding) = approval.evaluation_binding.as_ref() {
                    if standing_rule.is_none() {
                        return Err(StoreError::ProposedArtifactLifecycle(
                            "evaluated approval has no standing-rule manifest".into(),
                        ));
                    }
                    verify_evaluated_activation_in_tx(tx, proposed_id, binding)?;
                    owner_eval_checked = true;
                }
            }
            if let Some((manifest, _)) = standing_rule.as_ref() {
                // Verdict currency is a read-time property of the committed
                // activation state. Re-check it while BEGIN IMMEDIATE owns the
                // write transaction, so a world change after any caller-side
                // preparation cannot slip through.
                let live = self.live_epochs_for_standing_rule(
                    manifest,
                    live_descriptor_version,
                    live_implementation_version,
                    live_policy_version,
                );
                if let Err(error) = super::eval_verdict_currency::reject_stale_eval_verdicts_conn(
                    tx,
                    "standing_rule",
                    manifest.id.as_str(),
                    manifest.version,
                    &live,
                ) {
                    // Commit only the durable refusal audit; no activation writes
                    // have occurred yet. Return Ok(Err(error)) so with_immediate_tx
                    // commits the refusal audit, then the outer method unwraps to Err.
                    return Ok(Err(error));
                }
            }
            let reviewed_to_approved = tx.execute(
                "UPDATE proposed_artifacts SET state = 'approved'
                 WHERE id = ?1 AND state = 'review_required'",
                params![proposed_id.to_string()],
            )?;
            if reviewed_to_approved != 1 && owner_eval_checked {
                return Err(StoreError::ProposedArtifactLifecycle(
                    "evaluated proposal failed review_required -> approved".into(),
                ));
            }
            if let Some((manifest, rule_grant_id)) = standing_rule.as_ref() {
                Self::activate_standing_rule_in_tx(
                    tx,
                    manifest,
                    *rule_grant_id,
                    jiff::Timestamp::now(),
                )?;
            }
            // Erased is terminal for the identity (kind, artifact_id, version),
            // not just for the producing scope. INSERT OR REPLACE would otherwise
            // delete the erased row and reinsert under a different source_scope.
            let existing_status: Option<String> = tx
                .query_row(
                    "SELECT compatibility FROM learned_artifacts
                      WHERE kind = ?1 AND artifact_id = ?2 AND version = ?3",
                    params![learned.kind, learned.artifact_id, learned.version as i64],
                    |row| row.get(0),
                )
                .optional()
                .map_err(StoreError::from)?;
            if existing_status.as_deref() == Some("erased") {
                return Err(StoreError::LearnedArtifact(
                    "cannot replace an erased learned artifact identity".into(),
                ));
            }
            let learned_rows = tx.execute(
                "INSERT OR REPLACE INTO learned_artifacts \
                 (kind, artifact_id, version, namespace, provenance, accepted_via, learned_at, \
                  compatibility, nomination, pending_reconfirmation_id, pending_yaml_digest, \
                  accepted_dependency_fingerprint, source_path, accepted_base_epoch) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    learned.kind,
                    learned.artifact_id,
                    learned.version as i64,
                    match learned.namespace {
                        openspine_schemas::artifact::ArtifactNamespace::Base => "base",
                        openspine_schemas::artifact::ArtifactNamespace::Overlay => "overlay",
                    },
                    provenance_json,
                    learned
                        .accepted_via
                        .as_ref()
                        .map(serde_json::to_string)
                        .transpose()
                        .map_err(|err| StoreError::LearnedArtifact(format!(
                            "accepted_via json: {err}"
                        )))?,
                    learned.learned_at.to_string(),
                    match learned.compatibility {
                        super::learned_artifacts::CompatibilityStatus::Compatible => "compatible",
                        super::learned_artifacts::CompatibilityStatus::ReconfirmationRequired =>
                            "reconfirmation_required",
                        super::learned_artifacts::CompatibilityStatus::OwnerAccepted =>
                            "owner_accepted",
                        // Erased artifacts can never be activated (AD-140): their
                        // source exchange is undecryptable.
                        super::learned_artifacts::CompatibilityStatus::Erased => "erased",
                    },
                    match learned.nomination {
                        super::learned_artifacts::NominationStatus::None => "none",
                        super::learned_artifacts::NominationStatus::Nominated => "nominated",
                    },
                    learned.pending_reconfirmation_id.map(|u| u.to_string()),
                    learned.pending_yaml_digest,
                    learned.accepted_dependency_fingerprint,
                    learned.source_path,
                    learned.accepted_base_epoch,
                ],
            )?;
            if learned_rows == 0 {
                return Err(StoreError::LearnedArtifact(
                    "learned artifact row failed to insert".into(),
                ));
            }
            if !dangling {
                let active = tx.execute(
                    "UPDATE proposed_artifacts SET state = 'active' \
                     WHERE id = ?1 AND state = 'approved'",
                    params![proposed_id.to_string()],
                )?;
                if active != 1 {
                    return Err(StoreError::ProposedArtifactLifecycle(format!(
                        "proposal {proposed_id} failed to advance approved -> active"
                    )));
                }
                Store::append_audit_conn(
                    tx,
                    "artifact.activated",
                    None,
                    None,
                    None,
                    grant_id,
                    &[],
                    payload_ref.as_slice(),
                )?;
                if let Some(old) = superseded_old_version {
                    let reason = format!(
                        "{}:{} v{} superseded by v{}",
                        learned.kind, learned.artifact_id, old, learned.version
                    );
                    Store::append_audit_conn(
                        tx,
                        "artifact.superseded",
                        None,
                        None,
                        Some(&reason),
                        grant_id,
                        &[],
                        &[],
                    )?;
                }
            }
            if let Some(approval) = owner_review_approval.as_ref() {
                let changed = tx.execute(
                    "UPDATE owner_reviews SET state = 'approved', last_decision = ?2,
                     decision_binding_digest = ?3
                     WHERE id = ?1 AND owner_principal_id = ?4 AND state = 'pending'",
                    params![
                        approval.review_id.to_string(),
                        format!(
                            "{:?}",
                            openspine_schemas::owner_review::DecisionIntent::Approve
                        ),
                        approval.binding_digest.as_str(),
                        approval.owner_principal_id.to_string(),
                    ],
                )?;
                if changed != 1 {
                    return Err(StoreError::FailureRouting(
                        "owner review is not pending for this principal".into(),
                    ));
                }
                tx.execute(
                    "INSERT INTO owner_review_standing_rules (review_id, rule_id, rule_version)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(review_id) DO UPDATE SET
                        rule_id = excluded.rule_id, rule_version = excluded.rule_version",
                    params![
                        approval.review_id.to_string(),
                        approval.rule_id,
                        approval.rule_version as i64,
                    ],
                )?;
                Store::append_audit_conn(
                    tx,
                    "owner_review.decision",
                    None,
                    None,
                    Some(&format!(
                        "{}:Approve:{}:{}:{}",
                        approval.review_id,
                        approval.binding_digest.as_str(),
                        approval.rule_id,
                        approval.owner_principal_id
                    )),
                    None,
                    &[],
                    &[],
                )?;
                Store::append_audit_conn(
                    tx,
                    "owner_review.transitioned",
                    None,
                    None,
                    Some(&format!("{}:pending->approved", approval.review_id)),
                    None,
                    &[],
                    &[],
                )?;
            }
            #[cfg(test)]
            if self
                .activation_tx_failure
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                return Err(StoreError::LearnedArtifact(
                    "injected activation transaction failure".into(),
                ));
            }
            if self
                .fail_next_owner_reconfirmation
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                self.fail_next_owner_reconfirmation
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                return Err(StoreError::LearnedArtifact(
                    "injected activation transaction failure".into(),
                ));
            }
            Ok(Ok(true))
        })?
    }
}

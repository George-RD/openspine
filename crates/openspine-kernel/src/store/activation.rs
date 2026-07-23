//! Durable, atomic commit for a normal artifact activation (AD-070): the
//! learned-artifact provenance row, the proposal's `Approved -> Active`
//! transition, and the activation audit all land in one SQLite transaction so
//! a crash between staging the overlay file and publishing it leaves a
//! consistent, recoverable state (startup republishes from the committed
//! `pending_yaml_digest`).

use rusqlite::{params, OptionalExtension};
use ulid::Ulid;

use super::learned_artifacts::LearnedArtifact;
use super::{Store, StoreError};

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
}
impl Store {
    /// Atomically persist an activation's durable disposition (learned row +
    /// proposal lifecycle + activation audit), then return so the caller may
    /// publish the staged overlay file only on success.
    pub fn commit_artifact_activation(&self, input: ActivationCommit) -> Result<bool, StoreError> {
        let ActivationCommit {
            learned,
            proposed_id,
            grant_id,
            payload_ref,
            dangling,
            superseded_old_version,
            standing_rule,
        } = input;
        let provenance_json = serde_json::to_string(&learned.provenance)
            .map_err(|err| StoreError::LearnedArtifact(format!("provenance json: {err}")))?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if let Some((manifest, rule_grant_id)) = standing_rule.as_ref() {
            Self::activate_standing_rule_in_tx(
                &tx,
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
            tx.rollback()?;
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
            tx.rollback()?;
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
                tx.rollback()?;
                return Err(StoreError::ProposedArtifactLifecycle(format!(
                    "proposal {proposed_id} failed to advance approved -> active"
                )));
            }
            Store::append_audit_conn(
                &tx,
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
                    &tx,
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
        #[cfg(test)]
        if self
            .activation_tx_failure
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            tx.rollback()?;
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
        tx.commit()?;
        Ok(true)
    }
}

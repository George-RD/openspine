use rusqlite::{params, OptionalExtension};
use ulid::Ulid;

use super::learned_artifacts::{Provenance, ReconfirmAnchor};
use super::proposed_artifacts::ProposedArtifact;
use super::{Store, StoreError};

/// Inputs for an atomic owner-reconfirmation commit (AD-070). Bundled so the
/// transaction fields are explicit and reviewable.
pub struct OwnerReconfirmation {
    pub kind: String,
    pub artifact_id: String,
    pub version: u32,
    pub provenance: Provenance,
    pub accepted_via: Option<ReconfirmAnchor>,
    pub base_epoch: String,
    pub accepted_dependency_fingerprint: Option<String>,
    pub request_id: Ulid,
    pub grant_id: Option<Ulid>,
    pub review_ref: Option<openspine_schemas::artifact::ArtifactRef>,
    pub proposal_id: Option<Ulid>,
    pub new_proposal: Option<ProposedArtifact>,
    pub dangling_refs: Vec<String>,
    pub superseded_old_version: Option<u32>,
}

impl Store {
    /// Atomically persist an owner reconfirmation and its proposal lifecycle.
    pub fn commit_owner_reconfirmation(
        &self,
        input: OwnerReconfirmation,
    ) -> Result<bool, StoreError> {
        let OwnerReconfirmation {
            kind,
            artifact_id,
            version,
            provenance,
            accepted_via,
            base_epoch,
            accepted_dependency_fingerprint,
            request_id,
            grant_id,
            review_ref,
            proposal_id,
            new_proposal,
            dangling_refs,
            superseded_old_version,
        } = input;
        let provenance_json = serde_json::to_string(&provenance)
            .map_err(|err| StoreError::LearnedArtifact(format!("provenance json: {err}")))?;
        let accepted_via_json = accepted_via
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|err| StoreError::LearnedArtifact(format!("accepted_via json: {err}")))?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let consumed = tx.execute(
            "UPDATE action_requests SET used = 1 WHERE id = ?1 AND used = 0",
            params![request_id.to_string()],
        )?;
        if consumed == 0 {
            tx.rollback()?;
            return Ok(false);
        }
        // Erased is terminal for the identity itself. A replacement that only
        // changes source_scope must still fail closed rather than revive the row.
        let existing_status: Option<String> = tx
            .query_row(
                "SELECT compatibility FROM learned_artifacts
                  WHERE kind = ?1 AND artifact_id = ?2 AND version = ?3",
                params![kind, artifact_id, version as i64],
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
            "UPDATE learned_artifacts SET compatibility = 'owner_accepted', provenance = ?1,
             accepted_via = ?2, accepted_base_epoch = ?3,
             accepted_dependency_fingerprint = ?7, pending_reconfirmation_id = NULL
             WHERE kind = ?4 AND artifact_id = ?5 AND version = ?6
               AND compatibility != 'erased'",
            params![
                provenance_json,
                accepted_via_json,
                base_epoch,
                kind,
                artifact_id,
                version as i64,
                accepted_dependency_fingerprint,
            ],
        )?;
        if learned_rows != 1 {
            tx.rollback()?;
            return Err(StoreError::LearnedArtifact(
                "learned artifact provenance row not found".into(),
            ));
        }
        let proposal_rows = if let Some(proposal) = new_proposal {
            let lineage_json = serde_json::to_string(&proposal.lineage)
                .map_err(|err| StoreError::LearnedArtifact(format!("lineage json: {err}")))?;
            tx.execute(
                "INSERT INTO proposed_artifacts \
                 (id, kind, artifact_id, version, state, yaml_digest, task_grant_id, \
                  action_request_id, proposed_at, lineage_json) \
                 VALUES (?1, ?2, ?3, ?4, 'proposed', ?5, ?6, ?7, ?8, ?9)",
                params![
                    proposal.id.to_string(),
                    proposal.kind,
                    proposal.artifact_id,
                    proposal.version as i64,
                    proposal.yaml_digest,
                    proposal.task_grant_id.to_string(),
                    proposal.action_request_id.map(|u| u.to_string()),
                    proposal.proposed_at.to_string(),
                    lineage_json,
                ],
            )?;
            Store::append_audit_conn(
                &tx,
                "artifact.proposed",
                None,
                None,
                None,
                grant_id,
                &[],
                &[],
            )?;
            for (from, to) in [
                ("proposed", "validated"),
                ("validated", "review_required"),
                ("review_required", "approved"),
                ("approved", "active"),
            ] {
                let rows = tx.execute(
                    "UPDATE proposed_artifacts SET state = ?1 WHERE id = ?2 AND state = ?3",
                    params![to, proposal.id.to_string(), from],
                )?;
                if rows != 1 {
                    return Err(StoreError::LearnedArtifact(format!(
                        "legacy proposal {} lifecycle transition {} -> {} affected {} rows",
                        proposal.id, from, to, rows
                    )));
                }
            }
            1
        } else if let Some(proposal_id) = proposal_id {
            tx.execute(
                "UPDATE proposed_artifacts SET state = 'active' WHERE id = ?1 AND state = 'approved'",
                params![proposal_id.to_string()],
            )?
        } else {
            0
        };
        Store::append_audit_conn(
            &tx,
            "artifact.reconfirmed",
            None,
            None,
            None,
            grant_id,
            &[],
            review_ref.as_slice(),
        )?;
        if proposal_rows == 1 {
            Store::append_audit_conn(
                &tx,
                "artifact.activated",
                None,
                None,
                None,
                grant_id,
                &[],
                review_ref.as_slice(),
            )?;
        }
        if !dangling_refs.is_empty() {
            let reason = format!(
                "owner accepted reviewed dangling refs: {}",
                dangling_refs.join(",")
            );
            Store::append_audit_conn(
                &tx,
                "artifact.reconfirm_owner_accepted_dangling",
                None,
                None,
                Some(&reason),
                grant_id,
                &[],
                review_ref.as_slice(),
            )?;
        }
        if let Some(old) = superseded_old_version {
            let reason = format!(
                "{}:{} v{} superseded by v{}",
                kind, artifact_id, old, version
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
        if self
            .fail_next_owner_reconfirmation
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            self.fail_next_owner_reconfirmation
                .store(false, std::sync::atomic::Ordering::SeqCst);
            return Err(StoreError::LearnedArtifact(
                "injected owner reconfirmation transaction failure".into(),
            ));
        }
        tx.commit()?;
        Ok(true)
    }
}

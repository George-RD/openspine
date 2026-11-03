//! Init's explicit owner ceremony; never called by runtime discovery.
use jiff::Timestamp;
use openspine_schemas::artifact::{ArtifactRef, Lifecycle};
use openspine_schemas::ids::PrincipalId;
use openspine_schemas::lineage::ArtifactLineage;
use rusqlite::{params, OptionalExtension};
use ulid::Ulid;

use super::learned_artifacts::{dependency_fingerprint, Provenance};
use super::proposed_artifacts::ProposedArtifact;
use super::{Store, StoreError};

pub(crate) struct SeedAcceptance<'a> {
    pub owner: PrincipalId,
    pub id: &'a str,
    pub version: u32,
    pub yaml: ArtifactRef,
    pub exchange: ArtifactRef,
    pub source_path: &'a str,
    pub base_epoch: &'a str,
    pub dangling: &'a [String],
}

impl Store {
    /// Consume a parked request, establish real producing provenance, and
    /// activate its proposal in ONE transaction. No reconfirm request is minted
    /// for a fresh seed. Existing accepted/edited/erased/proposed rows are no-ops.
    pub(crate) fn accept_init_seed(&self, seed: SeedAcceptance<'_>) -> Result<bool, StoreError> {
        self.with_immediate_tx(|tx| {
            let owner_exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM principals WHERE id = ?1 AND is_owner = 1)",
                [seed.owner.to_string()], |row| row.get(0),
            )?;
            if !owner_exists {
                return Err(StoreError::LearnedArtifact("init acceptance requires the bound owner".into()));
            }
            let existing: Option<(String, String, Option<String>, Option<String>)> = tx.query_row(
                "SELECT provenance, compatibility, pending_yaml_digest, pending_reconfirmation_id
                 FROM learned_artifacts WHERE kind = 'workflow' AND artifact_id = ?1 AND version = ?2",
                params![seed.id, seed.version],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            ).optional()?;
            let pending = if let Some((provenance, status, digest, pending)) = existing.as_ref() {
                let provenance: Provenance = serde_json::from_str(provenance)
                    .map_err(|err| StoreError::LearnedArtifact(err.to_string()))?;
                if !matches!(provenance, Provenance::LegacyMigration { .. })
                    || status != "reconfirmation_required"
                    || digest.as_deref() != Some(seed.yaml.digest.as_str())
                {
                    return Ok(false);
                }
                pending.clone()
            } else {
                None
            };
            let proposal_exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM proposed_artifacts
                 WHERE kind = 'workflow' AND artifact_id = ?1 AND version >= ?2)",
                params![seed.id, seed.version], |row| row.get(0),
            )?;
            if proposal_exists { return Ok(false); }
            if let Some(request) = pending.as_deref() {
                let target = openspine_schemas::digest::digest_of(&serde_json::json!({
                    "kind": "workflow", "artifact_id": seed.id, "version": seed.version,
                }));
                let consumed = tx.execute(
                    "UPDATE action_requests SET used = 1 WHERE id = ?1 AND used = 0
                     AND json_extract(request_json, '$.action') = 'artifact.reconfirm'
                     AND json_extract(request_json, '$.payload_ref.digest') = ?2
                     AND json_extract(request_json, '$.target_digest') = ?3",
                    params![request, seed.yaml.digest.as_str(), target.as_str()],
                )?;
                if consumed != 1 { return Ok(false); }
            }
            let event = Self::append_audit_conn_with_actor(
                tx, "seed_workflows.owner_accepted", None, None,
                Some("explicit init ceremony acceptance of embedded workflow bytes (D-175)"),
                None, &[], &[seed.exchange.clone(), seed.yaml.clone()], None, None, Some(&seed.owner),
            )?;
            let provenance = Provenance::ProducedBy {
                source_event_id: event.id,
                source_exchange: seed.exchange,
                source_scope: openspine_schemas::provenance::ProvenanceOrigin::system(),
            };
            let provenance = serde_json::to_string(&provenance)
                .map_err(|err| StoreError::LearnedArtifact(err.to_string()))?;
            tx.execute(
                "INSERT INTO learned_artifacts
                 (kind, artifact_id, version, namespace, provenance, learned_at, compatibility,
                  nomination, pending_yaml_digest, source_path, accepted_base_epoch,
                  accepted_dependency_fingerprint)
                 VALUES ('workflow', ?1, ?2, 'overlay', ?3, ?4, 'owner_accepted', 'none', ?5, ?6, ?7, ?8)
                 ON CONFLICT(kind, artifact_id, version) DO UPDATE SET
                  provenance = excluded.provenance, compatibility = 'owner_accepted',
                  pending_reconfirmation_id = NULL, accepted_via = NULL,
                  accepted_base_epoch = excluded.accepted_base_epoch,
                  accepted_dependency_fingerprint = excluded.accepted_dependency_fingerprint",
                params![seed.id, seed.version, provenance, Timestamp::now().to_string(),
                    seed.yaml.digest.as_str(), seed.source_path, seed.base_epoch,
                    dependency_fingerprint(seed.dangling)],
            )?;
            let proposal = ProposedArtifact {
                id: Ulid::new(), kind: "workflow".into(), artifact_id: seed.id.into(),
                version: seed.version, state: Lifecycle::Proposed,
                yaml_digest: seed.yaml.digest.to_string(),
                // Init is an owner ceremony, not a delegated task. The legacy
                // NOT NULL column uses nil for this explicitly grantless origin.
                task_grant_id: Ulid::nil(),
                action_request_id: pending.as_deref().map(Ulid::from_string).transpose()
                    .map_err(|err| StoreError::LearnedArtifact(err.to_string()))?,
                proposed_at: Timestamp::now(), lineage: Some(ArtifactLineage::root()),
            };
            Self::activate_owner_proposal(tx, &proposal, None)?;
            Self::append_audit_conn_with_actor(
                tx, "artifact.activated", None, None, Some("owner init ceremony (D-175)"),
                None, &[], &[seed.yaml], None, None, Some(&seed.owner),
            )?;
            Ok(true)
        })
    }
}

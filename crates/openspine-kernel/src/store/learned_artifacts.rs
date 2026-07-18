//! Durable metadata for learned overlay artifacts (AD-023/070/071).

use jiff::Timestamp;
use openspine_schemas::artifact::{ArtifactNamespace, ArtifactRef};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use ulid::Ulid;

use super::{Store, StoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityStatus {
    Compatible,
    ReconfirmationRequired,
    OwnerAccepted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NominationStatus {
    None,
    Nominated,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReconfirmAnchor {
    pub request_id: Ulid,
    pub grant_event_id: Ulid,
    pub reviewed_ref: ArtifactRef,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    ProducedBy {
        source_event_id: Ulid,
        source_exchange: ArtifactRef,
    },
    LegacyMigration {
        discovered_at: Timestamp,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedArtifact {
    pub kind: String,
    pub artifact_id: String,
    pub version: u32,
    pub namespace: ArtifactNamespace,
    pub provenance: Provenance,
    pub accepted_via: Option<ReconfirmAnchor>,
    pub learned_at: Timestamp,
    pub compatibility: CompatibilityStatus,
    pub nomination: NominationStatus,
    pub pending_reconfirmation_id: Option<Ulid>,
    pub pending_yaml_digest: Option<String>,
    /// Canonical JSON array of dangling typed references accepted at review.
    /// A later dangling set is safe only when it is a subset of this anchor.
    pub accepted_dependency_fingerprint: Option<String>,
    pub source_path: Option<String>,
    pub accepted_base_epoch: Option<String>,
}

/// Canonical, order-independent durable anchor for reviewed dangling refs.
pub fn dependency_fingerprint(references: &[String]) -> String {
    let mut canonical = references.to_vec();
    canonical.sort();
    canonical.dedup();
    serde_json::to_string(&canonical).expect("string arrays always serialize")
}

/// Return whether every currently dangling reference was owner-accepted.
pub fn dependency_fingerprint_allows(current: &[String], accepted: Option<&str>) -> bool {
    if current.is_empty() {
        return true;
    }
    let Some(accepted) = accepted else {
        return false;
    };
    let Ok(mut allowed) = serde_json::from_str::<Vec<String>>(accepted) else {
        return false;
    };
    allowed.sort();
    allowed.dedup();
    current
        .iter()
        .all(|reference| allowed.binary_search(reference).is_ok())
}

pub(super) fn ensure_schema(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS learned_artifacts (
         kind TEXT NOT NULL, artifact_id TEXT NOT NULL, version INTEGER NOT NULL,
         namespace TEXT NOT NULL DEFAULT 'overlay', provenance TEXT NOT NULL,
         accepted_via TEXT, learned_at TEXT NOT NULL,
         compatibility TEXT NOT NULL DEFAULT 'compatible',
         nomination TEXT NOT NULL DEFAULT 'none', pending_reconfirmation_id TEXT,
         pending_yaml_digest TEXT, accepted_dependency_fingerprint TEXT,
         source_path TEXT, accepted_base_epoch TEXT,
         PRIMARY KEY(kind, artifact_id, version));",
    )?;
    Ok(())
}

/// Migrate pre-typed provenance rows. New columns are included for migrated
/// databases; ad-hoc ALTERs in `migrations.rs` cover databases already using
/// the typed schema.
pub(super) fn migrate_provenance_column(conn: &Connection) -> Result<(), StoreError> {
    let has_provenance: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('learned_artifacts') WHERE name = 'provenance'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .unwrap_or(false);
    if has_provenance {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(
        "ALTER TABLE learned_artifacts RENAME TO learned_artifacts_old;
         CREATE TABLE learned_artifacts (
           kind TEXT NOT NULL, artifact_id TEXT NOT NULL, version INTEGER NOT NULL,
           namespace TEXT NOT NULL DEFAULT 'overlay', provenance TEXT NOT NULL,
           accepted_via TEXT, learned_at TEXT NOT NULL,
           compatibility TEXT NOT NULL DEFAULT 'compatible', nomination TEXT NOT NULL DEFAULT 'none',
           pending_reconfirmation_id TEXT, pending_yaml_digest TEXT,
           accepted_dependency_fingerprint TEXT, source_path TEXT, accepted_base_epoch TEXT,
           PRIMARY KEY(kind, artifact_id, version));
         INSERT INTO learned_artifacts
           (kind, artifact_id, version, namespace, provenance, accepted_via, learned_at,
            compatibility, nomination, pending_reconfirmation_id, pending_yaml_digest,
            accepted_dependency_fingerprint)
         SELECT kind, artifact_id, version, namespace,
           json_object('produced_by', json_object(
             'source_event_id', source_event_id,
             'source_exchange', json_object('digest', source_exchange_digest,
                                             'schema_version', source_exchange_schema_version))),
           NULL, learned_at, compatibility, nomination, pending_reconfirmation_id, pending_yaml_digest,
           NULL
         FROM learned_artifacts_old;
         DROP TABLE learned_artifacts_old;",
    )?;
    tx.commit()?;
    Ok(())
}

fn status_name(status: CompatibilityStatus) -> &'static str {
    match status {
        CompatibilityStatus::Compatible => "compatible",
        CompatibilityStatus::ReconfirmationRequired => "reconfirmation_required",
        CompatibilityStatus::OwnerAccepted => "owner_accepted",
    }
}
fn nomination_name(status: NominationStatus) -> &'static str {
    match status {
        NominationStatus::None => "none",
        NominationStatus::Nominated => "nominated",
    }
}
fn parse_status(value: &str) -> Result<CompatibilityStatus, StoreError> {
    match value {
        "compatible" => Ok(CompatibilityStatus::Compatible),
        "reconfirmation_required" => Ok(CompatibilityStatus::ReconfirmationRequired),
        "owner_accepted" => Ok(CompatibilityStatus::OwnerAccepted),
        other => Err(StoreError::LearnedArtifact(format!(
            "unknown compatibility {other}"
        ))),
    }
}
fn parse_nomination(value: &str) -> Result<NominationStatus, StoreError> {
    match value {
        "none" => Ok(NominationStatus::None),
        "nominated" => Ok(NominationStatus::Nominated),
        other => Err(StoreError::LearnedArtifact(format!(
            "unknown nomination {other}"
        ))),
    }
}

impl Store {
    pub fn record_learned_artifact(&self, artifact: &LearnedArtifact) -> Result<(), StoreError> {
        if artifact.namespace != ArtifactNamespace::Overlay {
            return Err(StoreError::LearnedArtifact(
                "learned artifacts must be overlay-owned".into(),
            ));
        }
        let conn = self.conn.lock();
        Self::insert_learned_artifact_conn(&conn, artifact)
    }

    /// Like [`Self::record_learned_artifact`] but inserts the learned-artifact
    /// row and appends an audit event (`audit_kind`, `audit_reason`) in a
    /// single SQLite transaction, so the authoritative row and its receipt
    /// can never diverge. This is the provenance-binding path used by
    /// non-proposable bootstrap artifacts (e.g. persona seed, AD-080) where a
    /// separate row-then-audit call would leave a permanently unrecoverable
    /// row if the audit append failed (a restart skips the already-recorded
    /// id and never backfills the missing receipt).
    pub fn record_learned_artifact_with_audit(
        &self,
        artifact: &LearnedArtifact,
        audit_kind: &str,
        audit_reason: &str,
    ) -> Result<(), StoreError> {
        if artifact.namespace != ArtifactNamespace::Overlay {
            return Err(StoreError::LearnedArtifact(
                "learned artifacts must be overlay-owned".into(),
            ));
        }
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::insert_learned_artifact_conn(&tx, artifact)?;
        Self::append_audit_conn(
            &tx,
            audit_kind,
            None,
            None,
            Some(audit_reason),
            None,
            &[],
            &[],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Remove a malformed learned row after recording the quarantine decision.
    pub fn quarantine_learned_artifact(
        &self,
        kind: &str,
        artifact_id: &str,
        version: u32,
        reason: &str,
    ) -> Result<bool, StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM learned_artifacts
                 WHERE kind = ?1 AND artifact_id = ?2 AND version = ?3",
                params![kind, artifact_id, version as i64],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Ok(false);
        }
        Self::append_audit_conn(
            &tx,
            "artifact.persona_quarantined",
            None,
            None,
            Some(reason),
            None,
            &[],
            &[],
        )?;
        tx.execute(
            "DELETE FROM learned_artifacts
             WHERE kind = ?1 AND artifact_id = ?2 AND version = ?3",
            params![kind, artifact_id, version as i64],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Insert a learned-artifact row using an existing connection/transaction.
    /// Shared by [`Self::record_learned_artifact`] (auto-committed) and
    /// [`Self::record_learned_artifact_with_audit`] (wrapped in a transaction
    /// with a paired audit event). Behavior is identical either way.
    fn insert_learned_artifact_conn(
        conn: &Connection,
        artifact: &LearnedArtifact,
    ) -> Result<(), StoreError> {
        let provenance = serde_json::to_string(&artifact.provenance)
            .map_err(|err| StoreError::LearnedArtifact(format!("provenance json: {err}")))?;
        let accepted_via = artifact
            .accepted_via
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|err| StoreError::LearnedArtifact(format!("accepted_via json: {err}")))?;
        conn.execute(
            "INSERT INTO learned_artifacts
             (kind, artifact_id, version, provenance, accepted_via, learned_at, compatibility,
              nomination, pending_reconfirmation_id, pending_yaml_digest, source_path,
              accepted_base_epoch, accepted_dependency_fingerprint)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                artifact.kind,
                artifact.artifact_id,
                artifact.version as i64,
                provenance,
                accepted_via,
                artifact.learned_at.to_string(),
                status_name(artifact.compatibility),
                nomination_name(artifact.nomination),
                artifact.pending_reconfirmation_id.map(|id| id.to_string()),
                artifact.pending_yaml_digest,
                artifact.source_path,
                artifact.accepted_base_epoch,
                artifact.accepted_dependency_fingerprint
            ],
        )?;
        Ok(())
    }

    pub fn list_learned_artifacts(&self) -> Result<Vec<LearnedArtifact>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT kind, artifact_id, version, namespace, provenance, accepted_via, learned_at,
                    compatibility, nomination, pending_reconfirmation_id, pending_yaml_digest,
                    source_path, accepted_base_epoch, accepted_dependency_fingerprint
             FROM learned_artifacts ORDER BY kind, artifact_id, version",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<String>>(12)?,
                row.get::<_, Option<String>>(13)?,
            ))
        })?;
        rows.map(|row| {
            let (
                kind,
                artifact_id,
                version,
                namespace,
                provenance,
                accepted_via,
                learned_at,
                compatibility,
                nomination,
                pending,
                pending_digest,
                source_path,
                epoch,
                fingerprint,
            ) = row?;
            let namespace = match namespace.as_str() {
                "overlay" => ArtifactNamespace::Overlay,
                "base" => ArtifactNamespace::Base,
                other => {
                    return Err(rusqlite::Error::InvalidParameterName(format!(
                        "namespace {other}"
                    )))
                }
            };
            Ok(LearnedArtifact {
                kind,
                artifact_id,
                version: version as u32,
                namespace,
                provenance: serde_json::from_str(&provenance)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                accepted_via: accepted_via
                    .map(|raw| {
                        serde_json::from_str(&raw).map_err(|_| rusqlite::Error::InvalidQuery)
                    })
                    .transpose()?,
                learned_at: learned_at
                    .parse()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                compatibility: parse_status(&compatibility)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                nomination: parse_nomination(&nomination)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                pending_reconfirmation_id: pending
                    .as_deref()
                    .map(Ulid::from_string)
                    .transpose()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                pending_yaml_digest: pending_digest,
                source_path,
                accepted_base_epoch: epoch,
                accepted_dependency_fingerprint: fingerprint,
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
    }

    pub fn mark_reconfirmation_required(
        &self,
        kind: &str,
        artifact_id: &str,
        version: u32,
        request_id: Ulid,
        yaml_digest: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        let changed = conn.execute(
            "UPDATE learned_artifacts SET compatibility = 'reconfirmation_required',
             pending_reconfirmation_id = ?1, pending_yaml_digest = ?2
             WHERE kind = ?3 AND artifact_id = ?4 AND version = ?5",
            params![
                request_id.to_string(),
                yaml_digest,
                kind,
                artifact_id,
                version as i64
            ],
        )?;
        if changed == 0 {
            return Err(StoreError::LearnedArtifact(
                "learned artifact provenance row not found".into(),
            ));
        }
        Ok(())
    }

    /// Silently refresh the stored base compatibility epoch for a durably
    /// owner-accepted artifact when the base epoch changed but its typed
    /// references remain compatible. Keeps `OwnerAccepted` and all provenance
    /// intact — no exclusion, no prompt (AD-070 revalidation semantics).
    pub fn refresh_owner_accepted_epoch(
        &self,
        kind: &str,
        artifact_id: &str,
        version: u32,
        epoch: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE learned_artifacts SET accepted_base_epoch = ?1 \
             WHERE kind = ?2 AND artifact_id = ?3 AND version = ?4 \
             AND compatibility = 'owner_accepted'",
            params![epoch, kind, artifact_id, version as i64],
        )?;
        Ok(())
    }

    pub fn nominate_upstream(
        &self,
        kind: &str,
        artifact_id: &str,
        version: u32,
        depersonalized: bool,
    ) -> Result<(), StoreError> {
        if !depersonalized {
            return Err(StoreError::LearnedArtifact(
                "upstream nomination requires explicit depersonalized opt-in".into(),
            ));
        }
        let conn = self.conn.lock();
        let changed = conn.execute("UPDATE learned_artifacts SET nomination = 'nominated' WHERE kind = ?1 AND artifact_id = ?2 AND version = ?3 AND compatibility = 'compatible'", params![kind, artifact_id, version as i64])?;
        if changed == 0 {
            return Err(StoreError::LearnedArtifact(
                "compatible learned artifact not found".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "learned_artifacts_tests.rs"]
mod tests;

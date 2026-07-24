// openspine:allow-large-module reason: learned-artifact metadata is one audit boundary; crypto-erasure invalidation (AD-140) must stay co-located with the provenance schema and the erased-scope marker that gates its audit row.
//! Durable metadata for learned overlay artifacts (AD-023/070/071).

use jiff::Timestamp;
use openspine_schemas::artifact::{ArtifactNamespace, ArtifactRef};
use openspine_schemas::digest::digest_of_bytes;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use ulid::Ulid;

use super::{Store, StoreError};
use crate::artifact_store::ArtifactStore;
use crate::counterparty_keys::SYSTEM_SCOPE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityStatus {
    Compatible,
    ReconfirmationRequired,
    OwnerAccepted,
    /// The artifact was derived (via its provenance link, D-077) from a
    /// counterparty that has been crypto-erased (AD-140). Its source
    /// exchange is permanently undecryptable, so the learned artifact can
    /// never be re-derived or re-confirmed — unlike `ReconfirmationRequired`
    /// it is terminal rather than a prompt awaiting owner action.
    Erased,
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
        /// The counterparty scope `source_exchange` was stored under at
        /// production time (AD-140). Required, not inferred from the blob
        /// store at erasure time: two counterparties can store identical
        /// plaintext, which content-addresses to the SAME `source_exchange`
        /// digest, so a path-existence or blob-header check alone cannot
        /// tell which counterparty actually produced THIS artifact. Recording
        /// the producing scope in the provenance edge itself is what makes
        /// crypto-erase invalidation exact rather than a byproduct of
        /// digest collisions.
        source_scope: Ulid,
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

/// Stable identity of a learned artifact invalidated by counterparty erasure.
///
/// The runtime registry needs the exact tuple to evict the artifact
/// immediately after the durable erasure transaction commits. Audit data uses
/// a digest of this tuple so D-012 never gains learned payload content.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LearnedArtifactIdentity {
    pub kind: String,
    pub artifact_id: String,
    pub version: u32,
}

impl LearnedArtifactIdentity {
    fn audit_ref(&self) -> ArtifactRef {
        let mut canonical = b"openspine:learned-artifact-identity:v1\0".to_vec();
        canonical
            .extend(serde_json::to_vec(self).expect("learned artifact identity always serializes"));
        ArtifactRef {
            digest: digest_of_bytes(&canonical),
            schema_version: 1,
        }
    }
}

pub(crate) struct LearnedArtifactErasure {
    /// Every terminal identity matching the erased scope, including identities
    /// already erased by an earlier pass. Callers use this complete set to
    /// repeat process-local cleanup safely after a partial failure.
    pub invalidated: Vec<LearnedArtifactIdentity>,
    /// Rows newly transitioned to `Erased` by this pass.
    pub newly_invalidated: usize,
    pub key_deleted: bool,
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
         PRIMARY KEY(kind, artifact_id, version));
         /* AD-140: durable, transaction-atomic marker of 'this counterparty
            has been crypto-erased at least once'. Gates the
            mark_learned_artifacts_erased audit-row decision (NOT filesystem
            tombstone existence -- the filesystem key-ring tombstone and this
            DB row are written in SEPARATE operations that are not atomic
            with each other; if the filesystem operation fails after this
            row/its audit row already committed, a retry must complete the
            filesystem cleanup WITHOUT re-appending a duplicate audit event,
            and this table's own PRIMARY KEY-conflict-on-retry is what makes
            that safe). */
         CREATE TABLE IF NOT EXISTS erased_counterparties (
           counterparty_id TEXT PRIMARY KEY,
           erased_at TEXT NOT NULL
         );",
    )?;
    // Pre-typed legacy tables lack `provenance` until
    // `migrate_provenance_column` rebuilds them. Installing the trigger
    // earlier fails because the WHEN clause references NEW.provenance.
    if table_has_column(conn, "learned_artifacts", "provenance")? {
        ensure_closure_triggers(conn)?;
    }
    Ok(())
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, StoreError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2",
        params![table, column],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn ensure_closure_triggers(conn: &Connection) -> Result<(), StoreError> {
    // The database marker is the durable closure boundary. Triggers enforce
    // it for every insertion path, including activation's upsert, without a
    // check-then-insert race. SYSTEM_SCOPE remains valid for
    // internal/unattributable and migrated legacy provenance.
    conn.execute_batch(&format!(
        "CREATE TRIGGER IF NOT EXISTS reject_closed_scope_learned_artifact_insert
           BEFORE INSERT ON learned_artifacts
           WHEN json_extract(NEW.provenance, '$.produced_by.source_scope') != '{system_scope}'
            AND EXISTS (
              SELECT 1 FROM erased_counterparties
               WHERE counterparty_id =
                     json_extract(NEW.provenance, '$.produced_by.source_scope')
            )
           BEGIN
             SELECT RAISE(ABORT, 'learned artifact source scope is erased');
           END;
         CREATE TRIGGER IF NOT EXISTS reject_closed_scope_learned_artifact_provenance_update
           BEFORE UPDATE OF provenance ON learned_artifacts
           WHEN json_extract(NEW.provenance, '$.produced_by.source_scope') != '{system_scope}'
            AND EXISTS (
              SELECT 1 FROM erased_counterparties
               WHERE counterparty_id =
                     json_extract(NEW.provenance, '$.produced_by.source_scope')
            )
           BEGIN
             SELECT RAISE(ABORT, 'learned artifact source scope is erased');
           END;",
        system_scope = SYSTEM_SCOPE
    ))?;
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
        // Current-main databases already have typed provenance JSON, but
        // ProducedBy rows created before AD-140 lack source_scope. Upgrade
        // only that missing member; valid scopes and LegacyMigration rows are
        // untouched. A single UPDATE statement is atomic and idempotent.
        conn.execute(
            "UPDATE learned_artifacts
                SET provenance = json_set(
                    provenance, '$.produced_by.source_scope', ?1
                )
              WHERE json_type(provenance, '$.produced_by') = 'object'
                AND json_type(
                    provenance, '$.produced_by.source_scope'
                ) IS NULL",
            params![SYSTEM_SCOPE.to_string()],
        )?;
        ensure_closure_triggers(conn)?;
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    // Pre-typed-provenance rows predate per-counterparty scoping (AD-140)
    // entirely, so they belong to the reserved SYSTEM_SCOPE -- interpolated
    // (not a bind parameter: `execute_batch` runs a script, not one
    // parameterized statement) as a fixed constant, never user input.
    let sql = format!(
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
                                             'schema_version', source_exchange_schema_version),
             'source_scope', '{system_scope}')),
           NULL, learned_at, compatibility, nomination, pending_reconfirmation_id, pending_yaml_digest,
           NULL
         FROM learned_artifacts_old;
         DROP TABLE learned_artifacts_old;",
        system_scope = SYSTEM_SCOPE
    );
    tx.execute_batch(&sql)?;
    // Renaming the old table retargets its triggers to
    // `learned_artifacts_old`, and dropping it removes those triggers. Restore
    // them against the replacement table before this rebuild can commit.
    ensure_closure_triggers(&tx)?;
    tx.commit()?;
    Ok(())
}

fn status_name(status: CompatibilityStatus) -> &'static str {
    match status {
        CompatibilityStatus::Compatible => "compatible",
        CompatibilityStatus::ReconfirmationRequired => "reconfirmation_required",
        CompatibilityStatus::OwnerAccepted => "owner_accepted",
        CompatibilityStatus::Erased => "erased",
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
        "erased" => Ok(CompatibilityStatus::Erased),
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
        let compatibility: Option<String> = tx
            .query_row(
                "SELECT compatibility FROM learned_artifacts
                 WHERE kind = ?1 AND artifact_id = ?2 AND version = ?3",
                params![kind, artifact_id, version as i64],
                |row| row.get(0),
            )
            .optional()?;
        let Some(compatibility) = compatibility else {
            return Ok(false);
        };
        if compatibility == status_name(CompatibilityStatus::Erased) {
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
             WHERE kind = ?1 AND artifact_id = ?2 AND version = ?3
               AND compatibility != 'erased'",
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
                    )));
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
             WHERE kind = ?3 AND artifact_id = ?4 AND version = ?5
               AND compatibility != 'erased'",
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
    /// Crypto-erase invalidation (AD-140): mark every learned artifact whose
    /// `Provenance::ProducedBy.source_scope` equals `counterparty_id` as
    /// permanently `Erased`, and permanently close that counterparty's
    /// scope (in memory first, then tombstone + physical key deletion via
    /// `ArtifactStore::erase_counterparty_key_locked`). Returns every matching
    /// terminal identity, the number newly transitioned, and key cleanup state.
    ///
    /// **Locking (closes the write/erase race).** The ENTIRE operation —
    /// durable-marker check/insert, the DB transaction, and key deletion —
    /// runs inside `artifacts.with_scope_lock(counterparty_id, ..)`, the
    /// SAME per-counterparty in-process lock `ArtifactStore::put_scoped`
    /// holds across its own full key-fetch-through-durable-write. Without
    /// this, a `put_scoped` call could complete (return `Ok(ref)`) using a
    /// key that THIS function deletes moments later — silently orphaning a
    /// write that was never actually readable. With the shared lock, a
    /// write and an erasure for the same counterparty can never interleave:
    /// whichever acquires the lock first fully completes before the other
    /// proceeds. (Holding `self.conn.lock()` alone — a SEPARATE lock — does
    /// NOT provide this: `put_scoped`/key creation never touch `conn`, so
    /// it only serializes concurrent DB row inserts, not artifact-blob
    /// writes. Single-process guarantee only — see `CounterpartyKeyRing`'s
    /// module doc for the cross-process caveat.)
    ///
    /// **Ordering (audit-before-effect) + idempotent retries.** The
    /// invalidation `UPDATE`, the `counterparty.erased` audit row, and the
    /// durable erased-scope marker are all committed in ONE `BEGIN
    /// IMMEDIATE` transaction FIRST. The (un-rollbackable) filesystem key
    /// deletion happens AFTER `COMMIT` — never inside the SQL transaction —
    /// so a commit/audit failure can never leave the key gone with no
    /// committed audit. The marker (`erased_counterparties` table, PK =
    /// `counterparty_id`, `INSERT OR IGNORE`) is the SINGLE source of truth
    /// for "has this scope been closed at least once" and gates the audit
    /// row — NOT filesystem tombstone existence, which is a SEPARATE
    /// operation not atomic with this transaction. Concretely: if the key
    /// deletion fails after the transaction committed, a retry re-enters
    /// the same scope-locked block, re-runs the (no-op) UPDATE, and re-runs
    /// the marker `INSERT OR IGNORE` which now affects ZERO rows — so it
    /// appends NO duplicate audit and simply retries the filesystem
    /// cleanup. The audit row is therefore emitted at most once per erase,
    /// even across partial-failure retries, and the chain stays verifiable.
    ///
    /// **Unconditional key-erasure call.** `erase_counterparty_key_locked`
    /// is called EVERY time (not gated on "a key currently exists"): a
    /// counterparty who never stored anything still gets tombstoned on
    /// their first erase call — permanently closing that scope to future
    /// writes — which a conditional "only if a key exists" guard would skip
    /// entirely, leaving that scope silently re-usable. Idempotency of the
    /// audit/chain is guaranteed by the DB marker above, not by
    /// key-existence.
    ///
    /// Resolution is by `source_scope` (the producing scope recorded in the
    /// provenance edge), the only reliable match: two counterparties can
    /// store identical plaintext (same digest), so the digest alone cannot
    /// attribute the producing scope. LegacyMigration provenance has no
    /// `source_scope`, so it never matches an erase.
    pub(crate) fn mark_learned_artifacts_erased(
        &self,
        counterparty_id: Ulid,
        artifacts: &ArtifactStore,
    ) -> Result<LearnedArtifactErasure, StoreError> {
        if counterparty_id == SYSTEM_SCOPE {
            return Err(StoreError::LearnedArtifact(
                "SYSTEM_SCOPE is reserved and cannot be erased".into(),
            ));
        }
        artifacts.with_scope_lock(
            counterparty_id,
            || -> Result<LearnedArtifactErasure, StoreError> {
                let mut conn = self.conn.lock();
                let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

                // Resolve every matching identity before changing rows. Retry
                // callers need the complete terminal set for process-local
                // cleanup, while the status flag separately identifies rows
                // that this transaction will newly transition.
                let matching = {
                    let mut stmt = tx.prepare(
                        "SELECT kind, artifact_id, version,
                                compatibility != 'erased'
                           FROM learned_artifacts
                          WHERE json_extract(
                                    provenance,
                                    '$.produced_by.source_scope'
                                ) = ?1
                          ORDER BY kind, artifact_id, version",
                    )?;
                    let rows = stmt.query_map(params![counterparty_id.to_string()], |row| {
                        Ok((
                            LearnedArtifactIdentity {
                                kind: row.get(0)?,
                                artifact_id: row.get(1)?,
                                version: row.get::<_, i64>(2)? as u32,
                            },
                            row.get::<_, bool>(3)?,
                        ))
                    })?;
                    let mut vec = Vec::new();
                    for item in rows {
                        vec.push(item?);
                    }
                    vec
                };

                // Consume every still-live reconfirmation request before
                // clearing the row link. A stale owner tap then observes a
                // consumed request and can never revive terminal Erased.
                tx.execute(
                    "UPDATE action_requests
                        SET used = 1
                      WHERE used = 0
                        AND id IN (
                          SELECT pending_reconfirmation_id
                            FROM learned_artifacts
                           WHERE json_extract(
                                     provenance,
                                     '$.produced_by.source_scope'
                                 ) = ?1
                             AND pending_reconfirmation_id IS NOT NULL
                        )",
                    params![counterparty_id.to_string()],
                )?;
                let newly_invalidated = tx.execute(
                    "UPDATE learned_artifacts
                        SET compatibility = 'erased',
                            pending_reconfirmation_id = NULL,
                            pending_yaml_digest = NULL
                      WHERE json_extract(
                                provenance,
                                '$.produced_by.source_scope'
                            ) = ?1
                        AND compatibility != 'erased'",
                    params![counterparty_id.to_string()],
                )?;

                // Revoke every runtime effect addressed by the exact learned
                // artifact identities resolved above. Tuple-bound predicates
                // keep another version (or another artifact kind with the
                // same id) live, while retries also clean up any runtime row
                // that was resurrected after its learned row became terminal.
                let invalidated_at = Timestamp::now();
                let revoked_at = super::standing_rules::timestamp_to_epoch_nanos(invalidated_at)?;
                for (identity, _) in &matching {
                    match identity.kind.as_str() {
                        "standing_rule" => {
                            tx.execute(
                                "UPDATE standing_rules
                                    SET status = 'revoked', revoked_at = ?3
                                  WHERE artifact_id = ?1 AND version = ?2
                                    AND status = 'active'",
                                params![
                                    identity.artifact_id.as_str(),
                                    identity.version as i64,
                                    revoked_at,
                                ],
                            )?;
                        }
                        "model_swap" => {
                            tx.execute(
                                "UPDATE proposed_artifacts
                                    SET state = 'retired'
                                  WHERE kind = 'model_swap'
                                    AND artifact_id = ?1 AND version = ?2
                                    AND state = 'active'",
                                params![identity.artifact_id.as_str(), identity.version as i64,],
                            )?;
                        }
                        _ => {}
                    }
                }

                let marker_new = tx.execute(
                    "INSERT OR IGNORE INTO erased_counterparties
                             (counterparty_id, erased_at) VALUES (?1, ?2)",
                    params![counterparty_id.to_string(), invalidated_at.to_string()],
                )?;

                if newly_invalidated > 0 || marker_new > 0 {
                    let target_refs: Vec<_> = matching
                        .iter()
                        .filter(|(_, was_live)| *was_live)
                        .map(|(identity, _)| identity.audit_ref())
                        .collect();
                    let aggregate = format!("counterparty:{counterparty_id}");
                    let reason = format!(
                        "invalidated {newly_invalidated} derived artifacts via provenance links"
                    );
                    Self::append_audit_conn_with_options(
                        &tx,
                        "counterparty.erased",
                        None,
                        None,
                        Some(&reason),
                        None,
                        &target_refs,
                        &[],
                        Some(&aggregate),
                        None,
                    )?;
                }

                // Transaction owns rollback on every early error, including a
                // failed COMMIT. Immediately after the durable marker,
                // invalidations, reconfirmation cancellation, and audit commit,
                // fail closed in this process while the scope lock is still
                // held. Fallible filesystem cleanup follows only after that
                // non-fallible in-memory closure.
                tx.commit()?;
                artifacts.close_counterparty_scope_in_memory(counterparty_id);
                let key_deleted = artifacts
                    .erase_counterparty_key_locked(counterparty_id)
                    .map_err(StoreError::ArtifactStore)?;
                Ok(LearnedArtifactErasure {
                    invalidated: matching.into_iter().map(|(identity, _)| identity).collect(),
                    newly_invalidated,
                    key_deleted,
                })
            },
        )
    }

    /// Durable closure markers awaiting (or safe to repeat through) key-ring
    /// tombstone reconciliation at startup.
    pub fn erased_counterparty_ids(&self) -> Result<Vec<Ulid>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT counterparty_id
               FROM erased_counterparties
              ORDER BY counterparty_id",
        )?;
        let raw = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        raw.into_iter()
            .map(|value| Ulid::from_string(&value).map_err(|_| StoreError::BadUlid(value)))
            .collect()
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

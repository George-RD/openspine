//! Inactive package index and audit pairing. All SQL stays inside Store.
use jiff::Timestamp;
use openspine_schemas::audit::AuditEvent;
use openspine_schemas::event_bus::EventSubscriptionFilter;
use rusqlite::{params, Connection, OptionalExtension};
use ulid::Ulid;

use super::{Store, StoreError};
use crate::package::install_types::{
    crash_at, InstallMetadata, InstallReceipt, PackageIdentity, PackageProvenance,
};

pub(super) fn ensure_schema(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS installed_packages (
            package_id TEXT NOT NULL,
            revision INTEGER NOT NULL CHECK (revision > 0 AND revision <= 4294967295),
            content_digest TEXT NOT NULL,
            installation_id TEXT NOT NULL UNIQUE,
            audit_seq INTEGER NOT NULL UNIQUE REFERENCES audit_log(seq),
            PRIMARY KEY (package_id, revision)
        );
        CREATE TABLE IF NOT EXISTS package_install_attempts (
            installation_id TEXT PRIMARY KEY,
            metadata_json TEXT NOT NULL,
            state TEXT NOT NULL CHECK (state IN ('prepared','committed','conflict','interrupted','reused')),
            cleanup_pending INTEGER NOT NULL CHECK (cleanup_pending IN (0,1))
        );",
    )?;
    Ok(())
}

pub(crate) enum PrepareResult {
    Prepared(InstallMetadata),
    Existing(InstallReceipt),
    Conflict,
}

pub(crate) enum CommitResult {
    Committed(InstallReceipt),
    Existing(InstallReceipt),
    Conflict,
}

impl Store {
    pub(crate) fn prepare_package_install(
        &self,
        identity: PackageIdentity,
        provenance: PackageProvenance,
    ) -> Result<PrepareResult, StoreError> {
        let metadata = InstallMetadata {
            installation_id: Ulid::new(),
            identity,
            provenance,
        };
        self.with_immediate_tx(|tx| {
            if let Some(existing) = load_receipt(tx, &metadata.identity)? {
                if existing.identity() == metadata.identity {
                    return Ok(PrepareResult::Existing(existing));
                }
                append(tx, "package.install_conflict", &metadata)?;
                insert_attempt(tx, &metadata, "conflict", false)?;
                return Ok(PrepareResult::Conflict);
            }
            append(tx, "package.install_prepared", &metadata)?;
            insert_attempt(tx, &metadata, "prepared", true)?;
            Ok(PrepareResult::Prepared(metadata))
        })
    }

    /// The caller holds the data-root lifetime lock and has durably published
    /// and verified the object. Index + audit + attempt transition commit once.
    pub(crate) fn commit_package_install(
        &self,
        metadata: &InstallMetadata,
    ) -> Result<CommitResult, StoreError> {
        self.with_immediate_tx(|tx| {
            let (stored, state): (String, String) = tx.query_row(
                "SELECT metadata_json, state FROM package_install_attempts WHERE installation_id = ?1",
                [metadata.installation_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if serde_json::from_str::<InstallMetadata>(&stored)? != *metadata || state != "prepared" {
                return Err(inconsistent());
            }
            if let Some(existing) = load_receipt(tx, &metadata.identity)? {
                let same = existing.identity() == metadata.identity;
                let (kind, state) = if same {
                    ("package.install_reused", "reused")
                } else {
                    ("package.install_conflict", "conflict")
                };
                append(tx, kind, metadata)?;
                transition(tx, metadata.installation_id, state)?;
                return Ok(if same {
                    CommitResult::Existing(existing)
                } else {
                    CommitResult::Conflict
                });
            }
            let audit_seq = append(tx, "package.install_committed", metadata)?;
            crash_at("after-audit-before-index");
            tx.execute(
                "INSERT INTO installed_packages (package_id, revision, content_digest, installation_id, audit_seq)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![metadata.identity.package_id, metadata.identity.revision,
                    metadata.identity.content_digest.as_str(), metadata.installation_id.to_string(), audit_seq],
            )?;
            transition(tx, metadata.installation_id, "committed")?;
            crash_at("before-index-commit");
            Ok(CommitResult::Committed(
                load_receipt(tx, &metadata.identity)?.ok_or_else(inconsistent)?,
            ))
        })
    }

    pub(crate) fn interrupt_package_install(
        &self,
        metadata: &InstallMetadata,
    ) -> Result<(), StoreError> {
        self.with_immediate_tx(|tx| {
            let (stored, state): (String, String) = tx.query_row(
                "SELECT metadata_json, state FROM package_install_attempts WHERE installation_id = ?1",
                [metadata.installation_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if serde_json::from_str::<InstallMetadata>(&stored)? != *metadata {
                return Err(inconsistent());
            }
            if state == "prepared" {
                append(tx, "package.install_interrupted", metadata)?;
                transition(tx, metadata.installation_id, "interrupted")?;
            }
            Ok(())
        })
    }

    /// Called only under the exclusive data-root lifetime lock. A surviving
    /// prepare belongs to a dead invocation, never to a concurrent installer.
    /// Recovery does not infer an installation from directory presence.
    pub(crate) fn recover_package_attempts(&self) -> Result<Vec<Ulid>, StoreError> {
        self.with_immediate_tx(|tx| {
            let pending = tx
                .prepare("SELECT metadata_json FROM package_install_attempts WHERE state = 'prepared'")?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            for json in pending {
                let metadata: InstallMetadata = serde_json::from_str(&json)?;
                append(tx, "package.install_recovered", &metadata)?;
                transition(tx, metadata.installation_id, "interrupted")?;
            }
            let ids = tx
                .prepare("SELECT installation_id FROM package_install_attempts WHERE cleanup_pending = 1 AND state <> 'prepared'")?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids.into_iter()
                .map(|id| id.parse().map_err(|_| StoreError::BadUlid(id)))
                .collect()
        })
    }

    pub(crate) fn finish_package_staging_cleanup(&self, id: Ulid) -> Result<(), StoreError> {
        self.with_immediate_tx(|tx| {
            tx.execute(
                "UPDATE package_install_attempts SET cleanup_pending = 0 WHERE installation_id = ?1 AND state <> 'prepared'",
                [id.to_string()],
            )?;
            Ok(())
        })
    }

    pub(crate) fn installed_packages(&self) -> Result<Vec<InstallReceipt>, StoreError> {
        self.with_deferred_read(|tx| {
            let identities = tx
                .prepare("SELECT package_id, revision FROM installed_packages ORDER BY package_id, revision")?
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            identities.into_iter()
                .map(|(id, revision)| load_receipt_by_key(tx, &id, revision)?.ok_or_else(inconsistent))
                .collect()
        })
    }

    pub(crate) fn package_install_receipts(&self) -> Result<Vec<serde_json::Value>, StoreError> {
        self.with_deferred_read(|tx| {
            let rows = tx.prepare(
                "SELECT seq, event_json FROM audit_log WHERE kind IN (
                  'package.install_prepared', 'package.install_committed', 'package.install_conflict',
                  'package.install_interrupted', 'package.install_recovered', 'package.install_reused') ORDER BY seq",
            )?.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter().map(|(seq, json)| {
                let event: AuditEvent = serde_json::from_str(&json)?;
                let metadata: InstallMetadata = serde_json::from_str(event.payload_json.as_deref().ok_or_else(inconsistent)?)?;
                Ok(serde_json::json!({
                    "audit_seq": seq, "audit_id": event.id, "recorded_at": event.ts,
                    "event": event.kind, "metadata": metadata,
                }))
            }).collect()
        })
    }

    /// Package maintenance neither serves tasks nor runs standing-rule sweeps.
    /// Its caller still holds the normal lock and checks pending operations.
    pub(crate) fn validate_package_ledger(&self) -> Result<(), StoreError> {
        if !self.verify_audit_chain()? {
            return Err(inconsistent());
        }
        // Chain verification authenticates meta_json, not the redundant
        // event_json projection used by receipts. Reuse the existing replay
        // validator to bind every delivered field to that hashed metadata.
        self.replay_audit(&EventSubscriptionFilter::all(), 0)?;
        if matches!(
            self.validate_boot_clock(Timestamp::now().as_millisecond())?,
            super::BootClockCheck::Regressed { .. }
        ) {
            return Err(inconsistent());
        }
        self.with_deferred_read(|tx| {
            let committed: i64 = tx.query_row(
                "SELECT COUNT(*) FROM audit_log WHERE kind = 'package.install_committed'",
                [],
                |row| row.get(0),
            )?;
            let indexed: i64 =
                tx.query_row("SELECT COUNT(*) FROM installed_packages", [], |row| {
                    row.get(0)
                })?;
            if committed != indexed {
                return Err(inconsistent());
            }
            Ok(())
        })?;
        self.installed_packages()?;
        Ok(())
    }
}

fn append(conn: &Connection, kind: &str, metadata: &InstallMetadata) -> Result<i64, StoreError> {
    let json = serde_json::to_string(metadata)?;
    let event = Store::append_audit_conn_with_options(
        conn,
        kind,
        None,
        None,
        Some("Inactive package management; no selection or authority change."),
        None,
        &[],
        &[],
        Some(&format!("package_install:{}", metadata.installation_id)),
        Some(&json),
    )?;
    Ok(conn.query_row(
        "SELECT seq FROM audit_log WHERE id = ?1",
        [event.id.to_string()],
        |row| row.get(0),
    )?)
}

fn insert_attempt(
    conn: &Connection,
    metadata: &InstallMetadata,
    state: &str,
    cleanup: bool,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO package_install_attempts (installation_id, metadata_json, state, cleanup_pending) VALUES (?1, ?2, ?3, ?4)",
        params![metadata.installation_id.to_string(), serde_json::to_string(metadata)?, state, cleanup],
    )?;
    Ok(())
}

fn transition(conn: &Connection, id: Ulid, state: &str) -> Result<(), StoreError> {
    if conn.execute("UPDATE package_install_attempts SET state = ?1 WHERE installation_id = ?2 AND state = 'prepared'", params![state, id.to_string()])? != 1 {
        return Err(inconsistent());
    }
    Ok(())
}

fn load_receipt(
    conn: &Connection,
    identity: &PackageIdentity,
) -> Result<Option<InstallReceipt>, StoreError> {
    load_receipt_by_key(conn, &identity.package_id, identity.revision)
}

fn load_receipt_by_key(
    conn: &Connection,
    id: &str,
    revision: u32,
) -> Result<Option<InstallReceipt>, StoreError> {
    let row: Option<(String, String, i64, Option<String>)> = conn
        .query_row(
            "SELECT p.content_digest, p.installation_id, p.audit_seq, a.event_json
         FROM installed_packages p LEFT JOIN audit_log a ON a.seq = p.audit_seq
         WHERE p.package_id = ?1 AND p.revision = ?2",
            params![id, revision],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((digest, installation_id, audit_seq, event_json)) = row else {
        return Ok(None);
    };
    let event: AuditEvent = serde_json::from_str(&event_json.ok_or_else(inconsistent)?)?;
    let metadata: InstallMetadata =
        serde_json::from_str(event.payload_json.as_deref().ok_or_else(inconsistent)?)?;
    if event.kind.as_str() != "package.install_committed"
        || metadata.installation_id.to_string() != installation_id
        || metadata.identity.package_id != id
        || metadata.identity.revision != revision
        || metadata.identity.content_digest.as_str() != digest
        || metadata.identity.inventory_format_version != 1
        || audit_seq <= 0
    {
        return Err(inconsistent());
    }
    Ok(Some(InstallReceipt {
        installation_id: metadata.installation_id,
        package_id: metadata.identity.package_id,
        revision: metadata.identity.revision,
        inventory_format_version: metadata.identity.inventory_format_version,
        content_digest: metadata.identity.content_digest,
        manifest_digest: metadata.identity.manifest_digest,
        provenance: metadata.provenance,
        installed_at: event.ts.to_string(),
        audit_id: event.id,
        audit_seq,
    }))
}

fn inconsistent() -> StoreError {
    StoreError::BadLedgerMeta("inactive package index/audit mismatch".into())
}

include!("package_outstanding_work.rs");

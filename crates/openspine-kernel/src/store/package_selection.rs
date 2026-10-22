//! Audited package selection (ANO-822). One Store-owned record, never a path.
//!
//! The selected installation, its generation and the authority-invalidation
//! generation change only in [`Store::commit_package_selection`]: one immediate
//! transaction that consumes the exact presented owner decision, applies the
//! reconfirmation consequences and appends the paired audit receipt.
use jiff::Timestamp;
use rusqlite::{params, Connection, OptionalExtension};
use ulid::Ulid;

use super::{Store, StoreError};
use crate::package::install_types::{crash_at, InstallReceipt};
use crate::package::selection_types::{
    SelectionCommitted, SelectionDecision, SelectionRecord, SelectionRefusal, SelectionSourceMode,
    MAX_DECISION_LIFETIME_SECS, MAX_RECONFIRMATIONS,
};

pub(super) const PRESENTED_KIND: &str = "package.selection_presented";
pub(super) const COMMITTED_KIND: &str = "package.selection_committed";

pub(super) fn ensure_schema(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS package_selection_decisions (
            decision_id TEXT PRIMARY KEY,
            expected_generation INTEGER NOT NULL CHECK (expected_generation >= 0),
            payload_json TEXT NOT NULL,
            presented_audit_seq INTEGER NOT NULL UNIQUE REFERENCES audit_log(seq),
            state TEXT NOT NULL CHECK (state IN ('presented','consumed'))
        );
        CREATE TABLE IF NOT EXISTS package_selection_history (
            generation INTEGER PRIMARY KEY CHECK (generation > 0),
            installation_id TEXT NOT NULL REFERENCES installed_packages(installation_id),
            invalidation_generation INTEGER NOT NULL
                CHECK (invalidation_generation >= 0 AND invalidation_generation <= generation),
            decision_id TEXT NOT NULL UNIQUE REFERENCES package_selection_decisions(decision_id),
            audit_seq INTEGER NOT NULL UNIQUE REFERENCES audit_log(seq)
        );
        CREATE TABLE IF NOT EXISTS package_selection (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            generation INTEGER NOT NULL REFERENCES package_selection_history(generation),
            installation_id TEXT NOT NULL,
            invalidation_generation INTEGER NOT NULL
        );",
    )?;
    Ok(())
}

pub(super) fn refuse(reason: SelectionRefusal) -> StoreError {
    StoreError::PackageSelection(reason)
}

fn to_i64(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::NumericRange)
}

fn to_u64(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::NumericRange)
}

pub(super) fn selection_conn(conn: &Connection) -> Result<Option<SelectionRecord>, StoreError> {
    let row: Option<(i64, String, i64)> = conn
        .query_row(
            "SELECT generation, installation_id, invalidation_generation FROM package_selection",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    row.map(|(generation, installation_id, invalidation)| {
        Ok(SelectionRecord {
            generation: to_u64(generation)?,
            invalidation_generation: to_u64(invalidation)?,
            installation_id: installation_id
                .parse()
                .map_err(|_| StoreError::BadUlid(installation_id))?,
        })
    })
    .transpose()
}

/// Managed mode is the existence of selection history; it is sticky.
pub(super) fn is_managed_conn(conn: &Connection) -> Result<bool, StoreError> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM package_selection_history)",
        [],
        |row| row.get(0),
    )?)
}

fn shape(decision: &SelectionDecision) -> Result<(), StoreError> {
    let lifetime = decision
        .expires_at
        .as_nanosecond()
        .checked_sub(decision.presented_at.as_nanosecond())
        .ok_or(refuse(SelectionRefusal::Malformed))?;
    let bounded = decision.schema_version == 1
        && !decision.owner_binding.is_empty()
        && decision.owner_binding.len() <= 128
        && !decision.principal.is_empty()
        && decision.principal.len() <= 128
        && decision.reconfirmations.len() <= MAX_RECONFIRMATIONS
        && decision
            .reconfirmations
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        && decision.reconfirmations.iter().all(|key| {
            key.version > 0
                && !key.kind.is_empty()
                && key.kind.len() <= 128
                && !key.artifact_id.is_empty()
                && key.artifact_id.len() <= 1024
        })
        && (decision.base_epoch_changed || decision.reconfirmations.is_empty())
        && (decision.source.mode == SelectionSourceMode::Managed)
            == decision.source.installation_id.is_some()
        && decision.source.identity.package_id == decision.target.identity.package_id
        && lifetime > 0
        && lifetime <= i128::from(MAX_DECISION_LIFETIME_SECS) * 1_000_000_000;
    if bounded {
        Ok(())
    } else {
        Err(refuse(SelectionRefusal::Malformed))
    }
}

pub(super) fn receipt_by_installation(
    conn: &Connection,
    installation_id: Ulid,
) -> Result<Option<InstallReceipt>, StoreError> {
    let key: Option<(String, u32)> = conn
        .query_row(
            "SELECT package_id, revision FROM installed_packages WHERE installation_id = ?1",
            [installation_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match key {
        Some((id, revision)) => super::package_install::load_receipt_by_key(conn, &id, revision),
        None => Ok(None),
    }
}

/// Compare the decision with live Store state: generation, source, target.
fn current_state(conn: &Connection, decision: &SelectionDecision) -> Result<(), StoreError> {
    let current = selection_conn(conn)?;
    // Managed history is sticky: a lost projection must never read as legacy.
    if current.is_none() && is_managed_conn(conn)? {
        return Err(StoreError::BadLedgerMeta(
            "package selection projection is missing".into(),
        ));
    }
    let generation = current.as_ref().map_or(0, |record| record.generation);
    if decision.expected_generation != generation {
        return Err(refuse(SelectionRefusal::GenerationChanged));
    }
    match &current {
        None => {
            if decision.source.mode != SelectionSourceMode::LegacyConfigured {
                return Err(refuse(SelectionRefusal::SourceChanged));
            }
        }
        Some(record) => {
            let source = receipt_by_installation(conn, record.installation_id)?
                .ok_or_else(|| refuse(SelectionRefusal::SourceChanged))?;
            if decision.source.installation_id != Some(record.installation_id)
                || decision.source.identity != source.identity()
            {
                return Err(refuse(SelectionRefusal::SourceChanged));
            }
            if decision.target.installation_id == record.installation_id {
                return Err(refuse(SelectionRefusal::Malformed));
            }
        }
    }
    let target = receipt_by_installation(conn, decision.target.installation_id)?
        .ok_or_else(|| refuse(SelectionRefusal::TargetUnavailable))?;
    if target.identity() != decision.target.identity {
        return Err(refuse(SelectionRefusal::TargetUnavailable));
    }
    Ok(())
}

fn append(conn: &Connection, kind: &str, id: Ulid, json: &str) -> Result<i64, StoreError> {
    let event = Store::append_audit_conn_with_options(
        conn,
        kind,
        None,
        None,
        Some("Owner-reviewed package selection; no standing authority is granted by it."),
        None,
        &[],
        &[],
        Some(&format!("package_selection:{id}")),
        Some(json),
    )?;
    Ok(conn.query_row(
        "SELECT seq FROM audit_log WHERE id = ?1",
        [event.id.to_string()],
        |row| row.get(0),
    )?)
}

impl Store {
    /// The committed selection projection, or `None` in legacy mode. Callers
    /// that start the runtime must have run `validate_package_selection`.
    pub(crate) fn package_selection(&self) -> Result<Option<SelectionRecord>, StoreError> {
        self.with_deferred_read(|tx| selection_conn(tx))
    }

    /// Persist the exact decision the owner is about to be asked to confirm,
    /// with its paired presentation receipt. The decision authorizes nothing
    /// until the single commit transaction consumes it.
    pub(crate) fn record_package_selection_decision(
        &self,
        decision: &SelectionDecision,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        self.with_immediate_tx(|tx| {
            shape(decision)?;
            if decision.presented_at > now {
                return Err(refuse(SelectionRefusal::Malformed));
            }
            if now >= decision.expires_at {
                return Err(refuse(SelectionRefusal::Expired));
            }
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM package_selection_decisions WHERE decision_id = ?1)",
                [decision.decision_id.to_string()],
                |row| row.get(0),
            )?;
            if exists {
                return Err(refuse(SelectionRefusal::Malformed));
            }
            current_state(tx, decision)?;
            let json = serde_json::to_string(decision)?;
            let seq = append(tx, PRESENTED_KIND, decision.decision_id, &json)?;
            tx.execute(
                "INSERT INTO package_selection_decisions
                 (decision_id, expected_generation, payload_json, presented_audit_seq, state)
                 VALUES (?1, ?2, ?3, ?4, 'presented')",
                params![
                    decision.decision_id.to_string(),
                    to_i64(decision.expected_generation)?,
                    json,
                    seq
                ],
            )?;
            Ok(())
        })
    }

    /// Compare-and-swap the selection generation, consume the exact presented
    /// decision, apply the reconfirmation consequences and append the receipt.
    /// Every effect commits together or none does.
    pub(crate) fn commit_package_selection(
        &self,
        confirmed: &SelectionDecision,
        now: Timestamp,
    ) -> Result<SelectionRecord, StoreError> {
        self.with_immediate_tx(|tx| {
            shape(confirmed)?;
            let row: Option<(String, String)> = tx
                .query_row(
                    "SELECT payload_json, state FROM package_selection_decisions WHERE decision_id = ?1",
                    [confirmed.decision_id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let (payload, state) = row.ok_or_else(|| refuse(SelectionRefusal::UnknownDecision))?;
            if state != "presented" {
                return Err(refuse(SelectionRefusal::AlreadyConsumed));
            }
            if serde_json::from_str::<SelectionDecision>(&payload)? != *confirmed {
                return Err(refuse(SelectionRefusal::Mismatch));
            }
            if now < confirmed.presented_at || now >= confirmed.expires_at {
                return Err(refuse(SelectionRefusal::Expired));
            }
            current_state(tx, confirmed)?;
            let prior = selection_conn(tx)?;
            let generation = confirmed.expected_generation + 1;
            let invalidation = prior
                .as_ref()
                .map_or(0, |record| record.invalidation_generation)
                + u64::from(confirmed.base_epoch_changed);
            for key in &confirmed.reconfirmations {
                let changed = tx.execute(
                    "UPDATE learned_artifacts SET compatibility = 'reconfirmation_required'
                     WHERE kind = ?1 AND artifact_id = ?2 AND version = ?3
                       AND compatibility IN ('compatible', 'owner_accepted')",
                    params![key.kind, key.artifact_id, key.version],
                )?;
                if changed != 1 {
                    return Err(refuse(SelectionRefusal::ConsequenceStale));
                }
            }
            // Runtime standing rules are the live reusable authority and are not
            // gated by registry membership, so a base-epoch change must also
            // send every active one back to review (pending dark-window
            // exceptions staled with it). Paused and revoked rules keep their
            // more restrictive owner state.
            let mut rules_needing_review = Vec::new();
            if confirmed.base_epoch_changed {
                let now_nanos = i64::try_from(now.as_nanosecond())
                    .map_err(|_| StoreError::NumericRange)?;
                rules_needing_review = tx
                    .prepare("SELECT rule_id FROM standing_rules WHERE status = 'active' ORDER BY rule_id")?
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                for rule_id in &rules_needing_review {
                    super::standing_rules_exceptions::stale_pending_exceptions_in_tx(
                        tx, rule_id, None, now_nanos,
                    )?;
                    tx.execute(
                        "UPDATE standing_rules SET status = 'needs_review', needs_review_since = ?2
                         WHERE rule_id = ?1 AND status = 'active'",
                        params![rule_id, now_nanos],
                    )?;
                }
            }
            let committed = SelectionCommitted {
                generation,
                invalidation_generation: invalidation,
                decision: confirmed.clone(),
                rules_needing_review,
            };
            let seq = append(
                tx,
                COMMITTED_KIND,
                confirmed.decision_id,
                &serde_json::to_string(&committed)?,
            )?;
            crash_at("selection-after-audit");
            if tx.execute(
                "UPDATE package_selection_decisions SET state = 'consumed'
                 WHERE decision_id = ?1 AND state = 'presented'",
                [confirmed.decision_id.to_string()],
            )? != 1
            {
                return Err(refuse(SelectionRefusal::AlreadyConsumed));
            }
            let installation = confirmed.target.installation_id.to_string();
            tx.execute(
                "INSERT INTO package_selection_history
                 (generation, installation_id, invalidation_generation, decision_id, audit_seq)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    to_i64(generation)?,
                    installation,
                    to_i64(invalidation)?,
                    confirmed.decision_id.to_string(),
                    seq
                ],
            )?;
            tx.execute(
                "INSERT INTO package_selection (singleton, generation, installation_id, invalidation_generation)
                 VALUES (1, ?1, ?2, ?3)
                 ON CONFLICT(singleton) DO UPDATE SET generation = excluded.generation,
                   installation_id = excluded.installation_id,
                   invalidation_generation = excluded.invalidation_generation",
                params![to_i64(generation)?, installation, to_i64(invalidation)?],
            )?;
            crash_at("selection-before-commit");
            Ok(SelectionRecord {
                generation,
                invalidation_generation: invalidation,
                installation_id: confirmed.target.installation_id,
            })
        })
    }
}

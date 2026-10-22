//! Replay of the selection projection against its authenticated audit receipts.
//!
//! The hash-chained `meta_json` is the authority: the history, decision and
//! selection tables are projections that must match it exactly. Managed mode is
//! sticky, so a missing or altered selection is an error, never legacy mode.
use rusqlite::{Connection, OptionalExtension};
use ulid::Ulid;

use super::package_selection::{
    receipt_by_installation, selection_conn, COMMITTED_KIND, PRESENTED_KIND,
};
use super::{Store, StoreError};
use crate::package::selection_types::{SelectionCommitted, SelectionDecision, SelectionSourceMode};

fn inconsistent() -> StoreError {
    StoreError::BadLedgerMeta("package selection ledger mismatch".into())
}

/// The payload bound by the audit hash for `seq`, requiring its exact kind.
fn authenticated_payload(conn: &Connection, seq: i64, kind: &str) -> Result<String, StoreError> {
    let (row_kind, meta): (String, String) = conn
        .query_row(
            "SELECT kind, meta_json FROM audit_log WHERE seq = ?1",
            [seq],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or_else(inconsistent)?;
    let meta: serde_json::Value = serde_json::from_str(&meta)?;
    if row_kind != kind || meta["kind"].as_str() != Some(kind) {
        return Err(inconsistent());
    }
    meta["payload_json"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(inconsistent)
}

fn count(conn: &Connection, sql: &str) -> Result<i64, StoreError> {
    Ok(conn.query_row(sql, [], |row| row.get(0))?)
}

impl Store {
    /// Validate every selection projection. The caller already verified the
    /// audit chain; this binds the projection to those authenticated rows.
    pub(crate) fn validate_package_selection(&self) -> Result<(), StoreError> {
        self.with_deferred_read(|tx| {
            let presented = count(
                tx,
                &format!("SELECT COUNT(*) FROM audit_log WHERE kind = '{PRESENTED_KIND}'"),
            )?;
            let committed = count(
                tx,
                &format!("SELECT COUNT(*) FROM audit_log WHERE kind = '{COMMITTED_KIND}'"),
            )?;
            let decisions = count(tx, "SELECT COUNT(*) FROM package_selection_decisions")?;
            let consumed = count(
                tx,
                "SELECT COUNT(*) FROM package_selection_decisions WHERE state = 'consumed'",
            )?;
            let history = count(tx, "SELECT COUNT(*) FROM package_selection_history")?;
            if presented != decisions || committed != history || consumed != history {
                return Err(inconsistent());
            }
            let mut statement = tx.prepare(
                "SELECT decision_id, expected_generation, payload_json, presented_audit_seq, state
                 FROM package_selection_decisions ORDER BY presented_audit_seq",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut stored = std::collections::BTreeMap::new();
            for (id, expected, payload, seq, state) in rows {
                let decision: SelectionDecision = serde_json::from_str(&payload)?;
                let audited: SelectionDecision =
                    serde_json::from_str(&authenticated_payload(tx, seq, PRESENTED_KIND)?)?;
                if decision != audited
                    || decision.decision_id.to_string() != id
                    || i64::try_from(decision.expected_generation).ok() != Some(expected)
                {
                    return Err(inconsistent());
                }
                stored.insert(id, (decision, state));
            }
            let mut statement = tx.prepare(
                "SELECT generation, installation_id, invalidation_generation, decision_id, audit_seq
                 FROM package_selection_history ORDER BY generation",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut previous: Option<(Ulid, u64)> = None;
            for (position, (generation, installation, invalidation, decision_id, seq)) in
                rows.iter().enumerate()
            {
                let audited: SelectionCommitted =
                    serde_json::from_str(&authenticated_payload(tx, *seq, COMMITTED_KIND)?)?;
                let expected_generation = u64::try_from(position).map_err(|_| inconsistent())? + 1;
                let prior_invalidation = previous.map_or(0, |(_, value)| value);
                let (decision, state) = stored.get(decision_id).ok_or_else(inconsistent)?;
                let managed_source = match previous {
                    None => decision.source.mode == SelectionSourceMode::LegacyConfigured,
                    Some((prior, _)) => {
                        decision.source.mode == SelectionSourceMode::Managed
                            && decision.source.installation_id == Some(prior)
                    }
                };
                if i64::try_from(audited.generation).ok() != Some(*generation)
                    || audited.generation != expected_generation
                    || decision.expected_generation + 1 != expected_generation
                    || audited.decision != *decision
                    || state != "consumed"
                    || audited.decision.target.installation_id.to_string() != *installation
                    || i64::try_from(audited.invalidation_generation).ok() != Some(*invalidation)
                    || audited.invalidation_generation
                        != prior_invalidation + u64::from(decision.base_epoch_changed)
                    || !managed_source
                {
                    return Err(inconsistent());
                }
                let receipt = receipt_by_installation(tx, decision.target.installation_id)?
                    .ok_or_else(inconsistent)?;
                if receipt.identity() != decision.target.identity {
                    return Err(inconsistent());
                }
                previous = Some((
                    decision.target.installation_id,
                    audited.invalidation_generation,
                ));
            }
            let projection = selection_conn(tx)?;
            match (rows.last(), projection) {
                (None, None) => Ok(()),
                (Some((generation, installation, invalidation, _, _)), Some(record))
                    if i64::try_from(record.generation).ok() == Some(*generation)
                        && record.installation_id.to_string() == *installation
                        && i64::try_from(record.invalidation_generation).ok()
                            == Some(*invalidation) =>
                {
                    Ok(())
                }
                _ => Err(inconsistent()),
            }
        })
    }
}

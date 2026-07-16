//! Eval-verdict / fitness store (agent-OS design log, non-retrofittable set;
//! AD-111 is *leaning* and cited here only for verdict landing;
//! change `define-lineage-and-eval-store`).
//!
//! Verdicts land in this dedicated indexed table rather than audit-chain rows.
//! Concrete evaluator policy and vocabulary are deferred to the later
//! evaluation change. Evaluator and evidence fields are forward-compatible
//! metadata only; they never confer authority (D-006).

use jiff::Timestamp;
use rusqlite::{params, OptionalExtension};
use std::convert::TryFrom;
use ulid::Ulid;

use super::{Store, StoreError};

/// One eval-verdict row.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub struct EvalVerdict {
    pub id: Ulid,
    pub artifact_kind: String,
    pub artifact_id: String,
    pub artifact_version: u32,
    /// Open-vocabulary verdict label; concrete policy remains deferred.
    pub verdict: String,
    pub fitness: Option<f64>,
    /// Optional forward-compatible supporting-evidence reference.
    pub evidence: Option<String>,
    /// Optional evaluator identity metadata; never authority (D-006).
    pub evaluator: Option<String>,
    /// Digest of evaluated bytes (digest-bound, D-011).
    pub artifact_digest: String,
    pub recorded_at: Timestamp,
}

type EvalRow = (
    String,
    String,
    String,
    i64,
    String,
    Option<f64>,
    Option<String>,
    Option<String>,
    String,
    i64,
);

pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS eval_verdicts (\n\
         \x20   id TEXT PRIMARY KEY,\n\
         \x20   artifact_kind TEXT NOT NULL,\n\
         \x20   artifact_id TEXT NOT NULL,\n\
         \x20   artifact_version INTEGER NOT NULL,\n\
         \x20   verdict TEXT NOT NULL,\n\
         \x20   fitness REAL,\n\
         \x20   evidence TEXT,\n\
         \x20   evaluator TEXT,
         \x20   artifact_digest TEXT NOT NULL,\n\
         \x20   recorded_at INTEGER NOT NULL\n\
         );\n\
         CREATE INDEX IF NOT EXISTS idx_eval_verdicts_artifact\n\
         \x20   ON eval_verdicts (artifact_kind, artifact_id, artifact_version, recorded_at);\n\
         CREATE INDEX IF NOT EXISTS idx_eval_verdicts_verdict\n\
         \x20   ON eval_verdicts (verdict);",
    )?;
    Ok(())
}

pub(super) fn timestamp_to_epoch_nanos(timestamp: Timestamp) -> Result<i64, StoreError> {
    i64::try_from(timestamp.as_nanosecond()).map_err(|_| {
        StoreError::TimestampRange(format!(
            "epoch nanoseconds {} do not fit SQLite INTEGER",
            timestamp.as_nanosecond()
        ))
    })
}

fn epoch_nanos_to_timestamp(nanos: i64) -> Result<Timestamp, StoreError> {
    Timestamp::from_nanosecond(i128::from(nanos)).map_err(|err| {
        StoreError::TimestampRange(format!("invalid epoch nanoseconds {nanos}: {err}"))
    })
}

fn map_row(row: EvalRow) -> Result<EvalVerdict, StoreError> {
    let (id, kind, aid, version, verdict, fitness, evidence, evaluator, digest, recorded_at) = row;
    Ok(EvalVerdict {
        id: Ulid::from_string(&id).map_err(|_| StoreError::BadDigest("eval_verdicts.id".into()))?,
        artifact_kind: kind,
        artifact_id: aid,
        artifact_version: version as u32,
        verdict,
        fitness,
        evidence,
        evaluator,
        artifact_digest: digest,
        recorded_at: epoch_nanos_to_timestamp(recorded_at)?,
    })
}

const SELECT_COLS: &str = "id, artifact_kind, artifact_id, artifact_version, verdict, \
     fitness, evidence, evaluator, artifact_digest, recorded_at";

fn read_eval_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EvalRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
    ))
}

/// Insert one eval verdict row against a caller-provided connection. Shared
/// between `Store::insert_eval_verdict` (locked own connection) and callers
/// that must write the row inside their own transaction, so both paths use
/// one column contract.
pub(super) fn insert_eval_verdict_conn(
    conn: &rusqlite::Connection,
    row: &EvalVerdict,
) -> Result<(), StoreError> {
    let recorded_at = timestamp_to_epoch_nanos(row.recorded_at)?;
    conn.execute(
        "INSERT INTO eval_verdicts \
         (id, artifact_kind, artifact_id, artifact_version, verdict, \
          fitness, evidence, evaluator, artifact_digest, recorded_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            row.id.to_string(),
            row.artifact_kind,
            row.artifact_id,
            row.artifact_version as i64,
            row.verdict,
            row.fitness,
            row.evidence,
            row.evaluator,
            row.artifact_digest,
            recorded_at,
        ],
    )?;
    Ok(())
}

#[allow(dead_code)]
impl Store {
    pub fn insert_eval_verdict(&self, row: &EvalVerdict) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        insert_eval_verdict_conn(&conn, row)
    }

    pub fn eval_verdicts_for_artifact(
        &self,
        artifact_kind: &str,
        artifact_id: &str,
        artifact_version: u32,
    ) -> Result<Vec<EvalVerdict>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SELECT_COLS} FROM eval_verdicts \
             WHERE artifact_kind = ?1 AND artifact_id = ?2 AND artifact_version = ?3 \
             ORDER BY recorded_at ASC"
        ))?;
        let rows = stmt.query_map(
            params![artifact_kind, artifact_id, artifact_version as i64],
            read_eval_row,
        )?;
        rows.map(|r| map_row(r?).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e))))
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    pub fn eval_verdicts_by_verdict(&self, verdict: &str) -> Result<Vec<EvalVerdict>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SELECT_COLS} FROM eval_verdicts WHERE verdict = ?1 ORDER BY recorded_at ASC"
        ))?;
        let rows = stmt.query_map(params![verdict], read_eval_row)?;
        rows.map(|r| map_row(r?).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e))))
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    pub fn latest_eval_verdict(
        &self,
        artifact_kind: &str,
        artifact_id: &str,
        artifact_version: u32,
    ) -> Result<Option<EvalVerdict>, StoreError> {
        let conn = self.conn.lock();
        let row: Option<EvalRow> = conn
            .query_row(
                &format!(
                    "SELECT {SELECT_COLS} FROM eval_verdicts \
                 WHERE artifact_kind = ?1 AND artifact_id = ?2 AND artifact_version = ?3 \
                 ORDER BY recorded_at DESC LIMIT 1"
                ),
                params![artifact_kind, artifact_id, artifact_version as i64],
                read_eval_row,
            )
            .optional()?;
        row.map(map_row).transpose()
    }
}

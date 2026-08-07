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

/// The epochs a verdict was computed under. All fields optional: a
/// proposal kind that has no reviewed scope simply records `None`.
///
/// Recording the epochs is what makes staleness a *read-time* question: a
/// verdict never has to be swept or rewritten, because the reader compares
/// what the verdict bound itself to against what is live now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub struct VerdictEpochs {
    pub proposal_digest: Option<String>,
    pub compatibility_digest: Option<String>,
    pub reviewed_scope_digest: Option<String>,
    pub evidence_set_digest: Option<String>,
    pub descriptor_version: Option<u32>,
    pub implementation_version: Option<u32>,
    pub policy_version: Option<u32>,
}

/// A recorded `None` binds nothing on that axis, so it is never compared.
/// A recorded `Some` is stale unless the live value is the identical `Some`
/// — including when the live value is `None`, i.e. the axis disappeared.
fn digest_axis_stale(recorded: &Option<String>, live: &Option<String>) -> bool {
    match recorded {
        None => false,
        Some(recorded) => live.as_deref() != Some(recorded.as_str()),
    }
}

fn version_axis_stale(recorded: Option<u32>, live: Option<u32>) -> bool {
    match recorded {
        None => false,
        Some(recorded) => live != Some(recorded),
    }
}

#[allow(dead_code)]
impl VerdictEpochs {
    /// Every axis paired with whether it is stale, in a fixed audit order.
    /// Returned by value on the stack so the currency check never allocates.
    fn axis_staleness(&self, live: &Self) -> [(&'static str, bool); 7] {
        [
            (
                "proposal_digest",
                digest_axis_stale(&self.proposal_digest, &live.proposal_digest),
            ),
            (
                "compatibility_digest",
                digest_axis_stale(&self.compatibility_digest, &live.compatibility_digest),
            ),
            (
                "reviewed_scope_digest",
                digest_axis_stale(&self.reviewed_scope_digest, &live.reviewed_scope_digest),
            ),
            (
                "evidence_set_digest",
                digest_axis_stale(&self.evidence_set_digest, &live.evidence_set_digest),
            ),
            (
                "descriptor_version",
                version_axis_stale(self.descriptor_version, live.descriptor_version),
            ),
            (
                "implementation_version",
                version_axis_stale(self.implementation_version, live.implementation_version),
            ),
            (
                "policy_version",
                version_axis_stale(self.policy_version, live.policy_version),
            ),
        ]
    }

    /// Read-time currency: every epoch this verdict RECORDED must still
    /// equal the corresponding live value. A recorded `None` is not
    /// compared (nothing was bound on that axis). A recorded `Some` whose
    /// live counterpart is `None` is STALE (the axis disappeared).
    pub(crate) fn is_current_against(&self, live: &Self) -> bool {
        self.axis_staleness(live).iter().all(|(_, stale)| !stale)
    }

    /// The names of the axes that no longer match, for audit/denial text.
    pub(crate) fn stale_axes(&self, live: &Self) -> Vec<&'static str> {
        self.axis_staleness(live)
            .into_iter()
            .filter_map(|(name, stale)| stale.then_some(name))
            .collect()
    }
}

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
    /// What this verdict was computed under; staleness is derived from it
    /// at read time rather than stamped onto the row by a sweeper.
    pub epochs: VerdictEpochs,
}

type EvalCore = (
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

type EvalRow = (EvalCore, VerdictEpochs);

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
    let (
        (id, kind, aid, version, verdict, fitness, evidence, evaluator, digest, recorded_at),
        epochs,
    ) = row;
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
        epochs,
    })
}

const SELECT_COLS: &str = "id, artifact_kind, artifact_id, artifact_version, verdict, \
     fitness, evidence, evaluator, artifact_digest, recorded_at, \
     proposal_digest, compatibility_digest, reviewed_scope_digest, evidence_set_digest, \
     descriptor_version, implementation_version, policy_version";

fn read_u32_col(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Option<u32>> {
    Ok(row.get::<_, Option<i64>>(idx)?.map(|v| v as u32))
}

fn read_eval_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EvalRow> {
    Ok((
        (
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
        ),
        VerdictEpochs {
            proposal_digest: row.get(10)?,
            compatibility_digest: row.get(11)?,
            reviewed_scope_digest: row.get(12)?,
            evidence_set_digest: row.get(13)?,
            descriptor_version: read_u32_col(row, 14)?,
            implementation_version: read_u32_col(row, 15)?,
            policy_version: read_u32_col(row, 16)?,
        },
    ))
}

/// Insert one eval verdict row against a caller-provided connection. Shared
/// between `Store::insert_eval_verdict` (locked own connection) and callers
/// that must write the row inside their own transaction, so both paths use
/// one column contract.
pub(crate) fn insert_eval_verdict_conn(
    conn: &rusqlite::Connection,
    row: &EvalVerdict,
) -> Result<(), StoreError> {
    let recorded_at = timestamp_to_epoch_nanos(row.recorded_at)?;
    let epochs = &row.epochs;
    conn.execute(
        "INSERT INTO eval_verdicts \
         (id, artifact_kind, artifact_id, artifact_version, verdict, \
          fitness, evidence, evaluator, artifact_digest, recorded_at, \
          proposal_digest, compatibility_digest, reviewed_scope_digest, \
          evidence_set_digest, descriptor_version, implementation_version, policy_version) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
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
            epochs.proposal_digest,
            epochs.compatibility_digest,
            epochs.reviewed_scope_digest,
            epochs.evidence_set_digest,
            epochs.descriptor_version.map(i64::from),
            epochs.implementation_version.map(i64::from),
            epochs.policy_version.map(i64::from),
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

// openspine:allow-large-module reason: activation transaction remains co-located with proposal lifecycle storage
//! Storage for the `proposed_artifacts` table (5b): one row per
//! `artifact.propose` dispatch, tracking the proposal through the PRD §13.1
//! lifecycle (`proposed → validated → review_required → approved → active`)
//! until a digest-bound `artifact.activate` approval activates it. Split out
//! of `store/mod.rs` to keep that file under the 500-line gate, mirroring
//! `budget_support`/`gate_support`; the table's own `CREATE TABLE` lives
//! here too (rather than in mod.rs's `SCHEMA_SQL`) for the same reason.
//!
//! The `yaml_digest` column is the content-addressed ref into the encrypted
//! artifact store (`ArtifactRef.digest`) holding the raw YAML the owner
//! reviewed — the approval binds exactly those bytes (D-011), never a value
//! re-supplied by the shell at activation time.

use crate::overlay_eval_gate::{JudgePassed, ReplayPassed};
use jiff::Timestamp;
use openspine_schemas::artifact::{can_transition, Lifecycle};
use openspine_schemas::lineage::ArtifactLineage;
use rusqlite::{params, OptionalExtension};
use ulid::Ulid;

use super::{Store, StoreError};

/// One proposed-artifact row. `state` mirrors the artifact lifecycle
/// (`proposed`/`validated`/`review_required`/`approved`/`active`); only the
/// `review_required` and `approved` states are observable between dispatch
/// and activation in the normal flow.
#[derive(Debug, Clone)]
pub struct ProposedArtifact {
    pub id: Ulid,
    pub kind: String,
    pub artifact_id: String,
    pub version: u32,
    pub state: Lifecycle,
    /// `sha256:<hex>` — the artifact-store ref to the reviewed raw YAML.
    pub yaml_digest: String,
    pub task_grant_id: Ulid,
    /// `None` until the approval `ActionRequest` is persisted; the approval
    /// handler joins back through this column.
    pub action_request_id: Option<Ulid>,
    pub proposed_at: Timestamp,
    /// Generation/lineage of this artifact (distinct from `version`).
    /// `Some(root())` for freshly proposed root artifacts; `Some(derived)`
    /// when parents are known. `None` means provenance is *unknown* —
    /// pre-lineage legacy rows after migration — and MUST NOT be silently
    /// rewritten as root (unknown ≠ generation-0).
    pub lineage: Option<ArtifactLineage>,
}

/// Raw column tuple for one `proposed_artifacts` row, in `SELECT` order:
/// `(id, kind, artifact_id, version, state, yaml_digest, task_grant_id,
/// action_request_id, proposed_at, lineage_json)`.
type ProposedRow = (
    String,
    String,
    String,
    i64,
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
);

/// Serialise a lifecycle state to its on-disk column form (the serde
/// `snake_case` rename), reusing the schema's own naming rather than a
/// second mapping that could drift.
fn lifecycle_name(state: Lifecycle) -> String {
    serde_json::to_value(state)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

fn parse_lifecycle(s: &str) -> Result<Lifecycle, StoreError> {
    serde_json::from_value::<Lifecycle>(serde_json::Value::String(s.to_string()))
        .map_err(|_| StoreError::ProposedArtifactLifecycle(format!("unparseable state {s}")))
}

/// Create the table if absent. Idempotent — safe against both a fresh file
/// and an existing `data/kernel.db` predating this slice. The `lineage_json`
/// column is nullable: `NULL` means provenance is unknown (legacy rows),
/// never silently rewritten as root. New inserts supply an explicit value.
/// An ad-hoc migration in `migrations.rs` adds the column for databases
/// that already have the table without it.
pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS proposed_artifacts (\n\
         \x20   id TEXT PRIMARY KEY,\n\
         \x20   kind TEXT NOT NULL,\n\
         \x20   artifact_id TEXT NOT NULL,\n\
         \x20   version INTEGER NOT NULL,\n\
         \x20   state TEXT NOT NULL,\n\
         \x20   yaml_digest TEXT NOT NULL,\n\
         \x20   task_grant_id TEXT NOT NULL,\n\
         \x20   action_request_id TEXT,\n\
         \x20   proposed_at TEXT NOT NULL,\n\
         \x20   lineage_json TEXT,\n\
         \x20   UNIQUE(kind, artifact_id, version)\n\
         );",
    )?;
    Ok(())
}

fn lineage_to_json(lineage: &Option<ArtifactLineage>) -> Result<Option<String>, StoreError> {
    match lineage {
        Some(l) if !l.is_consistent() => Err(StoreError::InconsistentLineage(
            "generation does not agree with parent presence".into(),
        )),
        Some(l) => Ok(Some(serde_json::to_string(l)?)),
        None => Ok(None),
    }
}

fn lineage_from_json(s: Option<&str>) -> Result<Option<ArtifactLineage>, StoreError> {
    match s {
        None => Ok(None),
        Some(raw) => {
            let lineage: ArtifactLineage = serde_json::from_str(raw).map_err(|err| {
                StoreError::InconsistentLineage(format!("unparseable lineage_json: {err}"))
            })?;
            if !lineage.is_consistent() {
                return Err(StoreError::InconsistentLineage(
                    "stored generation does not agree with parent presence".into(),
                ));
            }
            Ok(Some(lineage))
        }
    }
}

impl Store {
    pub fn find_proposed_artifact_state(
        &self,
        kind: &str,
        artifact_id: &str,
        version: u32,
    ) -> Result<Option<(Lifecycle, String)>, StoreError> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT state, yaml_digest FROM proposed_artifacts
             WHERE kind = ?1 AND artifact_id = ?2 AND version = ?3
             ORDER BY proposed_at DESC LIMIT 1",
            params![kind, artifact_id, version as i64],
            |row| {
                let state: String = row.get(0)?;
                let digest: String = row.get(1)?;
                Ok((state, digest))
            },
        )
        .optional()?
        .map(|(state, digest)| Ok((parse_lifecycle(&state)?, digest)))
        .transpose()
    }
}

impl Store {
    pub fn active_model_swap_ids(&self) -> Result<Vec<(String, u32)>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT artifact_id, MAX(version) FROM proposed_artifacts
             WHERE kind = 'model_swap' AND state = ?1
             GROUP BY artifact_id",
        )?;
        let rows = stmt.query_map(params![lifecycle_name(Lifecycle::Active)], |row| {
            Ok((row.get(0)?, row.get::<_, i64>(1)? as u32))
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }
}

impl Store {
    pub fn insert_proposed_artifact(&self, row: &ProposedArtifact) -> Result<(), StoreError> {
        if row.state != Lifecycle::Proposed {
            return Err(StoreError::ProposedArtifactLifecycle(
                "new proposals must enter storage in proposed state".to_string(),
            ));
        }
        let conn = self.conn.lock();
        let lineage_json = lineage_to_json(&row.lineage)?;
        conn.execute(
            "INSERT INTO proposed_artifacts \
             (id, kind, artifact_id, version, state, yaml_digest, task_grant_id, \
              action_request_id, proposed_at, lineage_json) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                row.id.to_string(),
                row.kind,
                row.artifact_id,
                row.version as i64,
                lifecycle_name(row.state),
                row.yaml_digest,
                row.task_grant_id.to_string(),
                row.action_request_id.map(|u| u.to_string()),
                row.proposed_at.to_string(),
                lineage_json,
            ],
        )?;
        Ok(())
    }

    pub fn proposed_artifact_exists(
        &self,
        kind: &str,
        artifact_id: &str,
        version: u32,
    ) -> Result<bool, StoreError> {
        let conn = self.conn.lock();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proposed_artifacts \
             WHERE kind = ?1 AND artifact_id = ?2 AND version = ?3",
            params![kind, artifact_id, version as i64],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }
    pub fn highest_proposed_version(
        &self,
        kind: &str,
        artifact_id: &str,
    ) -> Result<Option<u32>, StoreError> {
        let conn = self.conn.lock();
        let value: Option<i64> = conn.query_row(
            "SELECT MAX(version) FROM proposed_artifacts WHERE kind = ?1 AND artifact_id = ?2",
            params![kind, artifact_id],
            |row| row.get::<_, Option<i64>>(0),
        )?;
        Ok(value.map(|version| version as u32))
    }

    pub fn find_proposed_artifact_by_action_request(
        &self,
        action_request_id: Ulid,
    ) -> Result<Option<ProposedArtifact>, StoreError> {
        let conn = self.conn.lock();
        let row: Option<ProposedRow> = conn
            .query_row(
                "SELECT id, kind, artifact_id, version, state, yaml_digest, task_grant_id, \
                 action_request_id, proposed_at, lineage_json \
                 FROM proposed_artifacts WHERE action_request_id = ?1",
                params![action_request_id.to_string()],
                |row| {
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
                },
            )
            .optional()?;
        let Some((
            id,
            kind,
            artifact_id,
            version,
            state,
            yaml_digest,
            task_grant_id,
            action_request_id,
            proposed_at,
            lineage_json,
        )) = row
        else {
            return Ok(None);
        };
        Ok(Some(ProposedArtifact {
            id: Ulid::from_string(&id)
                .map_err(|_| StoreError::BadDigest("proposed_artifacts.id".into()))?,
            kind,
            artifact_id,
            version: version as u32,
            state: parse_lifecycle(&state)?,
            yaml_digest,
            task_grant_id: Ulid::from_string(&task_grant_id)
                .map_err(|_| StoreError::BadDigest("proposed_artifacts.task_grant_id".into()))?,
            action_request_id: action_request_id
                .as_deref()
                .map(Ulid::from_string)
                .transpose()
                .map_err(|_| {
                    StoreError::BadDigest("proposed_artifacts.action_request_id".into())
                })?,
            proposed_at: proposed_at
                .parse()
                .map_err(|_| StoreError::BadDigest("proposed_artifacts.proposed_at".into()))?,
            lineage: lineage_from_json(lineage_json.as_deref())?,
        }))
    }
    /// Load the proposed row for a `(kind, id, version)` identity, if one
    /// exists. Used to advance a dangling-initial-activation proposal to
    /// `Active` when the owner re-confirms it (lifecycle truth must match
    /// effective authority — AD-070).
    pub fn find_proposed_artifact(
        &self,
        kind: &str,
        artifact_id: &str,
        version: u32,
    ) -> Result<Option<ProposedArtifact>, StoreError> {
        let conn = self.conn.lock();
        let row: Option<ProposedRow> = conn
            .query_row(
                "SELECT id, kind, artifact_id, version, state, yaml_digest, task_grant_id, \
                 action_request_id, proposed_at, lineage_json \
                 FROM proposed_artifacts \
                 WHERE kind = ?1 AND artifact_id = ?2 AND version = ?3 \
                 ORDER BY proposed_at DESC LIMIT 1",
                params![kind, artifact_id, version as i64],
                |row| {
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
                },
            )
            .optional()?;
        let Some((
            id,
            kind,
            artifact_id,
            version,
            state,
            yaml_digest,
            task_grant_id,
            action_request_id,
            proposed_at,
            lineage_json,
        )) = row
        else {
            return Ok(None);
        };
        Ok(Some(ProposedArtifact {
            id: Ulid::from_string(&id)
                .map_err(|_| StoreError::BadDigest("proposed_artifacts.id".into()))?,
            kind,
            artifact_id,
            version: version as u32,
            state: parse_lifecycle(&state)?,
            yaml_digest,
            task_grant_id: Ulid::from_string(&task_grant_id)
                .map_err(|_| StoreError::BadDigest("proposed_artifacts.task_grant_id".into()))?,
            action_request_id: action_request_id
                .as_deref()
                .map(Ulid::from_string)
                .transpose()
                .map_err(|_| {
                    StoreError::BadDigest("proposed_artifacts.action_request_id".into())
                })?,
            proposed_at: proposed_at
                .parse()
                .map_err(|_| StoreError::BadDigest("proposed_artifacts.proposed_at".into()))?,
            lineage: lineage_from_json(lineage_json.as_deref())?,
        }))
    }
    /// Whether a committed proposal lifecycle is Active for this exact
    /// artifact version. Startup recovery uses this as the durable publication
    /// gate, never learned compatibility alone.
    pub fn is_active_proposal(
        &self,
        kind: &str,
        artifact_id: &str,
        version: u32,
    ) -> Result<bool, StoreError> {
        let conn = self.conn.lock();
        let active: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM proposed_artifacts \
                 WHERE kind = ?1 AND artifact_id = ?2 AND version = ?3 AND state = 'active' \
                 LIMIT 1",
                params![kind, artifact_id, version as i64],
                |row| row.get(0),
            )
            .optional()?;
        Ok(active.is_some())
    }

    /// Highest Active proposal version for an identity, if any.
    pub fn highest_active_version(
        &self,
        kind: &str,
        artifact_id: &str,
    ) -> Result<Option<u32>, StoreError> {
        let conn = self.conn.lock();
        let version: Option<i64> = conn.query_row(
            "SELECT MAX(version) FROM proposed_artifacts \
             WHERE kind = ?1 AND artifact_id = ?2 AND state = 'active'",
            params![kind, artifact_id],
            |row| row.get(0),
        )?;
        Ok(version.map(|v| v as u32))
    }

    /// Advance a proposal's state one legal step. `can_transition` is
    /// enforced *before* the UPDATE (PRD §13.2 — the proposer can never
    /// skip a stage), and the UPDATE's own `WHERE state = <from>` clause is
    /// a second guard against a concurrent modification racing the check.
    pub fn set_proposed_artifact_state(
        &self,
        id: Ulid,
        from: Lifecycle,
        to: Lifecycle,
    ) -> Result<(), StoreError> {
        if !can_transition(from, to) {
            return Err(StoreError::ProposedArtifactLifecycle(format!(
                "illegal transition {} -> {}",
                lifecycle_name(from),
                lifecycle_name(to)
            )));
        }
        if from == Lifecycle::Validated && to == Lifecycle::ReviewRequired {
            return Err(StoreError::ProposedArtifactLifecycle(
                "validated -> review_required requires the AD-142 replay and risk-judge gate"
                    .to_string(),
            ));
        }
        let conn = self.conn.lock();
        let rows = conn.execute(
            "UPDATE proposed_artifacts SET state = ?1 WHERE id = ?2 AND state = ?3",
            params![lifecycle_name(to), id.to_string(), lifecycle_name(from)],
        )?;
        if rows == 0 {
            return Err(StoreError::ProposedArtifactLifecycle(format!(
                "proposed artifact {id} was not in the expected {from_state} state",
                from_state = lifecycle_name(from)
            )));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn force_proposed_artifact_state_for_test(
        &self,
        id: Ulid,
        state: Lifecycle,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE proposed_artifacts SET state = ?1 WHERE id = ?2",
            params![lifecycle_name(state), id.to_string()],
        )?;
        Ok(())
    }
    /// Atomically persist the two passing eval verdicts and promote exactly
    /// this stored proposal to `review_required`. No caller-supplied artifact
    /// identity is trusted: the row is loaded inside the transaction and
    /// both opaque proofs must match its stored YAML digest.
    pub fn promote_authority_bearing_proposal(
        &self,
        proposal_id: Ulid,
        replay: ReplayPassed,
        judge: JudgePassed,
    ) -> Result<(), StoreError> {
        if replay.artifact_digest() != judge.artifact_digest() {
            return Err(StoreError::ProposedArtifactLifecycle(
                "replay and judge proofs have different artifact digests".to_string(),
            ));
        }
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let row: (String, String, i64, String, String) = tx.query_row(
            "SELECT kind, artifact_id, version, state, yaml_digest
             FROM proposed_artifacts WHERE id = ?1",
            params![proposal_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
        let (kind, artifact_id, version, state, digest) = row;
        if state != lifecycle_name(Lifecycle::Validated) {
            return Err(StoreError::ProposedArtifactLifecycle(format!(
                "proposed artifact {proposal_id} was not in validated state"
            )));
        }
        if digest != replay.artifact_digest() {
            return Err(StoreError::ProposedArtifactLifecycle(
                "eval proof digest does not match stored proposal digest".to_string(),
            ));
        }
        // Distinct, strictly-ordered timestamps so latest_eval_verdict is
        // deterministic across the two semantically different rows.
        let replay_at = Timestamp::now();
        let judge_at = replay_at + std::time::Duration::from_nanos(1);
        for (id, verdict, fitness, evidence, evaluator, recorded_at) in [
            (
                Ulid::new(),
                replay.verdict(),
                replay.fitness(),
                replay.evidence_json(),
                "overlay-eval-gate/replay@v1",
                replay_at,
            ),
            (
                Ulid::new(),
                judge.verdict(),
                judge.fitness(),
                judge.evidence_json(),
                "overlay-eval-gate/risk-judge@v1",
                judge_at,
            ),
        ] {
            let verdict_row = super::eval_verdict_store::EvalVerdict {
                id,
                artifact_kind: kind.clone(),
                artifact_id: artifact_id.clone(),
                artifact_version: version as u32,
                verdict: verdict.to_string(),
                fitness,
                evidence: Some(evidence.to_string()),
                evaluator: Some(evaluator.to_string()),
                artifact_digest: digest.clone(),
                recorded_at,
            };
            super::eval_verdict_store::insert_eval_verdict_conn(&tx, &verdict_row)?;
        }
        let changed = tx.execute(
            "UPDATE proposed_artifacts SET state = ?1 WHERE id = ?2 AND state = ?3",
            params![
                lifecycle_name(Lifecycle::ReviewRequired),
                proposal_id.to_string(),
                lifecycle_name(Lifecycle::Validated)
            ],
        )?;
        if changed != 1 {
            return Err(StoreError::ProposedArtifactLifecycle(
                "proposal changed while eval gate was running".to_string(),
            ));
        }
        tx.commit()?;
        Ok(())
    }
}

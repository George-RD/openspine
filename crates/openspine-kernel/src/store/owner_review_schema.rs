//! Owner-review row schema, column convergence, and row⇄struct mapping for
//! `store::owner_review`. Split out to keep both files under the 500-line
//! gate; the transactional state machine lives in `owner_review.rs`.

use jiff::Timestamp;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use openspine_schemas::owner_review::OwnerReviewState;
use ulid::Ulid;

use super::owner_review::OwnerReviewRow;
use super::StoreError;

pub(super) type ReviewRow = (String, String, i64, String, String, i64, i64);

pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS owner_reviews (
            id TEXT PRIMARY KEY,
            artifact_ref_digest TEXT NOT NULL,
            artifact_ref_schema_version INTEGER NOT NULL,
            state TEXT NOT NULL CHECK(state IN ('pending', 'approved', 'rejected', 'narrowed', 'revoked', 'expired')),
            owner_principal_id TEXT NOT NULL,
            expires_at INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            last_decision TEXT,
            decision_binding_digest TEXT
        );
        CREATE TABLE IF NOT EXISTS owner_review_standing_rules (
            review_id TEXT PRIMARY KEY REFERENCES owner_reviews(id),
            rule_id TEXT NOT NULL,
            rule_version INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_owner_reviews_principal
            ON owner_reviews (owner_principal_id, state);",
    )?;
    ensure_column(
        conn,
        "owner_reviews",
        "last_decision",
        "ALTER TABLE owner_reviews ADD COLUMN last_decision TEXT",
    )?;
    ensure_column(
        conn,
        "owner_reviews",
        "decision_binding_digest",
        "ALTER TABLE owner_reviews ADD COLUMN decision_binding_digest TEXT",
    )?;
    Ok(())
}

fn ensure_column(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
    alter: &str,
) -> Result<(), StoreError> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let exists = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|name| name == column);
    if !exists {
        conn.execute(alter, [])?;
    }
    Ok(())
}

pub(super) fn state_str(state: OwnerReviewState) -> &'static str {
    match state {
        OwnerReviewState::Pending => "pending",
        OwnerReviewState::Approved => "approved",
        OwnerReviewState::Rejected => "rejected",
        OwnerReviewState::Narrowed => "narrowed",
        OwnerReviewState::Revoked => "revoked",
        OwnerReviewState::Expired => "expired",
    }
}

pub(super) fn map_row(row: ReviewRow) -> Result<OwnerReviewRow, StoreError> {
    let (id, digest, schema_version, state, principal, expires_at, _created_at) = row;
    let digest = Digest::parse(digest.clone()).map_err(|_| StoreError::BadDigest(digest))?;
    let id = Ulid::from_string(&id).map_err(|_| StoreError::BadUlid(id))?;
    let owner_principal_id =
        Ulid::from_string(&principal).map_err(|_| StoreError::BadUlid(principal))?;
    let expires_at = epoch_nanos_to_timestamp(expires_at)?;
    let state = match state.as_str() {
        "pending" => OwnerReviewState::Pending,
        "approved" => OwnerReviewState::Approved,
        "rejected" => OwnerReviewState::Rejected,
        "narrowed" => OwnerReviewState::Narrowed,
        "revoked" => OwnerReviewState::Revoked,
        "expired" => OwnerReviewState::Expired,
        other => return Err(StoreError::BadAuditKind(other.to_string())),
    };
    Ok(OwnerReviewRow {
        id,
        artifact_ref: ArtifactRef {
            digest,
            schema_version: schema_version as u32,
        },
        state,
        owner_principal_id,
        expires_at,
    })
}

pub(super) fn read_review_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

pub(super) const REVIEW_COLUMNS: &str =
    "id, artifact_ref_digest, artifact_ref_schema_version, state, \
     owner_principal_id, expires_at, created_at";

pub(super) fn timestamp_to_epoch_nanos(timestamp: Timestamp) -> Result<i64, StoreError> {
    Ok(timestamp.as_nanosecond() as i64)
}

pub(super) fn epoch_nanos_to_timestamp(nanos: i64) -> Result<Timestamp, StoreError> {
    Timestamp::from_nanosecond(nanos as i128)
        .map_err(|_| StoreError::TimestampRange(nanos.to_string()))
}

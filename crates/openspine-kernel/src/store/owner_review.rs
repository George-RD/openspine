//! Owner-review runtime storage (add-channel-neutral-responsibility-review,
//! #129).
//!
//! The canonical `OwnerReviewRequest` is persisted whole as a content-
//! addressed artifact (via `ArtifactStore::put`, whose read-time digest
//! re-verification is the integrity guarantee) and a `review` row keys the
//! review by id, its `ArtifactRef`, its `OwnerReviewState`, the bound owner
//! principal id, and `expires_at`. Every state transition writes an audit
//! event in the same transaction as the row update, so the ledger and the
//! review state cannot drift apart.
//!
//! The review object is NOT live authority (D-007): it is the single
//! security-relevant record every owner surface renders and submits
//! principal-bound decision intents against. Pause/resume/revoke act on the
//! standing-rule status, not on `OwnerReviewState`; this module only records
//! the review-side disposition of a decision.

use jiff::Timestamp;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use openspine_schemas::owner_review::{can_transition, DecisionIntent, OwnerReviewState};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use ulid::Ulid;

use super::{Store, StoreError};

/// One persisted owner-review row.
#[derive(Debug, Clone)]
pub struct OwnerReviewRow {
    pub id: Ulid,
    pub artifact_ref: ArtifactRef,
    pub state: OwnerReviewState,
    pub owner_principal_id: Ulid,
    pub expires_at: Timestamp,
}

pub(super) use super::owner_review_schema::{
    ensure_schema, map_row, read_review_row, state_str, timestamp_to_epoch_nanos, ReviewRow,
    REVIEW_COLUMNS,
};

impl Store {
    /// Persist a new owner-review row. The caller MUST have already stored
    /// the review plaintext via `ArtifactStore::put` and passed the returned
    /// `ArtifactRef`. The row is written with an `owner_review.created` audit
    /// event in the same transaction.
    pub fn insert_owner_review(
        &self,
        id: Ulid,
        artifact_ref: &ArtifactRef,
        owner_principal_id: Ulid,
        expires_at: Timestamp,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::append_audit_conn(
            &tx,
            "owner_review.created",
            None,
            None,
            Some(&id.to_string()),
            None,
            &[],
            std::slice::from_ref(artifact_ref),
        )?;
        tx.execute(
            "INSERT INTO owner_reviews (
                id, artifact_ref_digest, artifact_ref_schema_version, state,
                owner_principal_id, expires_at, created_at
            ) VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6)",
            params![
                id.to_string(),
                artifact_ref.digest.as_str(),
                artifact_ref.schema_version as i64,
                owner_principal_id.to_string(),
                timestamp_to_epoch_nanos(expires_at)?,
                timestamp_to_epoch_nanos(now)?,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Load a persisted owner-review row by id.
    pub fn owner_review_row(&self, id: Ulid) -> Result<Option<OwnerReviewRow>, StoreError> {
        let conn = self.conn.lock();
        let row: Option<ReviewRow> = conn
            .query_row(
                &format!("SELECT {REVIEW_COLUMNS} FROM owner_reviews WHERE id = ?1"),
                params![id.to_string()],
                read_review_row,
            )
            .optional()?;
        row.map(map_row).transpose()
    }

    /// Transition a review's state, refusing any transition not in the legal
    /// table. Writes the audit event in the same transaction as the update.
    /// Returns `Ok(false)` when the review is unknown or already in the
    /// target state (idempotent), `Ok(true)` when the transition applied.
    pub fn transition_owner_review(
        &self,
        id: Ulid,
        from: OwnerReviewState,
        to: OwnerReviewState,
        reason: &str,
    ) -> Result<bool, StoreError> {
        if !can_transition(from, to) {
            return Err(StoreError::FailureRouting(format!(
                "illegal owner-review transition {from:?} -> {to:?}"
            )));
        }
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::append_audit_conn(
            &tx,
            "owner_review.transitioned",
            None,
            None,
            Some(&format!("{id}:{from:?}->{to:?}:{reason}")),
            None,
            &[],
            &[],
        )?;
        let changed = tx.execute(
            "UPDATE owner_reviews SET state = ?2 \
             WHERE id = ?1 AND state = ?3",
            params![id.to_string(), state_str(to), state_str(from)],
        )?;
        tx.commit()?;
        Ok(changed == 1)
    }

    /// Audit a refused owner intent without mutating any review state.
    pub fn record_owner_review_refusal(
        &self,
        id: Ulid,
        submitted_principal_id: Ulid,
        intent: DecisionIntent,
        reason: &str,
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::append_audit_conn(
            &tx,
            "owner_review.decision_refused",
            None,
            None,
            Some(&format!(
                "{id}:{intent:?}:{submitted_principal_id}:{reason}"
            )),
            None,
            &[],
            &[],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Audit a read-only inspection without changing the review disposition
    /// or its last mutating decision.
    pub fn record_owner_review_inspection(
        &self,
        id: Ulid,
        owner_principal_id: Ulid,
        binding_digest: &Digest,
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let principal: Option<String> = tx
            .query_row(
                "SELECT owner_principal_id FROM owner_reviews WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if principal.as_deref() != Some(owner_principal_id.to_string().as_str()) {
            return Err(StoreError::FailureRouting(
                "owner-review principal mismatch".into(),
            ));
        }
        Self::append_audit_conn(
            &tx,
            "owner_review.inspected",
            None,
            None,
            Some(&format!(
                "{id}:Inspect:{}:{owner_principal_id}",
                binding_digest.as_str()
            )),
            None,
            &[],
            &[],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Atomically record a digest-bound principal decision and, when
    /// requested, transition the review state. A replay of the same decision
    /// and digest is a successful no-op.
    pub fn commit_owner_review_decision(
        &self,
        id: Ulid,
        intent: DecisionIntent,
        owner_principal_id: Ulid,
        binding_digest: &Digest,
        transition: Option<(OwnerReviewState, OwnerReviewState)>,
    ) -> Result<bool, StoreError> {
        if let Some((from, to)) = transition {
            if !can_transition(from, to) {
                return Err(StoreError::FailureRouting(format!(
                    "illegal owner-review transition {from:?} -> {to:?}"
                )));
            }
        }
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: Option<(String, String, Option<String>, Option<String>)> = tx
            .query_row(
                "SELECT state, owner_principal_id, last_decision, decision_binding_digest
                 FROM owner_reviews WHERE id = ?1",
                params![id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((state, principal, previous_intent, previous_digest)) = current else {
            return Ok(false);
        };
        if principal != owner_principal_id.to_string() {
            return Err(StoreError::FailureRouting(
                "owner-review principal mismatch".into(),
            ));
        }
        let intent_name = format!("{intent:?}");
        if previous_intent.as_deref() == Some(intent_name.as_str())
            && previous_digest.as_deref() == Some(binding_digest.as_str())
        {
            return Ok(false);
        }
        let target_state = transition.map_or_else(|| state.clone(), |(_, to)| state_str(to).into());
        if let Some((from, _)) = transition {
            if state != state_str(from) {
                return Err(StoreError::FailureRouting(
                    "owner-review state changed before decision commit".into(),
                ));
            }
        }
        let changed = tx.execute(
            "UPDATE owner_reviews SET state = ?2, last_decision = ?3,
             decision_binding_digest = ?4 WHERE id = ?1 AND owner_principal_id = ?5",
            params![
                id.to_string(),
                target_state,
                intent_name,
                binding_digest.as_str(),
                owner_principal_id.to_string(),
            ],
        )?;
        Self::append_audit_conn(
            &tx,
            "owner_review.decision",
            None,
            None,
            Some(&format!(
                "{id}:{intent:?}:{}:{owner_principal_id}",
                binding_digest.as_str()
            )),
            None,
            &[],
            &[],
        )?;
        if transition.is_some() {
            Self::append_audit_conn(
                &tx,
                "owner_review.transitioned",
                None,
                None,
                Some(&format!("{id}:{state}->{target_state}")),
                None,
                &[],
                &[],
            )?;
        }
        tx.commit()?;
        Ok(changed == 1)
    }

    /// Atomically supersede a pending review with its persisted narrowed
    /// replacement. The replacement artifact MUST already be durable.
    pub fn insert_narrowed_owner_review(
        &self,
        supersedes: (Ulid, &Digest),
        new_id: Ulid,
        artifact_ref: &ArtifactRef,
        owner_principal_id: Ulid,
        expires_at: Timestamp,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        let (original_id, original_binding_digest) = supersedes;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE owner_reviews SET state = 'narrowed', last_decision = 'Narrow',
             decision_binding_digest = ?3
             WHERE id = ?1 AND owner_principal_id = ?2 AND state = 'pending'",
            params![
                original_id.to_string(),
                owner_principal_id.to_string(),
                original_binding_digest.as_str(),
            ],
        )?;
        if changed != 1 {
            return Err(StoreError::FailureRouting(
                "original owner review is not pending for this principal".into(),
            ));
        }
        tx.execute(
            "INSERT INTO owner_reviews (
                id, artifact_ref_digest, artifact_ref_schema_version, state,
                owner_principal_id, expires_at, created_at
            ) VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6)",
            params![
                new_id.to_string(),
                artifact_ref.digest.as_str(),
                artifact_ref.schema_version as i64,
                owner_principal_id.to_string(),
                timestamp_to_epoch_nanos(expires_at)?,
                timestamp_to_epoch_nanos(now)?,
            ],
        )?;
        Self::append_audit_conn(
            &tx,
            "owner_review.decision",
            None,
            None,
            Some(&format!(
                "{original_id}:Narrow:{}:{new_id}:{owner_principal_id}",
                original_binding_digest.as_str()
            )),
            None,
            &[],
            std::slice::from_ref(artifact_ref),
        )?;
        Self::append_audit_conn(
            &tx,
            "owner_review.transitioned",
            None,
            None,
            Some(&format!("{original_id}:pending->narrowed:{new_id}")),
            None,
            &[],
            &[],
        )?;
        Self::append_audit_conn(
            &tx,
            "owner_review.created",
            None,
            None,
            Some(&new_id.to_string()),
            None,
            &[],
            std::slice::from_ref(artifact_ref),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn owner_review_standing_rule(
        &self,
        review_id: Ulid,
    ) -> Result<Option<(String, u32)>, StoreError> {
        let conn = self.conn.lock();
        let row: Option<(String, i64)> = conn
            .query_row(
                "SELECT rule_id, rule_version FROM owner_review_standing_rules
                 WHERE review_id = ?1",
                params![review_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(row.map(|(rule_id, version)| (rule_id, version as u32)))
    }

    pub fn owner_review_last_decision(
        &self,
        id: Ulid,
    ) -> Result<Option<(String, Digest)>, StoreError> {
        let conn = self.conn.lock();
        let value: (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT last_decision, decision_binding_digest
                 FROM owner_reviews WHERE id = ?1",
                params![id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .unwrap_or((None, None));
        match value {
            (Some(intent), Some(digest)) => {
                let digest =
                    Digest::parse(digest.clone()).map_err(|_| StoreError::BadDigest(digest))?;
                Ok(Some((intent, digest)))
            }
            _ => Ok(None),
        }
    }

    pub fn owner_review_decision_digest(&self, id: Ulid) -> Result<Option<Digest>, StoreError> {
        let conn = self.conn.lock();
        let value: Option<Option<String>> = conn
            .query_row(
                "SELECT decision_binding_digest FROM owner_reviews WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        value
            .flatten()
            .map(|digest| Digest::parse(digest.clone()).map_err(|_| StoreError::BadDigest(digest)))
            .transpose()
    }

    /// Persisted review rows. Backs the assertion that a refused decision
    /// creates no replacement review.
    #[cfg(test)]
    pub fn count_owner_reviews(&self) -> Result<usize, StoreError> {
        let conn = self.conn.lock();
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM owner_reviews", [], |row| row.get(0))?;
        Ok(count as usize)
    }
}

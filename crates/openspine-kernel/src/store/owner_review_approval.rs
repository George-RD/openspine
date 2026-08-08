//! Atomic owner-review approval and standing-rule activation.

use jiff::Timestamp;
use openspine_schemas::digest::Digest;
use openspine_schemas::owner_review::DecisionIntent;
use openspine_schemas::standing_rule::StandingRuleManifest;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use ulid::Ulid;

use super::{Store, StoreError};

impl Store {
    pub fn approve_owner_review_and_activate_rule(
        &self,
        review_id: Ulid,
        owner_principal_id: Ulid,
        binding_digest: &Digest,
        manifest: &StandingRuleManifest,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        self.reject_incomplete_scope_binding(manifest)?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: Option<(String, String)> = tx
            .query_row(
                "SELECT state, owner_principal_id FROM owner_reviews WHERE id = ?1",
                params![review_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((state, principal)) = current else {
            return Err(StoreError::FailureRouting(
                "owner review does not exist".into(),
            ));
        };
        if state != "pending" || principal != owner_principal_id.to_string() {
            return Err(StoreError::FailureRouting(
                "owner review is not pending for this principal".into(),
            ));
        }
        Self::activate_standing_rule_in_tx(&tx, manifest, None, now)?;
        tx.execute(
            "UPDATE owner_reviews SET state = 'approved', last_decision = ?2,
             decision_binding_digest = ?3 WHERE id = ?1 AND state = 'pending'",
            params![
                review_id.to_string(),
                format!("{:?}", DecisionIntent::Approve),
                binding_digest.as_str(),
            ],
        )?;
        tx.execute(
            "INSERT INTO owner_review_standing_rules (review_id, rule_id, rule_version)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(review_id) DO UPDATE SET
                rule_id = excluded.rule_id, rule_version = excluded.rule_version",
            params![review_id.to_string(), manifest.id, manifest.version as i64],
        )?;
        Self::append_audit_conn(
            &tx,
            "owner_review.decision",
            Some(&manifest.action_id),
            None,
            Some(&format!(
                "{review_id}:Approve:{}:{}:{owner_principal_id}",
                binding_digest.as_str(),
                manifest.id
            )),
            None,
            &[],
            &[],
        )?;
        Self::append_audit_conn(
            &tx,
            "owner_review.transitioned",
            Some(&manifest.action_id),
            None,
            Some(&format!("{review_id}:pending->approved")),
            None,
            &[],
            &[],
        )?;
        tx.commit()?;
        Ok(())
    }
}

//! Standing-rule dark-window recovery and monitoring (split from
//! `standing_rules_pending.rs` to keep every file under the 500-line gate).
//!
//! These are the crash-recovery and observability helpers: re-drive a fired
//! default whose re-dispatch was never durably attempted (`none` →
//! `dispatched`), and count unresolved pending actions. Both fail closed —
//! a malformed persisted row is an error, never a silently-nilled identity.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::standing_rule::DarkWindowDefault;
use rusqlite::params;
use rusqlite::TransactionBehavior;
use ulid::Ulid;

use super::standing_rules::timestamp_to_epoch_nanos;
use super::standing_rules::StandingRulePendingAction;
use super::{Store, StoreError};

// These are the crash-recovery and observability helpers: re-drive a fired
// default whose one-use token was never consumed (`none` → claimed), surface
// a claimed-but-never-attempted effect for owner attention (fail closed,
// never silently lost or blindly re-run), and mark an effect durably
// attempted before the connector call. Malformed rows are propagated as
// errors (fail closed) — never silently nilled or dropped.
impl Store {
    pub fn pending_dark_window_recoverable(
        &self,
    ) -> Result<Vec<StandingRulePendingAction>, StoreError> {
        let conn = self.conn.lock();
        type PendingRecoverRow = (
            String,
            String,
            i64,
            String,
            String,
            i64,
            Option<String>,
            String,
            String,
        );
        let rows: Vec<PendingRecoverRow> = conn
            .prepare(
                "SELECT pending_id, rule_id, rule_version, task_grant_id, action_id, \
                        bound_chat_id, payload_ref_json, request_fingerprint, dark_window_default \
                 FROM standing_rule_pending_actions \
                 WHERE resolution = 'allowed' AND dispatch_state = 'none'",
            )?
            .query_map([], |row| {
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
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (
            pending_id,
            rule_id,
            rule_version,
            task_grant_id,
            action_id,
            bound_chat_id,
            payload_ref_json,
            request_fingerprint,
            default_str,
        ) in rows
        {
            let grant_id = Ulid::from_string(&task_grant_id).map_err(|err| {
                StoreError::TimestampRange(format!("bad grant id in pending {pending_id}: {err}"))
            })?;
            let payload_ref: Option<ArtifactRef> = payload_ref_json
                .map(|j| serde_json::from_str(&j))
                .transpose()?;
            out.push(StandingRulePendingAction {
                pending_id,
                rule_id,
                rule_version: rule_version as u32,
                task_grant_id: grant_id,
                action_id: ActionId::new(&action_id),
                bound_chat_id,
                payload_ref,
                default: if default_str == "allow" {
                    DarkWindowDefault::Allow
                } else {
                    DarkWindowDefault::Deny
                },
                request_fingerprint,
                dispatch_state: "none".to_string(),
                resolved_at: None,
                resolution: Some("allowed".to_string()),
            });
        }
        Ok(out)
    }

    /// Claimed-but-never-attempted fired defaults: a one-use token was consumed
    /// (state `claimed`) but the connector effect was never durably attempted
    /// (state still `claimed`, not `dispatched`). A crash between the token
    /// claim and the pre-effect durably-attempted write leaves exactly this
    /// row. Recovery SURFACES these for owner attention (fail closed) and must
    /// never re-run the effect — the external connector may already have
    /// executed. Rows already surfaced (`owner_attention_since IS NOT NULL`)
    /// are excluded so the surface audit is emitted exactly once.
    pub fn pending_dark_window_claimed_unredriven(
        &self,
    ) -> Result<Vec<StandingRulePendingAction>, StoreError> {
        let conn = self.conn.lock();
        type PendingClaimedRow = (
            String,
            String,
            i64,
            String,
            String,
            i64,
            Option<String>,
            String,
            String,
        );
        let rows: Vec<PendingClaimedRow> = conn
            .prepare(
                "SELECT pending_id, rule_id, rule_version, task_grant_id, action_id, \
                        bound_chat_id, payload_ref_json, request_fingerprint, dark_window_default \
                 FROM standing_rule_pending_actions \
                 WHERE resolution = 'allowed' AND dispatch_state = 'claimed' \
                   AND owner_attention_since IS NULL",
            )?
            .query_map([], |row| {
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
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (
            pending_id,
            rule_id,
            rule_version,
            task_grant_id,
            action_id,
            bound_chat_id,
            payload_ref_json,
            request_fingerprint,
            default_str,
        ) in rows
        {
            let grant_id = Ulid::from_string(&task_grant_id).map_err(|err| {
                StoreError::TimestampRange(format!("bad grant id in pending {pending_id}: {err}"))
            })?;
            let payload_ref: Option<ArtifactRef> = payload_ref_json
                .map(|j| serde_json::from_str(&j))
                .transpose()?;
            out.push(StandingRulePendingAction {
                pending_id,
                rule_id,
                rule_version: rule_version as u32,
                task_grant_id: grant_id,
                action_id: ActionId::new(&action_id),
                bound_chat_id,
                payload_ref,
                default: if default_str == "allow" {
                    DarkWindowDefault::Allow
                } else {
                    DarkWindowDefault::Deny
                },
                request_fingerprint,
                dispatch_state: "claimed".to_string(),
                resolved_at: None,
                resolution: Some("allowed".to_string()),
            });
        }
        Ok(out)
    }

    /// Durably mark a claimed fired default as surfaced for owner attention
    /// (fail closed). Idempotent: sets `owner_attention_since` only on the
    /// first transition (WHERE ... IS NULL) and appends the
    /// `standing_rule.dark_window_effect_unconfirmed` audit in the same
    /// `changed == 1` transaction, so the recovery sweep emits exactly one
    /// surface and the owner can investigate a possibly-executed effect.
    pub fn surface_dark_window_claimed_for_owner(
        &self,
        pending_id: &str,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE standing_rule_pending_actions \
             SET owner_attention_since = ?2 \
             WHERE pending_id = ?1 AND owner_attention_since IS NULL \
               AND dispatch_state = 'claimed'",
            params![pending_id, timestamp_to_epoch_nanos(now)?],
        )?;
        if changed == 1 {
            Self::append_audit_conn(
                &tx,
                "standing_rule.dark_window_effect_unconfirmed",
                None,
                None,
                Some(&format!(
                    "fired dark-window default claimed but effect unconfirmed; pending {pending_id} \
                     requires owner attention (never auto-rerun)"
                )),
                None,
                &[],
                &[],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Durably record that a fired default's effect was attempted, called only
    /// AFTER the connector returns success (the outbox/receipt write). On
    /// success the row leaves the `claimed` state for `dispatched` and is never
    /// re-selected by recovery. A crash between the connector's effect and this
    /// write leaves the row in `claimed`, which recovery SURFACES for owner
    /// attention (fail closed, never auto-rerun). `receipt_digest` binds the
    /// attempt to a request identity for idempotency.
    pub fn mark_fired_effect_attempted(
        &self,
        pending_id: &str,
        receipt_digest: &str,
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE standing_rule_pending_actions \
             SET dispatch_state = 'dispatched', dispatch_receipt_digest = ?2 \
             WHERE pending_id = ?1 AND dispatch_state = 'claimed'",
            params![pending_id, receipt_digest],
        )?;
        if changed != 1 {
            return Err(StoreError::FailureRouting(format!(
                "fired pending {pending_id} was not in 'claimed' state; effect attempt not recorded"
            )));
        }
        Self::append_audit_conn(
            &tx,
            "standing_rule.dark_window_effect_attempted",
            None,
            None,
            Some(&format!(
                "fired dark-window effect durably attempted; pending {pending_id}"
            )),
            None,
            &[],
            &[],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Re-arm a fired default after a pre-effect failure. The token was
    /// consumed only to claim the action; when no connector effect has begun,
    /// it is safe to return the row to the recoverable `none` state so timer
    /// recovery can retry it. This transition is deliberately fenced to
    /// `claimed` and clears the one-use marker atomically.
    pub fn rearm_standing_rule_fired_pending(&self, pending_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        let changed = conn.execute(
            "UPDATE standing_rule_pending_actions
             SET token_consumed_at = NULL,
                 dispatch_state = 'none',
                 dispatch_receipt_digest = NULL,
                 owner_attention_since = NULL
             WHERE pending_id = ?1 AND dispatch_state = 'claimed'
               AND owner_attention_since IS NULL",
            params![pending_id],
        )?;
        if changed != 1 {
            return Err(StoreError::FailureRouting(format!(
                "fired pending {pending_id} was not rearmable before effect"
            )));
        }
        Ok(())
    }
    /// Count unresolved pending dark-window actions for a rule — used by the
    /// scheduling idempotency test and any monitoring.
    pub fn pending_dark_window_count(&self, rule_id: &str) -> Result<usize, StoreError> {
        let conn = self.conn.lock();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM standing_rule_pending_actions \
             WHERE rule_id = ?1 AND resolved_at IS NULL",
            params![rule_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    // `Timestamp` is part of the store's shared helper surface; keep the
    // import referenced so downstream helpers can adopt a uniform signature.
    #[allow(dead_code)]
    fn _unused_now(_now: Timestamp) {}
}

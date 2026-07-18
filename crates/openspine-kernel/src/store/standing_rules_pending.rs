//! Durable dark-window pending-action state machine for standing rules
//! (AD-012 leaning). Split from `standing_rules.rs` to keep every file under
//! the 500-line gate.
//!
//! Lifecycle: a `schedule` persists one pending action per stable request
//! fingerprint (deduped via a unique index across all states). When the
//! kernel timer fires and the owner has not resolved it, `claim` durably
//! decides the pre-agreed default and returns the action for re-dispatch
//! (Allow only; Deny resolves `denied` and returns nothing). The owner may
//! resolve it earlier (`resolve_pending_action`), which makes the fired
//! by [`Store::consume_standing_rule_fired_pending`]; the durable
//! `dispatch_state` (`none` → `claimed` → `dispatched`) bounds the effect so a
//! crash before dispatch is recoverable and a crash after the claim is
//! surfaced for owner attention (never silently lost, never blindly re-run —
//! the external effect may already have executed).

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::standing_rule::DarkWindowDefault;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use ulid::Ulid;

use super::standing_rules::{
    epoch_nanos_to_timestamp, timestamp_to_epoch_nanos, StandingRule, StandingRulePendingAction,
};
use super::{Store, StoreError};

impl Store {
    /// Persist the exact pending action a dark window will resolve, and
    /// schedule its D-074 timer bound to that pending item (not the rule).
    /// Deduplicated per stable `(rule_id, rule_version, request_fingerprint)`
    /// via a unique index across all states: a repeat of an already-resolved
    /// request reuses the existing row and never re-executes the default
    /// (P1-8). Carries only
    /// the encrypted `ArtifactRef` to the payload — never the plaintext
    /// (P1-7). Returns the freshly-scheduled timer id when a new live dark
    /// window was created, or `None` when an existing open timer covers the
    /// request or the request is already terminal (no new timer to report).
    #[allow(clippy::too_many_arguments)]
    pub fn schedule_standing_rule_dark_window(
        &self,
        rule: &StandingRule,
        grant_id: Ulid,
        bound_chat_id: i64,
        payload_ref: Option<ArtifactRef>,
        fingerprint: &str,
        fires_at: Timestamp,
        now: Timestamp,
    ) -> Result<Option<String>, StoreError> {
        let dw = match rule.dark_window {
            Some(dw) => dw,
            None => {
                return Err(StoreError::FailureRouting(
                    "schedule_standing_rule_dark_window requires a configured dark_window"
                        .to_string(),
                ))
            }
        };
        let pending_id = Ulid::new().to_string();
        let timer_id = Ulid::new().to_string();
        let run_id = format!("srdw_{pending_id}");
        let default_str = match dw.default {
            DarkWindowDefault::Allow => "allow",
            DarkWindowDefault::Deny => "deny",
        };
        let payload_json = payload_ref
            .as_ref()
            .map(|r| serde_json::to_string(r).map_err(StoreError::Serde))
            .transpose()?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT OR IGNORE INTO standing_rule_pending_actions (
                pending_id, rule_id, rule_version, task_grant_id, action_id,
                bound_chat_id, payload_ref_json, dark_window_default,
                request_fingerprint, requested_at, resolved_at, resolution
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, NULL)",
            params![
                pending_id,
                rule.rule_id,
                rule.version as i64,
                grant_id.to_string(),
                rule.action_id.to_string(),
                bound_chat_id,
                payload_json,
                default_str,
                fingerprint,
                timestamp_to_epoch_nanos(now)?,
            ],
        )?;
        let effective_pending_id: String = if tx.changes() == 0 {
            tx.query_row(
                "SELECT pending_id FROM standing_rule_pending_actions \
                 WHERE rule_id = ?1 AND rule_version = ?3 AND request_fingerprint = ?2 \
                 LIMIT 1",
                params![rule.rule_id, fingerprint, rule.version as i64],
                |row| row.get(0),
            )?
        } else {
            pending_id
        };
        let existing_timer: Option<(String, Option<i64>)> = tx
            .query_row(
                "SELECT timer_id, applied_at FROM standing_rule_timer_links WHERE pending_id = ?1 LIMIT 1",
                params![effective_pending_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        // `Some(timer_id)` is returned ONLY when a brand-new timer was just
        // inserted; an existing live timer (not yet fired) or a terminal
        // request with no open timer schedules nothing new and returns `None`
        // (P1-8 stable idempotency — no duplicate timer, no duplicate audit).
        let scheduled: Option<String> = match existing_timer {
            Some((_, None)) => None,
            _ => {
                let still_open: bool = tx
                    .query_row(
                        "SELECT 1 FROM standing_rule_pending_actions WHERE pending_id = ?1 AND resolved_at IS NULL",
                        params![effective_pending_id],
                        |r| r.get::<_, i64>(0),
                    )
                    .optional()?
                    .is_some();
                if !still_open {
                    None
                } else {
                    tx.execute(
                        "INSERT INTO workflow_timers (timer_id, run_id, fires_at, status, fired_event_id)
                         VALUES (?1, ?2, ?3, 'pending', NULL)",
                        params![timer_id, run_id, timestamp_to_epoch_nanos(fires_at)?],
                    )?;
                    tx.execute(
                        "INSERT INTO standing_rule_timer_links (timer_id, pending_id, applied_at)
                         VALUES (?1, ?2, NULL)",
                        params![timer_id, effective_pending_id],
                    )?;
                    Self::append_audit_conn(
                        &tx,
                        "workflow.timer_scheduled",
                        Some(&rule.action_id),
                        None,
                        Some("standing rule dark-window timer scheduled for a specific pending action"),
                        Some(grant_id),
                        &[],
                        &[],
                    )?;
                    Some(timer_id)
                }
            }
        };
        tx.commit()?;
        Ok(scheduled)
    }

    /// Claim a fired dark-window timer for processing. Transactionally
    /// idempotent (D-082). A `timer_id` not in `standing_rule_timer_links`, or
    /// already `applied_at`-marked, returns `None`. On a fresh claim the
    /// pre-agreed default is durably decided (`resolution` set, idempotent
    /// across replays) and the timer is marked applied. `Allow` returns the
    /// pending action for re-dispatch; `Deny` resolves `denied` and returns
    /// `None` (no authority). A recoverable pending (decided `allowed` but
    /// `dispatch_state = 'none'`) is also returned so a crash between claim
    /// and dispatch is retried exactly once (P1-10), not lost.
    pub fn claim_standing_rule_dark_window(
        &self,
        timer_id: &str,
        now: Timestamp,
    ) -> Result<Option<StandingRulePendingAction>, StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let link: Option<(String, Option<i64>)> = tx
            .query_row(
                "SELECT pending_id, applied_at FROM standing_rule_timer_links WHERE timer_id = ?1",
                params![timer_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((pending_id, applied_at)) = link else {
            return Ok(None);
        };
        if applied_at.is_some() {
            // Already claimed: idempotent no-op. Recovery re-drives the pending
            // row directly by id, not through this link.
            return Ok(None);
        }
        type PendingRow = (
            String,
            i64,
            String,
            String,
            i64,
            Option<String>,
            String,
            String,
            String,
            Option<i64>,
            Option<String>,
        );
        let pending: Option<PendingRow> = tx
            .query_row(
                "SELECT rule_id, rule_version, task_grant_id, action_id, bound_chat_id, \
                        payload_ref_json, dark_window_default, request_fingerprint, \
                        dispatch_state, resolved_at, resolution \
                 FROM standing_rule_pending_actions WHERE pending_id = ?1",
                params![pending_id],
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
                        row.get(10)?,
                    ))
                },
            )
            .optional()?;
        let Some((
            rule_id,
            rule_version,
            task_grant_id,
            action_id,
            bound_chat_id,
            payload_ref_json,
            default_str,
            fingerprint,
            dispatch_state,
            resolved_at,
            resolution,
        )) = pending
        else {
            return Ok(None);
        };
        let now_nanos = timestamp_to_epoch_nanos(now)?;
        let decided = resolved_at.is_some();
        let terminal = resolved_at
            .is_some_and(|_| matches!(resolution.as_deref(), Some("denied") | Some("stale")));
        let deny_default = default_str == "deny";
        if terminal {
            tx.execute(
                "UPDATE standing_rule_timer_links SET applied_at = ?2 WHERE timer_id = ?1",
                params![timer_id, now_nanos],
            )?;
            tx.commit()?;
            return Ok(None);
        }
        if !decided {
            // Normal fire: owner silence = pre-agreed default. Deny resolves
            // `denied` (no dispatch); Allow resolves `allowed` (dispatchable).
            let resolution = if deny_default { "denied" } else { "allowed" };
            tx.execute(
                "UPDATE standing_rule_pending_actions SET resolved_at = ?2, resolution = ?3 \
                 WHERE pending_id = ?1",
                params![pending_id, now_nanos, resolution],
            )?;
            Self::append_audit_conn(
                &tx,
                "standing_rule.dark_window_fired",
                None,
                None,
                Some(&format!(
                    "rule {rule_id} dark-window fired; default: {default_str}"
                )),
                None,
                &[],
                &[],
            )?;
        }
        tx.execute(
            "UPDATE standing_rule_timer_links SET applied_at = ?2 WHERE timer_id = ?1",
            params![timer_id, now_nanos],
        )?;
        tx.commit()?;
        // Compute the effective post-update state: a freshly-fired Allow
        // (owner silence = pre-agreed Allow default) must surface as
        // `resolution = 'allowed'` / `resolved_at = now` so the consumer
        // dispatches it immediately, not just on a later recovery sweep.
        let effective_resolution: Option<String> = if !decided {
            if deny_default {
                Some("denied".to_string())
            } else {
                Some("allowed".to_string())
            }
        } else {
            resolution.clone()
        };
        let effective_resolved_at: Option<Timestamp> = if !decided {
            Some(now)
        } else {
            resolved_at.map(epoch_nanos_to_timestamp).transpose()?
        };
        // Denied by the fired default (or already resolved denied/stale):
        // nothing to dispatch.
        if effective_resolution.as_deref() != Some("allowed") {
            return Ok(None);
        }
        let payload_ref = payload_ref_json
            .map(|json| serde_json::from_str::<ArtifactRef>(&json))
            .transpose()?;
        Ok(Some(StandingRulePendingAction {
            pending_id,
            rule_id,
            rule_version: rule_version as u32,
            task_grant_id: Ulid::from_string(&task_grant_id)
                .map_err(|err| StoreError::TimestampRange(format!("bad grant id: {err}")))?,
            action_id: ActionId::new(&action_id),
            bound_chat_id,
            payload_ref,
            default: if default_str == "allow" {
                DarkWindowDefault::Allow
            } else {
                DarkWindowDefault::Deny
            },
            request_fingerprint: fingerprint,
            dispatch_state,
            resolved_at: effective_resolved_at,
            resolution: effective_resolution,
        }))
    }

    /// Resolve the pending identity for a stable request fingerprint so the
    /// owner notification can bind Allow/Deny buttons to the exact row.
    pub fn pending_id_for_fingerprint(
        &self,
        rule_id: &str,
        rule_version: u32,
        fingerprint: &str,
    ) -> Result<Option<String>, StoreError> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row(
                "SELECT pending_id FROM standing_rule_pending_actions
                 WHERE rule_id = ?1 AND rule_version = ?2 AND request_fingerprint = ?3",
                params![rule_id, rule_version as i64, fingerprint],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Owner-addressable resolution of a pending dark-window action (P1-9):
    /// the owner may allow or deny the specific pending action before the
    /// timer fires. First write wins and is idempotent; a late tap after the
    /// timer already fired is a harmless no-op (the fired default path has
    /// already decided `allowed`). Cancelling here means the fired timer will
    /// find `resolution = 'denied'`/`'stale'` and apply no authority.
    pub fn resolve_pending_action(
        &self,
        pending_id: &str,
        allow: bool,
        now: Timestamp,
    ) -> Result<bool, StoreError> {
        let resolution = if allow { "allowed" } else { "denied" };
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Only set if still unresolved (first write wins).
        let changed = tx.execute(
            "UPDATE standing_rule_pending_actions \
             SET resolved_at = ?2, resolution = ?3 \
             WHERE pending_id = ?1 AND resolved_at IS NULL",
            params![pending_id, timestamp_to_epoch_nanos(now)?, resolution],
        )?;
        if changed >= 1 {
            Self::append_audit_conn(
                &tx,
                "standing_rule.pending_resolved",
                None,
                None,
                Some(&format!(
                    "pending {pending_id} resolved by owner: {resolution}"
                )),
                None,
                &[],
                &[],
            )?;
        }
        tx.commit()?;
        Ok(changed >= 1)
    }

    /// Consume a fired dark-window one-use token (P1-11): invoked from the
    /// shared mediation boundary when a re-dispatched action is still
    /// over-budget. Digest-bound to the exact request (action + grant + chat +
    /// payload fingerprint) so it cannot be replayed against a different
    /// request, and one-use (the `token_consumed_at` flip means a second
    /// attempt, or a replay after a successful dispatch, returns `None` and
    /// fails closed). On success it records the owner-silence waiver as a
    /// *reserved* usage row (not yet committed — AD-106 failed-effects rule;
    /// P1-6) and returns the fired reservation identity so the caller can
    /// finalize it after a successful effect or cancel it on failure. The
    /// state predicates (`resolution = 'allowed'`, `resolved_at IS NOT NULL`,
    /// `token_consumed_at IS NULL`, matching fingerprint, and the rule still
    /// current at that version) are part of the same conditional UPDATE
    /// guarded by `changes() == 1` — so an unresolved or owner-denied pending
    /// id can never mint an Allow token, and the flip is atomic.
    pub fn consume_standing_rule_fired_pending(
        &self,
        pending_id: &str,
        action: &ActionId,
        grant_id: Ulid,
        bound_chat_id: i64,
        payload_ref: &Option<ArtifactRef>,
        now: Timestamp,
    ) -> Result<Option<(String, u32, String)>, StoreError> {
        let fingerprint = super::standing_rules::standing_rule_fingerprint(
            action,
            grant_id,
            bound_chat_id,
            payload_ref,
        );
        let now_nanos = timestamp_to_epoch_nanos(now)?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        type Row = (String, i64);
        let row: Option<Row> = tx
            .query_row(
                "SELECT rule_id, rule_version \
                 FROM standing_rule_pending_actions \
                 WHERE pending_id = ?1 \
                   AND resolution = 'allowed' AND resolved_at IS NOT NULL \
                   AND token_consumed_at IS NULL AND request_fingerprint = ?2 \
                   AND EXISTS (SELECT 1 FROM standing_rules r \
                               WHERE r.rule_id = standing_rule_pending_actions.rule_id \
                                 AND r.version = standing_rule_pending_actions.rule_version \
                                 AND r.status = 'active' \
                                 AND (r.expires_after_secs = 0 OR \
                                      (?3 - COALESCE(r.last_used_at, r.activated_at)) \
                                        < r.expires_after_secs * 1000000000))",
                params![pending_id, fingerprint, now_nanos],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((rule_id, rule_version)) = row else {
            return Ok(None);
        };
        // Atomically flip the one-use token AND mark the re-dispatch as
        // *claimed* (token consumed, effect not yet attempted) in the same
        // conditional UPDATE. Guarded by `changes() == 1` so an already-consumed
        // or owner-denied/unresolved pending can never mint an Allow token, and
        // a crash after this commit leaves a `claimed` row that recovery
        // SURFACES for owner attention (fail closed — never silently lost,
        // never blindly re-run because the connector may already have run).
        let flipped = tx.execute(
            "UPDATE standing_rule_pending_actions \
             SET token_consumed_at = ?2, dispatch_state = 'claimed' \
             WHERE pending_id = ?1 AND token_consumed_at IS NULL \
               AND resolution = 'allowed' AND resolved_at IS NOT NULL",
            params![pending_id, now_nanos],
        )?;
        if flipped != 1 {
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO standing_rule_usage (rule_id, version, kind, used_at, status, reservation_id)
             VALUES (?1, ?2, 'quota', ?3, 'reserved', ?4),
                    (?1, ?2, 'rate', ?3, 'reserved', ?4)",
            params![rule_id, rule_version, now_nanos, pending_id],
        )?;
        Self::append_audit_conn(
            &tx,
            "standing_rule.dark_window_admitted",
            Some(action),
            None,
            Some(&format!("fired dark-window default admitted for {rule_id}")),
            Some(grant_id),
            &[],
            &[],
        )?;
        tx.commit()?;
        Ok(Some((rule_id, rule_version as u32, pending_id.to_string())))
    }
}

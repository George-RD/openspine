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
use openspine_schemas::owner_surface::OwnerSurfaceRef;
use openspine_schemas::standing_rule::DarkWindowDefault;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use ulid::Ulid;

use super::standing_rules::{
    epoch_nanos_to_timestamp, timestamp_to_epoch_nanos, StandingRule, StandingRulePendingAction,
};
use super::standing_rules_exceptions::DarkWindowSchedule;
use super::{Store, StoreError};

/// Rebuild the channel-neutral owner binding of a persisted pending action.
/// A pre-v7 row has none: the dropped chat integer cannot be promoted into a
/// principal-bound surface, so the row is refused instead of guessed at.
pub(super) fn parse_owner_surface(json: Option<String>) -> Result<OwnerSurfaceRef, StoreError> {
    let json = json.ok_or_else(|| {
        StoreError::BadOwnerSurface("standing_rule_pending_actions.owner_surface_json".into())
    })?;
    serde_json::from_str(&json).map_err(|err| StoreError::BadOwnerSurface(err.to_string()))
}

impl Store {
    /// Persist the exact pending action a dark window will resolve, and
    /// schedule its D-074 timer bound to that pending item (not the rule).
    /// Deduplicated per stable `(rule_id, rule_version, request_fingerprint)`
    /// via a unique index across all states: a repeat of an already-resolved
    /// request reuses the existing row and never re-executes the default
    /// (P1-8). Carries only the encrypted `ArtifactRef` to the payload —
    /// never the plaintext (P1-7).
    ///
    /// Bounded by the rule's reviewed `max_pending_exceptions` (#135). The
    /// outstanding count is taken inside this transaction, BEFORE anything is
    /// inserted, so a refusal cannot leave an orphan row or timer and two
    /// concurrent callers cannot both take the final slot — `BEGIN IMMEDIATE`
    /// takes the write lock at the first statement, the same serialization
    /// D-050 relies on for quota and rate. Deduplication is evaluated before
    /// the count, so an idempotent repeat of an already-scheduled request
    /// never consumes a slot.
    ///
    /// `reviewed_scope_digest`/`compatibility_digest` bind the exception to
    /// the reviewed context it was minted for; a fired token is refused later
    /// if the freshly resolved context no longer equals them. Both are `None`
    /// for a rule with no scope binding.
    #[allow(clippy::too_many_arguments)]
    pub fn schedule_standing_rule_dark_window(
        &self,
        rule: &StandingRule,
        grant_id: Ulid,
        owner_surface: &OwnerSurfaceRef,
        payload_ref: Option<ArtifactRef>,
        fingerprint: &str,
        reviewed_scope_digest: Option<&str>,
        compatibility_digest: Option<&str>,
        fires_at: Timestamp,
        now: Timestamp,
    ) -> Result<DarkWindowSchedule, StoreError> {
        let dw = match rule.dark_window {
            Some(dw) => dw,
            None => {
                return Err(StoreError::FailureRouting(
                    "schedule_standing_rule_dark_window requires a configured dark_window"
                        .to_string(),
                ));
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

        // Dedup FIRST: an idempotent repeat of an already-scheduled request is
        // not a new exception and must never be refused at the cap.
        let existing_pending_id: Option<String> = tx
            .query_row(
                "SELECT pending_id FROM standing_rule_pending_actions \
                 WHERE rule_id = ?1 AND rule_version = ?3 AND request_fingerprint = ?2 \
                 LIMIT 1",
                params![rule.rule_id, fingerprint, rule.version as i64],
                |row| row.get(0),
            )
            .optional()?;

        let effective_pending_id = match existing_pending_id {
            Some(existing) => existing,
            None => {
                // A genuinely new exception. Count the outstanding ones for
                // this exact rule version BEFORE inserting anything, so a
                // refusal leaves no orphan row and no orphan timer, and the
                // `BEGIN IMMEDIATE` write lock keeps two racing callers from
                // both taking the final slot.
                let outstanding: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM standing_rule_pending_actions \
                     WHERE rule_id = ?1 AND rule_version = ?2 AND resolved_at IS NULL",
                    params![rule.rule_id, rule.version as i64],
                    |row| row.get(0),
                )?;
                if outstanding >= i64::from(dw.max_pending_exceptions) {
                    Self::append_audit_conn(
                        &tx,
                        "standing_rule.exception_suppressed_at_cap",
                        Some(&rule.action_id),
                        None,
                        Some(&format!(
                            "rule {} v{} already holds its reviewed limit of {} outstanding \
                             dark-window exception(s); scheduling refused",
                            rule.rule_id, rule.version, dw.max_pending_exceptions
                        )),
                        Some(grant_id),
                        &[],
                        &[],
                    )?;
                    tx.commit()?;
                    return Ok(DarkWindowSchedule::SuppressedAtCap);
                }
                tx.execute(
                    "INSERT INTO standing_rule_pending_actions (
                        pending_id, rule_id, rule_version, task_grant_id, action_id,
                        owner_surface_json, payload_ref_json, dark_window_default,
                        request_fingerprint, requested_at, resolved_at, resolution,
                        reviewed_scope_digest, compatibility_digest
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, NULL, ?11, ?12)",
                    params![
                        pending_id,
                        rule.rule_id,
                        rule.version as i64,
                        grant_id.to_string(),
                        rule.action_id.to_string(),
                        serde_json::to_string(owner_surface)?,
                        payload_json,
                        default_str,
                        fingerprint,
                        timestamp_to_epoch_nanos(now)?,
                        reviewed_scope_digest,
                        compatibility_digest,
                    ],
                )?;
                pending_id
            }
        };
        let existing_timer: Option<(String, Option<i64>)> = tx
            .query_row(
                "SELECT timer_id, applied_at FROM standing_rule_timer_links WHERE pending_id = ?1 LIMIT 1",
                params![effective_pending_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        // `Scheduled` is returned ONLY when a brand-new timer was just
        // inserted; an existing live timer (not yet fired) or a terminal
        // request with no open timer schedules nothing new and returns
        // `AlreadyCovered` (P1-8 stable idempotency — no duplicate timer, no
        // duplicate audit).
        let scheduled = match existing_timer {
            Some((_, None)) => DarkWindowSchedule::AlreadyCovered,
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
                    DarkWindowSchedule::AlreadyCovered
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
                        Some(
                            "standing rule dark-window timer scheduled for a specific pending action",
                        ),
                        Some(grant_id),
                        &[],
                        &[],
                    )?;
                    DarkWindowSchedule::Scheduled(timer_id)
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
            Option<String>,
            Option<String>,
            String,
            String,
            String,
            Option<i64>,
            Option<String>,
        );
        let pending: Option<PendingRow> = tx
            .query_row(
                "SELECT rule_id, rule_version, task_grant_id, action_id, owner_surface_json, \
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
            owner_surface_json,
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
                "standing_rule.exception_fired",
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
            owner_surface: parse_owner_surface(owner_surface_json)?,
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
}

//! Gate-time standing-rule consultation (AD-010, AD-106, AD-012 leaning).
//!
//! This is the new authority+gate path AD-010 warned about: a genuinely
//! new `gate()`-adjacent decision, not a new enum entry. It runs in
//! `mediate_and_dispatch_action` (the shared mediation boundary for HTTP and
//! durable-workflow actions) right after the pure `gate()` decision. A
//! standing rule is a composition INPUT only (D-007): the task grant remains
//! the sole live authority object, so this module never grants authority —
//! it only *reserves a budget* from the runtime `standing_rules` table and
//! reports whether the action may proceed without a fresh owner approval.
//!
//! Quota (volume) and rate (velocity) are independent sliding-window
//! counters; both must have headroom or the action falls back to normal
//! approval. The reserve is atomic with the lookup, expiry check, and action
//! binding in one `BEGIN IMMEDIATE` (D-050 precedent) so a saturated window
//! can never be overspent and a v2 action-swap between lookup and reserve
//! (P1-4 TOCTOU) cannot admit request A under B's budget. The reservation is
//! finalized (committed) only after the effect successfully dispatches, and
//! cancelled on failure, so failed/denied effects do not consume budget
//! (AD-106 failed-effects rule, P1-6). Remaining budget is returned so agents
//! self-adjust without extra round-trips (AD-013/AD-106).

use crate::store::standing_rules::PendingScheduleCtx;
use crate::store::Store;
use jiff::Timestamp;
use openspine_schemas::action::ActionId;

/// Result of consulting standing rules for one live action at dispatch time.
#[derive(Debug, Clone)]
pub struct StandingRuleGateOutcome {
    /// An active, non-expired, non-revoked rule matched this action.
    pub matched: bool,
    /// Budget was available and reserved — the action may proceed without a
    /// fresh owner approval.
    pub allow: bool,
    /// Remaining headroom after this consultation (for the gate response).
    pub quota_remaining: u32,
    pub rate_remaining: u32,
    pub dark_window_scheduled: bool,
    /// The rule carrying the decision (for audit), if any.
    pub rule: Option<crate::store::standing_rules::StandingRule>,
    /// Reservation id when budget was reserved (allow). Finalized on successful
    /// dispatch, cancelled on failure.
    pub reservation_id: Option<String>,
}

impl StandingRuleGateOutcome {
    /// Remaining headroom for the matched rule, returned in the gate response
    /// so agents self-adjust without extra round-trips (AD-013/AD-106).
    /// `None` when no rule matched, so a default `quota_remaining: 0,
    /// rate_remaining: 0` is never reported for an unmatched action.
    pub fn budget_info(&self) -> Option<(u32, u32)> {
        self.rule
            .as_ref()
            .map(|_| (self.quota_remaining, self.rate_remaining))
    }
}

/// Consult the standing-rule gate for `action` at `now`. Returns the outcome;
/// the caller keeps its `GateDecision::ApprovalRequired` if `allow` is false,
/// or flips to allow by consuming (reserving) budget.
///
/// When an over-budget rule with a dark window is consulted and `ctx` is
/// `Some`, a durable D-074 timer is scheduled against the owning rule (no
/// per-request correlation beyond the stable request fingerprint: the fired
/// timer grants a one-time, digest-bound default keyed by the pending action,
/// which the re-dispatch consumes — see the store doc comment for why a
/// default, not a forced re-dispatch, is the honest AD-012 concretization).
pub fn consult_standing_rule_gate(
    store: &Store,
    action: &ActionId,
    now: Timestamp,
    ctx: Option<&PendingScheduleCtx>,
) -> Result<StandingRuleGateOutcome, crate::store::StoreError> {
    let consult = match store.consult_and_reserve_standing_rule(action, now)? {
        Some((rule, reservation_id)) => (rule, reservation_id),
        None => {
            return Ok(StandingRuleGateOutcome {
                matched: false,
                allow: false,
                quota_remaining: 0,
                rate_remaining: 0,
                dark_window_scheduled: false,
                rule: None,
                reservation_id: None,
            });
        }
    };
    let (rule, reservation_id) = consult;
    // Headroom is only computed on an authorized Allow (a reserved budget).
    // Denials must never expose remaining-capacity metadata (AD-013/AD-106
    // calibration is meaningful only after Allow). The post-reservation
    // `standing_rule_remaining` read is a fallible DB read: if it fails, the
    // already-committed reservation is cancelled before returning a generic
    // internal error so no headroom/quota leaks (SrFinalSec retraction fix).
    let (quota_remaining, rate_remaining) = if reservation_id.is_some() {
        match store.standing_rule_remaining(&rule.rule_id, now) {
            Ok(remaining) => remaining,
            Err(err) => {
                if let Some(reservation_id) = reservation_id.as_deref() {
                    if let Err(cancel_err) = store.cancel_standing_rule_reservation(reservation_id)
                    {
                        tracing::error!(
                            error = %cancel_err,
                            reservation_id,
                            "standing-rule reservation cancel failed after headroom lookup error"
                        );
                    }
                }
                return Err(err);
            }
        }
    } else {
        (0, 0)
    };
    match reservation_id {
        Some(reservation_id) => Ok(StandingRuleGateOutcome {
            matched: true,
            allow: true,
            quota_remaining,
            rate_remaining,
            dark_window_scheduled: false,
            rule: Some(rule),
            reservation_id: Some(reservation_id),
        }),
        None => {
            // Budget exhausted. If a dark window is configured and we have a
            // scheduling context, schedule its D-074 timer against the owning
            // rule so the pre-agreed default applies if the owner stays
            // unreachable.
            let dark_window_scheduled = match (&rule.dark_window, ctx) {
                (Some(dw), Some(ctx)) => {
                    let fires_at =
                        now + std::time::Duration::from_secs(dw.timeout_secs.max(0) as u64);
                    store
                        .schedule_standing_rule_dark_window(
                            &rule,
                            ctx.grant_id,
                            ctx.bound_chat_id,
                            ctx.payload_ref.clone(),
                            &ctx.fingerprint,
                            fires_at,
                            now,
                        )?
                        .is_some()
                }
                _ => false,
            };
            Ok(StandingRuleGateOutcome {
                matched: true,
                allow: false,
                quota_remaining,
                rate_remaining,
                dark_window_scheduled,
                rule: Some(rule),
                reservation_id: None,
            })
        }
    }
}

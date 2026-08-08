//! Scope-matched standing-rule admission (mine-and-match-reusable-authority-
//! by-scope, #128).
//!
//! The old path (`consult_and_reserve_standing_rule`) was action-keyed and
//! single-valued. Scoped admission selects over *every* rule active for the
//! resolved context's action, retains only those whose compatibility epoch
//! AND reviewed scope both equal the freshly resolved context's, then admits
//! exactly one, falls back to ordinary owner approval on zero, and fails
//! closed on two or more. Matching runs BEFORE quota/rate reservation and
//! before any dark-window timer is scheduled, and the selected rule identity
//! is bound inside the same `BEGIN IMMEDIATE` so a concurrent activation
//! cannot swap it (P1-4 TOCTOU, extended to the scope key).
//!
//! One comparison implementation: matching hands the rule's persisted
//! [`ReviewedActionScope`] and the freshly resolved context to the canonical
//! [`ReviewedActionScope::compare`] (responsibility-contract), so a corrupt
//! persisted binding surfaces as `ScopeComparison::InvalidReviewedScope` and
//! a drifted declaration axis is caught by the compatibility-epoch prefilter
//! — no parallel comparison routine lives here.

use openspine_schemas::resolved_context::ResolvedActionContext;
use openspine_schemas::reviewed_scope::ScopeComparison;
use openspine_schemas::standing_rule::StandingRuleManifest;
use rusqlite::params;
use rusqlite::TransactionBehavior;
use ulid::Ulid;

use super::standing_rules::{
    rule_from_row, rule_row_from_row, timestamp_to_epoch_nanos, RuleRow, StandingRule,
    RULE_ROW_COLUMNS,
};
use super::{Store, StoreError};
use jiff::Timestamp;

/// Outcome of a scoped consultation: whether a single compatible rule was
/// found, the rule if one was admitted, and its reservation id when budget
/// was reserved.
#[derive(Debug, Clone)]
pub struct ScopedConsultOutcome {
    /// A single compatible rule was found (exactly one match).
    pub matched: bool,
    /// Budget was available and reserved on the matched rule.
    pub allow: bool,
    /// The matched rule (present exactly when `matched`).
    pub rule: Option<StandingRule>,
    /// Reservation id when `allow`.
    pub reservation_id: Option<String>,
    /// True when two or more compatible rules matched — the fail-closed
    /// ambiguous-overlap case. No reservation, no timer.
    pub ambiguous: bool,
    /// Remaining quota after this consultation's reservation, computed inside
    /// the same transaction that reserved it. Meaningful only when `allow`.
    pub quota_remaining: u32,
    /// Remaining rate headroom after this consultation's reservation.
    /// Meaningful only when `allow`.
    pub rate_remaining: u32,
}

impl ScopedConsultOutcome {
    pub fn none() -> Self {
        Self {
            matched: false,
            allow: false,
            rule: None,
            reservation_id: None,
            ambiguous: false,
            quota_remaining: 0,
            rate_remaining: 0,
        }
    }

    /// Headroom is exposed only on an authorized Allow (AD-013/AD-106): a
    /// denial must never leak remaining-capacity metadata.
    pub fn budget_info(&self) -> Option<(u32, u32)> {
        self.allow
            .then_some((self.quota_remaining, self.rate_remaining))
    }
}

impl Store {
    /// Every active rule for one action, with its scope binding, for
    /// proposal-time overlap evaluation (#133). Read-only: unlike
    /// `consult_and_reserve_scoped_rule` this lapses nothing, reserves
    /// nothing, and writes nothing — evaluation must not move runtime state.
    /// Rows whose lapse deadline has passed are filtered out in memory (the
    /// consult path would mark them `needs_review`; evaluation must not), so a
    /// lapsed rule is not reported as an overlap incumbent.
    pub fn active_standing_rules_for_action(
        &self,
        action_id: &openspine_schemas::action::ActionId,
        now: Timestamp,
    ) -> Result<Vec<StandingRule>, StoreError> {
        let now_nanos = timestamp_to_epoch_nanos(now)?;
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {RULE_ROW_COLUMNS} FROM standing_rules \
             WHERE action_id = ?1 AND status = 'active' \
             ORDER BY version DESC"
        ))?;
        let mut rules = Vec::new();
        let mut query = stmt.query(params![action_id.to_string()])?;
        while let Some(row) = query.next()? {
            let rule = rule_from_row(rule_row_from_row(row)?, "active")?;
            let reference =
                timestamp_to_epoch_nanos(rule.last_used_at.unwrap_or(rule.activated_at))?;
            if reference + rule.expires_after_secs * 1_000_000_000 <= now_nanos {
                continue;
            }
            rules.push(rule);
        }
        Ok(rules)
    }

    /// Select exactly one compatible scoped rule for `context` and reserve its
    /// budget atomically. Returns the classification:
    ///
    /// - 0 compatible rules → `ScopedConsultOutcome::none()` (ordinary owner
    ///   approval; no reservation, no timer).
    /// - 1 compatible rule with headroom → `matched + allow` with a
    ///   reservation id.
    /// - 1 compatible rule with saturated budget → `matched`, `allow=false`
    ///   (the caller may schedule a dark-window timer or fall back).
    /// - 2+ compatible rules → `matched=false, ambiguous=true`, no
    ///   reservation: an ambiguous overlap is an unreviewed authority
    ///   question, never a tie to break.
    ///
    /// Selection is pure until exactly one rule is chosen; nothing is written
    /// for a zero- or two-or-more outcome.
    pub fn consult_and_reserve_scoped_rule(
        &self,
        context: &ResolvedActionContext,
        now: Timestamp,
    ) -> Result<ScopedConsultOutcome, StoreError> {
        let now_nanos = timestamp_to_epoch_nanos(now)?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(counterparty_id) = context.counterparty_identity_id() {
            let erased: i64 = tx.query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM erased_counterparties WHERE counterparty_id = ?1
                 )",
                params![counterparty_id.to_string()],
                |row| row.get(0),
            )?;
            if erased != 0 {
                Self::append_audit_conn(
                    &tx,
                    "action.scope_context_unresolved",
                    Some(context.action_id()),
                    None,
                    Some(
                        "counterparty was erased before scoped rule selection; ordinary owner review required",
                    ),
                    None,
                    &[],
                    &[],
                )?;
                tx.commit()?;
                return Ok(ScopedConsultOutcome::none());
            }
        }

        let rows: Vec<RuleRow> = {
            let mut stmt = tx.prepare(&format!(
                "SELECT {RULE_ROW_COLUMNS} FROM standing_rules \
                 WHERE action_id = ?1 AND status = 'active' \
                 ORDER BY version DESC"
            ))?;
            let mut collected = Vec::new();
            {
                let mut query = stmt.query(params![context.action_id().to_string()])?;
                while let Some(row) = query.next()? {
                    collected.push(rule_row_from_row(row)?);
                }
            }
            collected
        };

        let mut matches: Vec<StandingRule> = Vec::new();
        for row in rows {
            let rule = rule_from_row(row, "active")?;
            // Expiry (last_used_at fallback to activation) lapses a rule the
            // instant `now` reaches its deadline — matching the canonical
            // matcher; an expired rule is excluded here and marked needs_review.
            let reference =
                timestamp_to_epoch_nanos(rule.last_used_at.unwrap_or(rule.activated_at))?;
            if reference + rule.expires_after_secs * 1_000_000_000 <= now_nanos {
                // #135: a lapsed rule leaves no fireable exception behind.
                super::standing_rules_exceptions::stale_pending_exceptions_in_tx(
                    &tx,
                    &rule.rule_id,
                    Some(rule.version),
                    now_nanos,
                )?;
                tx.execute(
                    "UPDATE standing_rules SET status = 'needs_review', needs_review_since = ?2 \
                     WHERE rule_id = ?1 AND status = 'active'",
                    params![rule.rule_id, now_nanos],
                )?;
                continue;
            }
            if !scoped_rule_matches(&rule, context) {
                continue;
            }
            matches.push(rule);
            // Selection must complete before any budget moves: a second
            // compatible rule is an ambiguity, not a race to reserve.
            if matches.len() > 1 {
                break;
            }
        }

        // Fail closed on ambiguity: no reservation, no timer, ordinary
        // approval, and durable owner-actionable evidence that two approved
        // responsibilities collide.
        if matches.len() > 1 {
            Self::append_audit_conn(
                &tx,
                "standing_rule.ambiguous_scope_overlap",
                Some(context.action_id()),
                None,
                Some("two or more active rules matched one resolved scope; admission refused"),
                None,
                &[],
                &[],
            )?;
            tx.commit()?;
            return Ok(ScopedConsultOutcome {
                matched: false,
                allow: false,
                rule: None,
                reservation_id: None,
                ambiguous: true,
                quota_remaining: 0,
                rate_remaining: 0,
            });
        }

        let Some(rule) = matches.into_iter().next() else {
            tx.commit()?;
            return Ok(ScopedConsultOutcome::none());
        };

        // Exactly one compatible rule: reserve quota+rate inside the same
        // transaction the selection ran in (headroom counts reserved+committed
        // so a concurrent in-flight reservation is never overspent).
        let quota_used: i64 = tx.query_row(
            "SELECT COUNT(*) FROM standing_rule_usage \
             WHERE rule_id = ?1 AND kind = 'quota' \
               AND status IN ('reserved', 'committed') AND used_at >= ?2",
            params![
                rule.rule_id,
                now_nanos - rule.quota.window_secs * 1_000_000_000
            ],
            |r| r.get(0),
        )?;
        let rate_used: i64 = tx.query_row(
            "SELECT COUNT(*) FROM standing_rule_usage \
             WHERE rule_id = ?1 AND kind = 'rate' \
               AND status IN ('reserved', 'committed') AND used_at >= ?2",
            params![
                rule.rule_id,
                now_nanos - rule.rate.window_secs * 1_000_000_000
            ],
            |r| r.get(0),
        )?;
        let reservation_id = if quota_used < rule.quota.max as i64
            && rate_used < rule.rate.max as i64
        {
            let reservation_id = Ulid::new().to_string();
            tx.execute(
                "INSERT INTO standing_rule_usage (rule_id, version, kind, used_at, status, reservation_id)
                 VALUES (?1, ?2, 'quota', ?3, 'reserved', ?4),
                        (?1, ?2, 'rate', ?3, 'reserved', ?4)",
                params![rule.rule_id, rule.version as i64, now_nanos, reservation_id],
            )?;
            Some(reservation_id)
        } else {
            None
        };
        tx.commit()?;
        // Headroom is computed from the same in-transaction counts that
        // authorized the reservation, so it never needs a second fallible
        // read after commit.
        let (quota_remaining, rate_remaining) = if reservation_id.is_some() {
            (
                (rule.quota.max as i64 - quota_used - 1).max(0) as u32,
                (rule.rate.max as i64 - rate_used - 1).max(0) as u32,
            )
        } else {
            (0, 0)
        };
        Ok(ScopedConsultOutcome {
            matched: true,
            allow: reservation_id.is_some(),
            rule: Some(rule),
            reservation_id,
            ambiguous: false,
            quota_remaining,
            rate_remaining,
        })
    }
}

/// Whether a persisted rule's scope binding matches a freshly resolved
/// context. Uses the canonical [`ReviewedActionScope::compare`]: both the
/// compatibility epoch (SQL fast-path) and the reviewed scope must match, and
/// a corrupt persisted binding (stored values disagreeing with the stored
/// digest) fails closed as invalid scope rather than matching on either half.
fn scoped_rule_matches(rule: &StandingRule, context: &ResolvedActionContext) -> bool {
    // Fast SQL pre-filter: the bound compatibility epoch must equal the
    // freshly resolved context's, and the bound scope-key digest must equal
    // the context's. A rule with no scope binding is not eligible for scoped
    // admission.
    let (Some(compat), Some(scope_digest)) =
        (&rule.compatibility_digest, &rule.reviewed_scope_digest)
    else {
        return false;
    };
    if compat != context.compatibility_digest() {
        return false;
    }
    let Some(context_scope_digest) = context.reviewed_scope_digest() else {
        return false;
    };
    if scope_digest != &context_scope_digest {
        return false;
    }
    // The digest columns are a fast pre-filter, not authoritative: deserialize
    // the persisted binding and hand it to the canonical comparison so a
    // persisted disagreement with the stored digest (corrupt binding) is an
    // invalid scope, never a match on either half.
    let manifest: StandingRuleManifest = match serde_json::from_str(&rule.rule_json) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let Some(binding) = manifest.reviewed_scope else {
        return false;
    };
    if !binding.binding_is_valid() {
        return false;
    }
    matches!(binding.scope.compare(context), ScopeComparison::Matches)
}

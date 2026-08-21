//! The bounded dark-window exception primitives (#135): the typed scheduling
//! outcome, and the staleness writer every lifecycle change uses. Split from
//! `standing_rules_pending.rs` to keep both files under the 500-line gate.

use rusqlite::params;

use super::{Store, StoreError};

/// The outcome of trying to schedule a dark-window exception (#135).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DarkWindowSchedule {
    /// A brand-new live dark window was created; carries its timer id.
    Scheduled(String),
    /// An existing open timer already covers this request, or the request is
    /// already terminal. Nothing new was created and nothing was consumed.
    AlreadyCovered,
    /// Refused at the rule's reviewed `max_pending_exceptions` limit: no
    /// pending row, no timer, no scheduled evidence. The caller MUST leave the
    /// action at ordinary owner approval and MUST NOT report a pending
    /// default.
    SuppressedAtCap,
}

impl DarkWindowSchedule {
    /// The freshly scheduled timer id, if this attempt created one.
    pub fn timer_id(&self) -> Option<&str> {
        match self {
            Self::Scheduled(timer_id) => Some(timer_id),
            _ => None,
        }
    }

    /// Whether a live dark window now covers the request — either just
    /// scheduled or already open. Suppression is not coverage.
    pub fn is_covered(&self) -> bool {
        matches!(self, Self::Scheduled(_) | Self::AlreadyCovered)
    }
}

/// Mark every unresolved pending exception for one rule version stale, so it
/// can never fire (#135). Runs on the caller's transaction, because a
/// staleness write that could be lost relative to the lifecycle change that
/// caused it would look like protection without being it.
///
/// `claim_standing_rule_dark_window` already treats `stale` as terminal and
/// grants no authority; what was missing was anything that wrote it. Because
/// stale rows are resolved, they also stop occupying a cap slot — a revoked
/// rule's exceptions are not outstanding.
pub(super) fn stale_pending_exceptions_in_tx(
    tx: &rusqlite::Transaction<'_>,
    rule_id: &str,
    rule_version: Option<u32>,
    now_nanos: i64,
) -> Result<usize, StoreError> {
    let changed = match rule_version {
        Some(version) => tx.execute(
            "UPDATE standing_rule_pending_actions \
             SET resolved_at = ?3, resolution = 'stale' \
             WHERE rule_id = ?1 AND rule_version = ?2 AND resolved_at IS NULL",
            params![rule_id, version as i64, now_nanos],
        )?,
        None => tx.execute(
            "UPDATE standing_rule_pending_actions \
             SET resolved_at = ?2, resolution = 'stale' \
             WHERE rule_id = ?1 AND resolved_at IS NULL",
            params![rule_id, now_nanos],
        )?,
    };
    if changed > 0 {
        Store::append_audit_conn(
            tx,
            "standing_rule.pending_exceptions_staled",
            None,
            None,
            Some(&format!(
                "{changed} unresolved dark-window exception(s) for rule {rule_id} marked stale by \
                 a lifecycle change; none may fire"
            )),
            None,
            &[],
            &[],
        )?;
    }
    Ok(changed)
}

impl Store {
    /// Retire any ACTIVE rule whose stored dark-window default is `Allow` for
    /// an action the catalog does not declare eligible (#135, D-162).
    ///
    /// Both activation entry points are guarded, but the guard is not
    /// retroactive: a rule activated before it existed — or before an action
    /// was removed from the eligibility allowlist — stays active and stays
    /// fireable, because rule hydration reconstructs `DarkWindowConfig` from
    /// the stored columns without re-checking eligibility. This sweep is what
    /// makes the prohibition true of the database and not merely of new
    /// activations. Each offending rule moves to `needs_review` and has its
    /// unresolved exceptions staled in the same transaction, so nothing it
    /// already scheduled can still fire.
    ///
    /// Runs at open, alongside the other startup convergence work.
    pub fn sweep_ineligible_dark_window_allow_rules(
        &self,
        now: jiff::Timestamp,
    ) -> Result<usize, StoreError> {
        let now_nanos: i64 = now
            .as_nanosecond()
            .try_into()
            .map_err(|_| StoreError::TimestampRange("timestamp out of i64 range".into()))?;
        self.with_immediate_tx(|tx| {
            let offenders: Vec<(String, i64, String)> = {
                let mut stmt = tx.prepare(
                    "SELECT rule_id, version, action_id FROM standing_rules \
                 WHERE status = 'active' AND dark_window_default = 'allow'",
                )?;
                let mut rows = stmt.query([])?;
                let mut collected = Vec::new();
                while let Some(row) = rows.next()? {
                    collected.push((row.get(0)?, row.get(1)?, row.get(2)?));
                }
                collected
            };
            let mut retired = 0usize;
            for (rule_id, version, action_id) in offenders {
                if crate::action_catalog::dark_window_allow_eligible(
                    &openspine_schemas::action::ActionId::new(&action_id),
                ) {
                    continue;
                }
                stale_pending_exceptions_in_tx(
                    tx,
                    &rule_id,
                    u32::try_from(version).ok(),
                    now_nanos,
                )?;
                tx.execute(
                    "UPDATE standing_rules SET status = 'needs_review', needs_review_since = ?2 \
                 WHERE rule_id = ?1 AND status = 'active'",
                    rusqlite::params![rule_id, now_nanos],
                )?;
                Store::append_audit_conn(
                    tx,
                    "standing_rule.ineligible_allow_retired",
                    Some(&openspine_schemas::action::ActionId::new(&action_id)),
                    None,
                    Some(&format!(
                    "rule {rule_id} carries a dark-window Allow default for {action_id}, which the \
                     catalog does not declare eligible; moved to needs_review and its pending \
                     exceptions staled"
                )),
                    None,
                    &[],
                    &[],
                )?;
                retired += 1;
            }
            Ok(retired)
        })
    }
}

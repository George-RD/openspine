//! Shared `standing_rules` row projection and hydration, split from
//! `standing_rules.rs` to keep every file under the 500-line gate.
//!
//! Every rule reader (`active_standing_rule_for_action`,
//! `consult_and_reserve_standing_rule`, scoped matching) selects the same
//! 18-column projection and hydrates a [`StandingRule`] from it, so the
//! column order, the digest parse, and the epoch-nanos↔Timestamp conversion
//! live in exactly one place.

use openspine_schemas::action::ActionId;
use openspine_schemas::standing_rule::{BudgetWindow, DarkWindowConfig, DarkWindowDefault};

use super::StoreError;
use jiff::Timestamp;

/// One active standing rule plus its budget configuration. Usage counters
/// live in `standing_rule_usage` (see `standing_rules_budget.rs`).
#[derive(Debug, Clone)]
pub struct StandingRule {
    pub rule_id: String,
    pub artifact_id: String,
    pub version: u32,
    pub action_id: ActionId,
    pub rule_json: String,
    pub quota: BudgetWindow,
    pub rate: BudgetWindow,
    pub expires_after_secs: i64,
    pub dark_window: Option<DarkWindowConfig>,
    pub status: String,
    pub activated_at: Timestamp,
    pub last_used_at: Option<Timestamp>,
    pub revoked_at: Option<Timestamp>,
    pub needs_review_since: Option<Timestamp>,
    /// Fast SQL pre-filter for scoped matching, mirrored from the manifest's
    /// `reviewed_scope` binding. `None` for a legacy unbounded rule.
    pub reviewed_scope_digest: Option<openspine_schemas::digest::Digest>,
    pub compatibility_digest: Option<openspine_schemas::digest::Digest>,
}

/// The 19-column `standing_rules` row projection shared by every rule reader.
/// Kept in lockstep with [`RULE_ROW_COLUMNS`] and [`rule_row_from_row`].
pub(super) type RuleRow = (
    String,
    String,
    i64,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    i64,
    Option<i64>,
    Option<String>,
    i64,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<i64>,
);

pub(super) const RULE_ROW_COLUMNS: &str = "rule_id, artifact_id, version, action_id, rule_json, \
    quota_max, quota_window_secs, rate_max, rate_window_secs, \
    expires_after_secs, dark_window_timeout_secs, dark_window_default, \
    activated_at, last_used_at, revoked_at, needs_review_since, \
    reviewed_scope_digest, compatibility_digest, dark_window_max_pending";

pub(super) fn rule_row_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RuleRow> {
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
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
        row.get(14)?,
        row.get(15)?,
        row.get(16)?,
        row.get(17)?,
        row.get(18)?,
    ))
}

/// Hydrate a [`StandingRule`] from a shared [`RuleRow`] projection.
pub(super) fn rule_from_row(row: RuleRow, status: &str) -> Result<StandingRule, StoreError> {
    let (
        rule_id,
        artifact_id,
        version,
        action_str,
        rule_json,
        quota_max,
        quota_window_secs,
        rate_max,
        rate_window_secs,
        expires_after_secs,
        dark_window_timeout_secs,
        dark_window_default,
        activated_at,
        last_used_at,
        revoked_at,
        needs_review_since,
        reviewed_scope_digest,
        compatibility_digest,
        dark_window_max_pending,
    ) = row;
    let dark_window = dark_window_timeout_secs.map(|timeout_secs| DarkWindowConfig {
        timeout_secs,
        default: if dark_window_default.as_deref() == Some("allow") {
            DarkWindowDefault::Allow
        } else {
            DarkWindowDefault::Deny
        },
        // A legacy row written before #135 carries NULL and reads back as the
        // safe default of one outstanding exception, never as unbounded.
        max_pending_exceptions: dark_window_max_pending
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
            .unwrap_or_else(openspine_schemas::standing_rule::default_max_pending_exceptions),
    });
    Ok(StandingRule {
        rule_id,
        artifact_id,
        version: version as u32,
        action_id: ActionId::new(&action_str),
        rule_json,
        quota: BudgetWindow {
            max: quota_max as u32,
            window_secs: quota_window_secs,
        },
        rate: BudgetWindow {
            max: rate_max as u32,
            window_secs: rate_window_secs,
        },
        expires_after_secs,
        dark_window,
        status: status.to_string(),
        activated_at: epoch_nanos_to_timestamp(activated_at)?,
        last_used_at: last_used_at.map(epoch_nanos_to_timestamp).transpose()?,
        revoked_at: revoked_at.map(epoch_nanos_to_timestamp).transpose()?,
        needs_review_since: needs_review_since
            .map(epoch_nanos_to_timestamp)
            .transpose()?,
        reviewed_scope_digest: reviewed_scope_digest
            .map(|d| openspine_schemas::digest::Digest::parse(&d))
            .transpose()
            .map_err(|_| StoreError::FailureRouting("invalid reviewed_scope_digest".into()))?,
        compatibility_digest: compatibility_digest
            .map(|d| openspine_schemas::digest::Digest::parse(&d))
            .transpose()
            .map_err(|_| StoreError::FailureRouting("invalid compatibility_digest".into()))?,
    })
}

pub(super) fn timestamp_to_epoch_nanos(timestamp: Timestamp) -> Result<i64, StoreError> {
    i64::try_from(timestamp.as_nanosecond())
        .map_err(|_| StoreError::TimestampRange(format!("{} out of i64 nanos", timestamp)))
}

pub(super) fn epoch_nanos_to_timestamp(nanos: i64) -> Result<Timestamp, StoreError> {
    Timestamp::from_nanosecond(nanos.into())
        .map_err(|err| StoreError::TimestampRange(format!("{nanos} ns: {err}")))
}

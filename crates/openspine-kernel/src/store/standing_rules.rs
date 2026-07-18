//! Standing-rule runtime storage (AD-010, AD-106, AD-012 leaning).
//!
//! Standing rules are authority-composition INPUTS, never a second live
//! authority object (D-007): the task grant remains the only live authority.
//! Budget reservation/finalize/release and drift detection live in
//! `standing_rules_budget.rs`; the durable dark-window pending-action state
//! machine (schedule / claim / owner-resolve / fired-token-consume /
//! recovery) lives in `standing_rules_pending.rs` (split out to keep every
//! file under the 500-line gate).
//!
//! AD-012 dark-window defaults bind to a durable *pending action* — "if you
//! don't respond in 30 min, I take pre-agreed default X" means X applies to
//! the *specific* action that timed out. The pending action stores only an
//! encrypted `ArtifactRef` to the action payload (never plaintext), is
//! deduplicated per stable request fingerprint, and is resolved either by
//! owner silence (the fired timer) or by an explicit owner decision.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::standing_rule::{
    BudgetWindow, DarkWindowConfig, DarkWindowDefault, StandingRuleManifest,
};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use ulid::Ulid;

use super::{Store, StoreError};

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
}

/// A pending action a standing rule's dark window will resolve if the owner
/// stays silent — or that the owner may resolve explicitly first. Carries an
/// encrypted `ArtifactRef` to the action payload, the stable request
/// `fingerprint` used for deduplication, and the durable dispatch-state
/// machine (`none` → `claimed`/`dispatched`) that makes the fired default
/// recoverable and the one-use fired token digest-bound (P1-4/P1-7/P1-10).
#[derive(Debug, Clone)]
pub struct StandingRulePendingAction {
    pub pending_id: String,
    pub rule_id: String,
    pub rule_version: u32,
    pub task_grant_id: Ulid,
    pub action_id: ActionId,
    pub bound_chat_id: i64,
    pub payload_ref: Option<ArtifactRef>,
    pub default: DarkWindowDefault,
    /// Stable per-request identity (action+grant+chat+payload digest): two
    /// identical retries collapse onto one pending action; distinct requests
    /// keep separate pending defaults.
    pub request_fingerprint: String,
    /// `none` before a timer claim, `claimed` after the one-use fired token is
    /// consumed but before the connector effect, and `dispatched` after the
    /// effect is durably attempted. Recovery surfaces `claimed` rows for owner
    /// attention and never retries them; `dispatched` is likewise fail-closed
    /// because the external effect may already have run.
    pub dispatch_state: String,
    /// Set when the owner (or the fired default) decides `allowed`, before any
    /// side effect — the moment the default is durably decided.
    pub resolved_at: Option<Timestamp>,
    pub resolution: Option<String>,
}

/// Context needed to schedule a dark-window timer when an over-budget
/// consultation would otherwise fall back to owner approval. `None` (e.g. in
/// unit tests) means "schedule the row but skip owner notification".
pub struct PendingScheduleCtx {
    pub bound_chat_id: i64,
    pub grant_id: Ulid,
    pub payload_ref: Option<ArtifactRef>,
    pub fingerprint: String,
}

pub fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS standing_rules (
            rule_id TEXT PRIMARY KEY,
            artifact_id TEXT NOT NULL,
            version INTEGER NOT NULL,
            action_id TEXT NOT NULL,
            rule_json TEXT NOT NULL,
            quota_max INTEGER NOT NULL,
            quota_window_secs INTEGER NOT NULL,
            rate_max INTEGER NOT NULL,
            rate_window_secs INTEGER NOT NULL,
            expires_after_secs INTEGER NOT NULL,
            dark_window_timeout_secs INTEGER,
            dark_window_default TEXT,
            status TEXT NOT NULL,
            activated_at INTEGER NOT NULL,
            last_used_at INTEGER,
            revoked_at INTEGER,
            needs_review_since INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_standing_rules_action
            ON standing_rules (action_id, status);
        CREATE TABLE IF NOT EXISTS standing_rule_usage (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            rule_id TEXT NOT NULL,
            version INTEGER NOT NULL,
            kind TEXT NOT NULL CHECK(kind IN ('quota', 'rate')),
            used_at INTEGER NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('reserved', 'committed', 'waiver')),
            reservation_id TEXT NOT NULL,
            FOREIGN KEY(rule_id) REFERENCES standing_rules(rule_id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_standing_rule_usage_window
            ON standing_rule_usage (rule_id, kind, status, used_at);
        CREATE TABLE IF NOT EXISTS standing_rule_pending_actions (
            pending_id TEXT PRIMARY KEY,
            rule_id TEXT NOT NULL,
            rule_version INTEGER NOT NULL,
            task_grant_id TEXT NOT NULL,
            action_id TEXT NOT NULL,
            bound_chat_id INTEGER NOT NULL,
            payload_ref_json TEXT,
            dark_window_default TEXT NOT NULL CHECK(dark_window_default IN ('allow', 'deny')),
            request_fingerprint TEXT NOT NULL,
            requested_at INTEGER NOT NULL,
            resolved_at INTEGER,
            resolution TEXT CHECK(resolution IN ('allowed', 'denied', 'stale')),
            dispatch_state TEXT NOT NULL DEFAULT 'none'
                CHECK(dispatch_state IN ('none', 'claimed', 'dispatched')),
            token_consumed_at INTEGER,
            dispatch_receipt_digest TEXT,
            owner_attention_since INTEGER,
            FOREIGN KEY(rule_id) REFERENCES standing_rules(rule_id) ON DELETE CASCADE
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_standing_rule_pending_fingerprint
            ON standing_rule_pending_actions (rule_id, rule_version, request_fingerprint);
        CREATE TABLE IF NOT EXISTS standing_rule_timer_links (
            timer_id TEXT PRIMARY KEY,
            pending_id TEXT NOT NULL,
            applied_at INTEGER,
            FOREIGN KEY(pending_id) REFERENCES standing_rule_pending_actions(pending_id) ON DELETE CASCADE
        );",
    )?;
    Ok(())
}

impl Store {
    /// Activate (or re-activate a higher version of) a standing rule.
    /// Idempotent per (artifact_id, version) via `INSERT OR REPLACE` keyed
    /// on `rule_id == artifact_id`. Validation runs first (defense in depth).
    pub fn activate_standing_rule(
        &self,
        manifest: &StandingRuleManifest,
        grant_id: Option<Ulid>,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        Self::activate_standing_rule_in_tx(&tx, manifest, grant_id, now)?;
        tx.commit()?;
        Ok(())
    }

    /// Variant that writes the runtime row inside an *existing* transaction,
    /// so the generic artifact-activation commit (proposal lifecycle + audit)
    /// and the standing-rule runtime row land atomically — closing the
    /// post-commit crash gap (P1-12).
    pub(super) fn activate_standing_rule_in_tx(
        tx: &Transaction<'_>,
        manifest: &StandingRuleManifest,
        grant_id: Option<Ulid>,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        if let Err(reason) = manifest.validate() {
            return Err(StoreError::ProposedArtifactLifecycle(format!(
                "standing_rule manifest invalid: {reason}"
            )));
        }
        let dark_timeout = manifest.dark_window.map(|d| d.timeout_secs);
        let dark_default = manifest.dark_window.map(|d| match d.default {
            DarkWindowDefault::Allow => "allow",
            DarkWindowDefault::Deny => "deny",
        });
        let rule_json = serde_json::to_string(manifest)?;
        let now_nanos = timestamp_to_epoch_nanos(now)?;
        let current_version: Option<i64> = tx
            .query_row(
                "SELECT version FROM standing_rules WHERE rule_id = ?1",
                params![manifest.id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(current_version) = current_version {
            if (manifest.version as i64) < current_version {
                return Err(StoreError::FailureRouting(format!(
                    "standing rule activation version {} is older than current {}",
                    manifest.version, current_version
                )));
            }
            if (manifest.version as i64) == current_version {
                return Ok(());
            }
        }
        Self::append_audit_conn(
            tx,
            "standing_rule.activated",
            Some(&manifest.action_id),
            None,
            Some("standing rule became an active gate consultation input"),
            grant_id,
            &[],
            &[],
        )?;
        tx.execute(
            "UPDATE standing_rules SET status = 'revoked', revoked_at = ?3 \
             WHERE action_id = ?1 AND rule_id != ?2 AND status = 'active'",
            params![manifest.action_id.to_string(), manifest.id, now_nanos],
        )?;
        tx.execute(
            "INSERT INTO standing_rules (
                rule_id, artifact_id, version, action_id, rule_json,
                quota_max, quota_window_secs, rate_max, rate_window_secs,
                expires_after_secs, dark_window_timeout_secs, dark_window_default,
                status, activated_at, last_used_at, revoked_at, needs_review_since
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'active', ?13, NULL, NULL, NULL)
             ON CONFLICT(rule_id) DO UPDATE SET
                artifact_id = excluded.artifact_id,
                version = excluded.version,
                action_id = excluded.action_id,
                rule_json = excluded.rule_json,
                quota_max = excluded.quota_max,
                quota_window_secs = excluded.quota_window_secs,
                rate_max = excluded.rate_max,
                rate_window_secs = excluded.rate_window_secs,
                expires_after_secs = excluded.expires_after_secs,
                dark_window_timeout_secs = excluded.dark_window_timeout_secs,
                dark_window_default = excluded.dark_window_default,
                status = excluded.status,
                activated_at = excluded.activated_at,
                last_used_at = excluded.last_used_at,
                revoked_at = excluded.revoked_at,
                needs_review_since = excluded.needs_review_since
             WHERE excluded.version > standing_rules.version",
            params![
                manifest.id,
                manifest.id,
                manifest.version as i64,
                manifest.action_id.to_string(),
                rule_json,
                manifest.quota.max as i64,
                manifest.quota.window_secs,
                manifest.rate.max as i64,
                manifest.rate.window_secs,
                manifest.expires_after_secs,
                dark_timeout,
                dark_default,
                now_nanos,
            ],
        )?;
        Ok(())
    }

    /// Revoke (versioned) a standing rule — makes it invisible to gate
    /// consultation immediately. Idempotent: revoking an already-revoked or
    /// unknown rule is `Ok(false)`.
    pub fn revoke_standing_rule(&self, rule_id: &str, now: Timestamp) -> Result<bool, StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::append_audit_conn(
            &tx,
            "standing_rule.revoked",
            None,
            None,
            Some(rule_id),
            None,
            &[],
            &[],
        )?;
        let changed = tx.execute(
            "UPDATE standing_rules SET status = 'revoked', revoked_at = ?2 \
             WHERE rule_id = ?1 AND status != 'revoked'",
            params![rule_id, timestamp_to_epoch_nanos(now)?],
        )?;
        tx.commit()?;
        Ok(changed == 1)
    }

    /// Find the single active, non-expired, non-revoked rule for an action.
    /// Expiry is computed at lookup time (a rule lapses when it has not been
    /// used within `expires_after_secs` of its last use). Returns `None`
    /// when no live rule matches — the signal for the caller to require
    /// normal owner approval.
    pub fn active_standing_rule_for_action(
        &self,
        action_id: &ActionId,
        now: Timestamp,
    ) -> Result<Option<StandingRule>, StoreError> {
        let conn = self.conn.lock();
        type Row = (
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
        );
        let row: Option<Row> = conn
            .query_row(
                "SELECT rule_id, artifact_id, version, action_id, rule_json,
                        quota_max, quota_window_secs, rate_max, rate_window_secs,
                        expires_after_secs, dark_window_timeout_secs, dark_window_default,
                        activated_at, last_used_at, revoked_at, needs_review_since
                 FROM standing_rules
                 WHERE action_id = ?1 AND status = 'active'
                 ORDER BY version DESC
                 LIMIT 1",
                params![action_id.to_string()],
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
                        row.get(11)?,
                        row.get(12)?,
                        row.get(13)?,
                        row.get(14)?,
                        row.get(15)?,
                    ))
                },
            )
            .optional()?;

        let Some((
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
            _revoked_at,
            needs_review_since,
        )) = row
        else {
            return Ok(None);
        };
        let reference = last_used_at.unwrap_or(activated_at);
        let deadline = reference + expires_after_secs * 1_000_000_000;
        // Canonical exact-deadline boundary: a rule lapses the instant `now`
        // reaches `deadline` (i.e. `deadline <= now`), matching the strict
        // fired-token SQL (`elapsed < expiry`) and the atomic consult path.
        if deadline <= timestamp_to_epoch_nanos(now)? {
            conn.execute(
                "UPDATE standing_rules SET status = 'needs_review', needs_review_since = ?2 \
                 WHERE rule_id = ?1 AND status = 'active'",
                params![rule_id, timestamp_to_epoch_nanos(now)?],
            )?;
            return Ok(None);
        }

        let dark_window = dark_window_timeout_secs.map(|timeout_secs| DarkWindowConfig {
            timeout_secs,
            default: if dark_window_default.as_deref() == Some("allow") {
                DarkWindowDefault::Allow
            } else {
                DarkWindowDefault::Deny
            },
        });
        Ok(Some(StandingRule {
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
            status: "active".to_string(),
            activated_at: epoch_nanos_to_timestamp(activated_at)?,
            last_used_at: last_used_at.map(epoch_nanos_to_timestamp).transpose()?,
            revoked_at: None,
            needs_review_since: needs_review_since
                .map(epoch_nanos_to_timestamp)
                .transpose()?,
        }))
    }

    /// Whether `rule_id` at exactly `version` is still the current active
    /// rule — used to reject a stale dark-window fire or to catch a v2
    /// action-swap between consult and finalize (P1-4).
    pub fn standing_rule_is_current(
        &self,
        rule_id: &str,
        version: u32,
    ) -> Result<bool, StoreError> {
        let conn = self.conn.lock();
        let found: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM standing_rules WHERE rule_id = ?1 AND version = ?2 AND status = 'active'",
                params![rule_id, version as i64],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }
}

pub(super) fn timestamp_to_epoch_nanos(timestamp: Timestamp) -> Result<i64, StoreError> {
    i64::try_from(timestamp.as_nanosecond())
        .map_err(|_| StoreError::TimestampRange(format!("{} out of i64 nanos", timestamp)))
}

pub(super) fn epoch_nanos_to_timestamp(nanos: i64) -> Result<Timestamp, StoreError> {
    Timestamp::from_nanosecond(nanos.into())
        .map_err(|err| StoreError::TimestampRange(format!("{nanos} ns: {err}")))
}

/// Stable per-request identity for dark-window deduplication. Mirrors the
/// `GatedStepDigest` inputs so a fired token re-checked against the same
/// (action, grant, chat, payload) is accepted and any other request is not.
pub fn standing_rule_fingerprint(
    action: &ActionId,
    grant_id: Ulid,
    bound_chat_id: i64,
    payload_ref: &Option<ArtifactRef>,
) -> String {
    let payload_key = payload_ref
        .as_ref()
        .map(|r| r.digest.as_str().to_string())
        .unwrap_or_default();
    openspine_schemas::digest::digest_of_bytes(
        format!("{action}|{grant_id}|{bound_chat_id}|{payload_key}").as_bytes(),
    )
    .as_str()
    .to_string()
}

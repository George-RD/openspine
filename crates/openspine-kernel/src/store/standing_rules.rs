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
use openspine_schemas::standing_rule::{DarkWindowDefault, StandingRuleManifest};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use ulid::Ulid;

use super::{Store, StoreError};

/// One active standing rule plus its budget configuration. Usage counters
/// live in `standing_rule_usage` (see `standing_rules_budget.rs`).
pub use crate::store::standing_rules_row::StandingRule;

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

/// Why a standing rule may not be activated for its action, or `None` when its
/// reviewed-scope binding is complete (or the action's descriptor declares no
/// required dimensions at all).
///
/// This is the enforcement point the standing-rules spec names — "a rule
/// missing any required dimension MUST be rejected **before activation**" —
/// and it lives here rather than in [`StandingRuleManifest::validate`] because
/// the required dimension set lives on the catalog's `ActionDescriptor`, which
/// a self-contained manifest cannot reach. `validate` keeps the check it *can*
/// perform (the stored digest agreeing with the stored values).
///
/// Refusing at activation is what keeps the store's contents honest: without
/// it an incomplete binding is caught only at match time by the scope-key
/// pre-filter, leaving a malformed rule sitting in the store looking active
/// while being silently unmatchable.
pub(crate) fn scope_binding_rejection(manifest: &StandingRuleManifest) -> Option<String> {
    let required = crate::action_catalog::required_scope_dimensions_for(&manifest.action_id)?;
    let Some(binding) = manifest.reviewed_scope.as_ref() else {
        return Some(format!(
            "standing rule for {} must bind a reviewed scope: its descriptor declares {} required \
             dimension(s)",
            manifest.action_id,
            required.len()
        ));
    };
    let missing = required
        .iter()
        .filter(|dimension| !binding.scope.dimensions().contains_key(dimension))
        .map(|dimension| format!("{dimension:?}"))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Some(format!(
            "standing rule for {} omits required reviewed scope dimension(s): {}",
            manifest.action_id,
            missing.join(", ")
        ));
    }
    None
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
        self.reject_incomplete_scope_binding(manifest)?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        Self::activate_standing_rule_in_tx(&tx, manifest, grant_id, now)?;
        tx.commit()?;
        Ok(())
    }

    /// Refuse an activation whose reviewed-scope binding is incomplete for its
    /// action, leaving durable owner-actionable evidence. Runs *before* the
    /// activation transaction so the audit row survives the refusal — an
    /// in-transaction audit would roll back with it.
    pub(crate) fn reject_incomplete_scope_binding(
        &self,
        manifest: &StandingRuleManifest,
    ) -> Result<(), StoreError> {
        let Some(reason) = scope_binding_rejection(manifest) else {
            return Ok(());
        };
        self.append_audit(
            "standing_rule.scope_binding_rejected",
            Some(&manifest.action_id),
            None,
            Some(&reason),
            None,
            &[],
            &[],
        )?;
        Err(StoreError::ProposedArtifactLifecycle(reason))
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
        // Defense in depth: no activation path may persist an active rule
        // whose reviewed-scope binding omits a dimension its action's
        // descriptor requires. The durable refusal audit is written by
        // `reject_incomplete_scope_binding` before this transaction opens.
        if let Some(reason) = scope_binding_rejection(manifest) {
            return Err(StoreError::ProposedArtifactLifecycle(reason));
        }
        let dark_timeout = manifest.dark_window.map(|d| d.timeout_secs);
        let dark_default = manifest.dark_window.map(|d| match d.default {
            DarkWindowDefault::Allow => "allow",
            DarkWindowDefault::Deny => "deny",
        });
        // The two fast SQL pre-filter digests a reviewed-scope rule binds,
        // mirrored from the manifest's `reviewed_scope` binding so the runtime
        // row and the manifest cannot drift apart. Legacy unbounded rules
        // carry NULL (no scope binding), staying consultable.
        let (reviewed_scope_digest, compatibility_digest) = match &manifest.reviewed_scope {
            Some(binding) => (
                Some(binding.reviewed_scope_digest.as_str().to_string()),
                Some(binding.compatibility_digest.as_str().to_string()),
            ),
            None => (None, None),
        };
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
        // Coexistence (standing-rules spec): two disjoint scoped rules for one
        // action both stay active. A new rule revokes only rules it overlaps —
        // the same reviewed scope, or an unbounded rule (which covers every
        // scope). Two scoped rules with different reviewed_scope_digest values
        // are disjoint and both remain active.
        tx.execute(
            "UPDATE standing_rules SET status = 'revoked', revoked_at = ?3 \
             WHERE action_id = ?1 AND rule_id != ?2 AND status = 'active' \
               AND (reviewed_scope_digest IS NULL OR ?4 IS NULL OR reviewed_scope_digest = ?4)",
            params![
                manifest.action_id.to_string(),
                manifest.id,
                now_nanos,
                reviewed_scope_digest,
            ],
        )?;
        tx.execute(
            "INSERT INTO standing_rules (
                rule_id, artifact_id, version, action_id, rule_json,
                quota_max, quota_window_secs, rate_max, rate_window_secs,
                expires_after_secs, dark_window_timeout_secs, dark_window_default,
                status, activated_at, last_used_at, revoked_at, needs_review_since,
                reviewed_scope_digest, compatibility_digest
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'active', ?13, NULL, NULL, NULL, ?14, ?15)
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
                needs_review_since = excluded.needs_review_since,
                reviewed_scope_digest = excluded.reviewed_scope_digest,
                compatibility_digest = excluded.compatibility_digest
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
                reviewed_scope_digest,
                compatibility_digest,
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
        let row: Option<RuleRow> = conn
            .query_row(
                &format!(
                    "SELECT {RULE_ROW_COLUMNS} FROM standing_rules \
                     WHERE action_id = ?1 AND status = 'active' \
                     ORDER BY version DESC LIMIT 1"
                ),
                params![action_id.to_string()],
                rule_row_from_row,
            )
            .optional()?;
        let Some(row) = row else {
            return Ok(None);
        };
        let rule = rule_from_row(row, "active")?;
        let reference = rule.last_used_at.unwrap_or(rule.activated_at);
        let deadline_nanos =
            timestamp_to_epoch_nanos(reference)? + rule.expires_after_secs * 1_000_000_000;
        let now_nanos = timestamp_to_epoch_nanos(now)?;
        // Canonical exact-deadline boundary: a rule lapses the instant `now`
        // reaches `deadline` (i.e. `deadline <= now`), matching the strict
        // fired-token SQL (`elapsed < expiry`) and the atomic consult path.
        if deadline_nanos <= now_nanos {
            conn.execute(
                "UPDATE standing_rules SET status = 'needs_review', needs_review_since = ?2 \
                 WHERE rule_id = ?1 AND status = 'active'",
                params![rule.rule_id, now_nanos],
            )?;
            return Ok(None);
        }
        Ok(Some(rule))
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
    /// Highest activated version for a standing rule bound to `action_id`, or
    /// `None` when no such rule has ever been activated. Used to bump the
    /// version on re-answer so reactivation is not a no-op (equal-version
    /// activation returns early).
    pub fn standing_rule_version_for_action(
        &self,
        action_id: &ActionId,
    ) -> Result<Option<u32>, StoreError> {
        let conn = self.conn.lock();
        let version: Option<i64> = conn
            .query_row(
                "SELECT version FROM standing_rules WHERE action_id = ?1 \
                 ORDER BY version DESC LIMIT 1",
                params![action_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(version.map(|v| v as u32))
    }
}

pub(super) use crate::store::standing_rules_row::{
    epoch_nanos_to_timestamp, rule_from_row, rule_row_from_row, timestamp_to_epoch_nanos, RuleRow,
    RULE_ROW_COLUMNS,
};

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

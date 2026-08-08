//! Consuming a fired dark-window one-use token (P1-11), with the #135
//! reviewed-context revalidation. Split from `standing_rules_pending.rs` to
//! keep both files under the 500-line gate.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::owner_surface::OwnerSurfaceRef;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use ulid::Ulid;

use super::standing_rules::timestamp_to_epoch_nanos;
use super::{Store, StoreError};

impl Store {
    /// Consume a fired dark-window one-use token (P1-11): invoked from the
    /// shared mediation boundary when a re-dispatched action is still
    /// over-budget. Digest-bound to the exact request (action + grant + owner
    /// surface +
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
        owner_surface: &OwnerSurfaceRef,
        payload_ref: &Option<ArtifactRef>,
        now: Timestamp,
    ) -> Result<Option<(String, u32, String)>, StoreError> {
        self.consume_fired_pending_for_context(
            pending_id,
            action,
            grant_id,
            owner_surface,
            payload_ref,
            None,
            None,
            now,
        )
    }

    /// As [`Self::consume_standing_rule_fired_pending`], but additionally
    /// requires the reviewed scope and compatibility epoch the exception was
    /// minted against to still equal the freshly resolved context's (#135,
    /// D-160). #128 made drift stop a rule from *matching* at consultation; a
    /// token minted before the drift was still consumable after it, because
    /// consuming checked the rule row and not the context. It no longer is.
    #[allow(clippy::too_many_arguments)]
    pub fn consume_fired_pending_for_context(
        &self,
        pending_id: &str,
        action: &ActionId,
        grant_id: Ulid,
        owner_surface: &OwnerSurfaceRef,
        payload_ref: &Option<ArtifactRef>,
        reviewed_scope_digest: Option<&str>,
        compatibility_digest: Option<&str>,
        now: Timestamp,
    ) -> Result<Option<(String, u32, String)>, StoreError> {
        let fingerprint = super::standing_rules::standing_rule_fingerprint(
            action,
            grant_id,
            owner_surface,
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
                   AND reviewed_scope_digest IS ?4 AND compatibility_digest IS ?5 \
                   AND EXISTS (SELECT 1 FROM standing_rules r \
                               WHERE r.rule_id = standing_rule_pending_actions.rule_id \
                                 AND r.version = standing_rule_pending_actions.rule_version \
                                 AND r.status = 'active' \
                                 AND (r.expires_after_secs = 0 OR \
                                      (?3 - COALESCE(r.last_used_at, r.activated_at)) \
                                        < r.expires_after_secs * 1000000000))",
                params![
                    pending_id,
                    fingerprint,
                    now_nanos,
                    reviewed_scope_digest,
                    compatibility_digest
                ],
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

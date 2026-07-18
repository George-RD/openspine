use super::failure_surfacing_types::{
    detail_insert_columns, DeadLetterState, DetailReceipt, NotifyDeadLetter,
};
use super::{Store, StoreError};
use jiff::Timestamp;
use openspine_schemas::action::{ActionId, GateDecision};
use rusqlite::{params, OptionalExtension};
use ulid::Ulid;

impl Store {
    // ---- owner-notification dead-letter queue (AD-138) -------------------

    /// Compatibility wrapper for a failed notification with no digest batch.
    /// Test-only: production callers carry digest-batch metadata via
    /// [`Self::record_notify_failure_with_digest`].
    #[cfg(test)]
    pub fn record_notify_failure(
        &self,
        chat_id: i64,
        text: &str,
        task_grant_id: Ulid,
        reason: &str,
    ) -> Result<Ulid, StoreError> {
        self.record_notify_failure_with_digest(chat_id, text, task_grant_id, reason, &[], None)
    }

    /// Atomically record a failed send. `text_ref` is the digest of the
    /// owner-facing message held as an encrypted artifact (D-012: the store
    /// layer must not become a plaintext privacy surface), not raw text.
    /// `detail` carries the `/digest <ULID>` semantic metadata so a later
    /// retry can reconstruct the contract-specific receipt; `None` leaves the
    /// columns NULL for a generic owner notification.
    pub fn record_notify_failure_with_digest(
        &self,
        chat_id: i64,
        text_ref: &str,
        task_grant_id: Ulid,
        reason: &str,
        digest_item_ids: &[Ulid],
        detail: Option<&DetailReceipt>,
    ) -> Result<Ulid, StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        Self::append_audit_conn(
            &tx,
            "owner.notify_failed",
            Some(&ActionId::new("owner.notify")),
            None,
            Some(reason),
            Some(task_grant_id),
            &[],
            &[],
        )?;
        let id = Ulid::new();
        let now = Timestamp::now();
        let ids = digest_item_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let (semantic_kind, detail_ref, page_index, page_count, availability_outcome) =
            detail_insert_columns(detail);
        tx.execute(
            "INSERT INTO notify_dead_letters \
             (id, enqueued_at, chat_id, text_ref, task_grant_id, digest_item_ids, attempts, next_attempt_at, state, \
              semantic_kind, detail_ref, page_index, page_count, availability_outcome) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, 'pending', ?8, ?9, ?10, ?11, ?12)",
            params![
                id.to_string(),
                now.to_string(),
                chat_id,
                text_ref,
                task_grant_id.to_string(),
                ids,
                now.to_string(),
                semantic_kind,
                detail_ref,
                page_index,
                page_count,
                availability_outcome,
            ],
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// Record notification success and audit metadata. The connector outcome
    /// counter is recorded by the universal connector wrapper.
    pub fn record_notify_success(
        &self,
        task_grant_id: Ulid,
        detail: Option<&DetailReceipt>,
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock();
        let decision = GateDecision::Allow;
        let tx = conn.transaction()?;
        Self::append_audit_conn(
            &tx,
            "owner.notified",
            Some(&ActionId::new("owner.notify")),
            Some(&decision),
            None,
            Some(task_grant_id),
            &[],
            &[],
        )?;
        if let Some(detail) = detail {
            detail.append_in_tx(&tx)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Record success, resolve digest items, and optional detail receipt atomically.
    pub fn record_notify_success_and_resolve(
        &self,
        task_grant_id: Ulid,
        digest_item_ids: &[Ulid],
        detail: Option<&DetailReceipt>,
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let decision = GateDecision::Allow;
        Self::append_audit_conn(
            &tx,
            "owner.notified",
            Some(&ActionId::new("owner.notify")),
            Some(&decision),
            None,
            Some(task_grant_id),
            &[],
            &[],
        )?;
        for id in digest_item_ids {
            tx.execute(
                "UPDATE digest_items SET resolved = 1 WHERE id = ?1",
                params![id.to_string()],
            )?;
        }
        if let Some(detail) = detail {
            detail.append_in_tx(&tx)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Claim one due dead-letter for retry, marking it in_progress and
    /// incrementing attempts. Returns None when nothing is due.
    pub fn claim_due_dead_letter(
        &self,
        now: Timestamp,
    ) -> Result<Option<NotifyDeadLetter>, StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        #[allow(clippy::type_complexity)]
        let row: Option<(
            String,
            String,
            i64,
            String,
            String,
            String,
            i64,
            String,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<i64>,
            Option<String>,
        )> = tx
            .query_row(
                "SELECT id, enqueued_at, chat_id, text_ref, task_grant_id, digest_item_ids, attempts, next_attempt_at, \
                        semantic_kind, detail_ref, page_index, page_count, availability_outcome \
                 FROM notify_dead_letters \
                 WHERE ((state = 'pending' AND next_attempt_at <= ?1) \
                    OR (state = 'in_progress' AND claimed_until <= ?1)) \
                 ORDER BY next_attempt_at LIMIT 1",
                params![now.to_string()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                        row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                        row.get(11)?,
                        row.get(12)?,
                    ))
                },
            )
            .optional()?;
        let Some((
            id,
            enqueued_at,
            chat_id,
            text_ref,
            task_grant,
            digest_ids,
            attempts,
            next_attempt_at,
            semantic_kind,
            detail_ref,
            page_index,
            page_count,
            availability_outcome,
        )) = row
        else {
            return Ok(None);
        };
        let new_attempts = attempts + 1;
        let lease_until = now + std::time::Duration::from_secs(300);
        let claim_token = Ulid::new().to_string();
        let changed = tx.execute(
            "UPDATE notify_dead_letters SET state = 'in_progress', attempts = ?2, claimed_until = ?3, claim_token = ?4 \
             WHERE id = ?1 AND ((state = 'pending' AND next_attempt_at <= ?5) OR (state = 'in_progress' AND claimed_until <= ?5))",
            params![id, new_attempts, lease_until.to_string(), claim_token, now.to_string()],
        )?;
        if changed != 1 {
            return Ok(None);
        }
        let decision = GateDecision::Allow;
        Self::append_audit_conn(
            &tx,
            "owner.notify_attempted",
            Some(&ActionId::new("owner.notify")),
            Some(&decision),
            Some("retry attempt"),
            Some(Ulid::from_string(&task_grant).map_err(|_| {
                StoreError::BadDigest(format!("dead_letter.task_grant_id {task_grant}"))
            })?),
            &[],
            &[],
        )?;
        tx.commit()?;
        let task_grant_id = Ulid::from_string(&task_grant).map_err(|_| {
            StoreError::BadDigest(format!("dead_letter.task_grant_id {task_grant}"))
        })?;
        let digest_item_ids = digest_ids
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| {
                Ulid::from_string(s).map_err(|_| StoreError::BadDigest(format!("digest id {s}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(NotifyDeadLetter {
            id: Ulid::from_string(&id)
                .map_err(|_| StoreError::BadDigest(format!("notify_dead_letters.id {id}")))?,
            enqueued_at: enqueued_at.parse().map_err(|_| {
                StoreError::BadDigest(format!("dead_letter.enqueued_at {enqueued_at}"))
            })?,
            chat_id,
            text_ref,
            task_grant_id,
            digest_item_ids,
            attempts: new_attempts as u32,
            next_attempt_at: next_attempt_at.parse().map_err(|_| {
                StoreError::BadDigest(format!("dead_letter.next_attempt_at {next_attempt_at}"))
            })?,
            state: DeadLetterState::InProgress,
            claim_token: Some(claim_token),
            semantic_kind,
            detail_ref,
            page_index,
            page_count,
            availability_outcome,
        }))
    }

    /// Resolve a claimed dead-letter and record its success receipt atomically.
    /// When `detail` is `Some`, the detail-specific receipt is appended in the
    /// same transaction, fenced by `claim_token`. Stale or duplicate completion
    /// returns `false` and writes no receipt.
    pub fn complete_dead_letter_success(
        &self,
        id: Ulid,
        claim_token: &str,
        task_grant_id: Ulid,
        digest_item_ids: &[Ulid],
        detail: Option<&DetailReceipt>,
    ) -> Result<bool, StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let changed = tx.execute(
            "UPDATE notify_dead_letters SET state = 'resolved', claimed_until = NULL, claim_token = NULL \
             WHERE id = ?1 AND state = 'in_progress' AND claim_token = ?2",
            params![id.to_string(), claim_token],
        )?;
        if changed != 1 {
            return Ok(false);
        }
        let decision = GateDecision::Allow;
        Self::append_audit_conn(
            &tx,
            "owner.notified",
            Some(&ActionId::new("owner.notify")),
            Some(&decision),
            None,
            Some(task_grant_id),
            &[],
            &[],
        )?;
        tx.execute(
            "INSERT INTO connector_counters (connector, outcome, count) VALUES ('telegram', 'success', 1) \
             ON CONFLICT(connector, outcome) DO UPDATE SET count = count + 1",
            [],
        )?;
        for item_id in digest_item_ids {
            tx.execute(
                "UPDATE digest_items SET resolved = 1 WHERE id = ?1",
                params![item_id.to_string()],
            )?;
        }
        if let Some(detail) = detail {
            detail.append_in_tx(&tx)?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// Atomically reschedule a failed retry AND record its durable failure
    /// receipt (`owner.notify_retry_failed` + telegram failure counter),
    /// conditioned on `claim_token`. Does not re-increment `attempts` —
    /// [`Self::claim_due_dead_letter`] already counted this attempt at claim
    /// time.
    pub fn reschedule_dead_letter_failure(
        &self,
        id: Ulid,
        claim_token: &str,
        next_attempt_at: Timestamp,
        reason: &str,
        task_grant_id: Ulid,
    ) -> Result<bool, StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let changed = tx.execute(
            "UPDATE notify_dead_letters SET state = 'pending', next_attempt_at = ?2, claimed_until = NULL, claim_token = NULL \
             WHERE id = ?1 AND state = 'in_progress' AND claim_token = ?3",
            params![id.to_string(), next_attempt_at.to_string(), claim_token],
        )?;
        if changed != 1 {
            return Ok(false);
        }
        Self::append_audit_conn(
            &tx,
            "owner.notify_failed",
            Some(&ActionId::new("owner.notify")),
            None,
            Some(reason),
            Some(task_grant_id),
            &[],
            &[],
        )?;
        if !reason.starts_with("resource failure") {
            tx.execute(
                "INSERT INTO connector_counters (connector, outcome, count) VALUES ('telegram', 'failure', 1) \
                 ON CONFLICT(connector, outcome) DO UPDATE SET count = count + 1",
                [],
            )?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// Every non-resolved dead-letter, for tests and future ops tooling.
    #[cfg(test)]
    pub fn pending_dead_letters(&self) -> Result<Vec<NotifyDeadLetter>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, enqueued_at, chat_id, text_ref, task_grant_id, attempts, next_attempt_at, state, \
                    semantic_kind, detail_ref, page_index, page_count, availability_outcome \
             FROM notify_dead_letters WHERE state != 'resolved' ORDER BY enqueued_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<i64>>(10)?,
                row.get::<_, Option<i64>>(11)?,
                row.get::<_, Option<String>>(12)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(
                |(
                    id,
                    enqueued_at,
                    chat_id,
                    text_ref,
                    task_grant_id,
                    attempts,
                    next_attempt_at,
                    state,
                    semantic_kind,
                    detail_ref,
                    page_index,
                    page_count,
                    availability_outcome,
                )| {
                    Ok(NotifyDeadLetter {
                        id: Ulid::from_string(&id).map_err(|_| {
                            StoreError::BadDigest(format!("notify_dead_letters.id {id}"))
                        })?,
                        enqueued_at: enqueued_at.parse().map_err(|_| {
                            StoreError::BadDigest(format!("dead_letter.enqueued_at {enqueued_at}"))
                        })?,
                        chat_id,
                        text_ref,
                        task_grant_id: Ulid::from_string(&task_grant_id).map_err(|_| {
                            StoreError::BadDigest(format!(
                                "dead_letter.task_grant_id {task_grant_id}"
                            ))
                        })?,
                        digest_item_ids: Vec::new(),
                        claim_token: None,
                        attempts: attempts as u32,
                        next_attempt_at: next_attempt_at.parse().map_err(|_| {
                            StoreError::BadDigest(format!(
                                "dead_letter.next_attempt_at {next_attempt_at}"
                            ))
                        })?,
                        state: DeadLetterState::parse(&state)?,
                        semantic_kind,
                        detail_ref,
                        page_index,
                        page_count,
                        availability_outcome,
                    })
                },
            )
            .collect()
    }

    // ---- per-connector success/failure counters (AD-138) -----------------

    /// Increment `connector`'s `outcome` counter ("success" or "failure").
    /// The same counters AD-103's breaker and AD-013's calibration signal
    /// will read (AD-138) — this change only owns the write side.
    pub fn increment_connector_outcome(
        &self,
        connector: &str,
        outcome: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO connector_counters (connector, outcome, count) VALUES (?1, ?2, 1) \
             ON CONFLICT(connector, outcome) DO UPDATE SET count = count + 1",
            params![connector, outcome],
        )?;
        Ok(())
    }

    /// Current count for one `(connector, outcome)` pair; `0` if never
    /// recorded.
    #[cfg(test)]
    pub fn connector_counter(&self, connector: &str, outcome: &str) -> Result<i64, StoreError> {
        let conn = self.conn.lock();
        let count: Option<i64> = conn
            .query_row(
                "SELECT count FROM connector_counters WHERE connector = ?1 AND outcome = ?2",
                params![connector, outcome],
                |row| row.get(0),
            )
            .optional()?;
        Ok(count.unwrap_or(0))
    }
}

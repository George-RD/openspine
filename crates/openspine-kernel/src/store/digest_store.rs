use super::failure_surfacing_types::{DigestItem, MAX_DIGEST_SUMMARY_CHARS};
use super::{Store, StoreError};
use jiff::Timestamp;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use rusqlite::{params, OptionalExtension};
use std::slice;
use ulid::Ulid;

impl Store {
    /// Record one unresolved failure. New rows require a verified encrypted
    /// `text_ref`; NULL is reserved for migrated legacy rows.
    pub fn batch_digest_failure(
        &self,
        class: &str,
        summary: &str,
        text_ref: &str,
    ) -> Result<Ulid, StoreError> {
        let artifact_ref = ArtifactRef {
            digest: Digest::parse(text_ref)
                .map_err(|_| StoreError::BadDigest(text_ref.to_string()))?,
            schema_version: 1,
        };
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let id = Ulid::new();
        let now = Timestamp::now();
        let summary = if summary.chars().count() > MAX_DIGEST_SUMMARY_CHARS {
            summary
                .chars()
                .take(MAX_DIGEST_SUMMARY_CHARS)
                .collect::<String>()
        } else {
            summary.to_string()
        };
        tx.execute(
            "INSERT INTO digest_items (id, ts, class, summary, text_ref, resolved) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
            params![id.to_string(), now.to_string(), class, summary, text_ref],
        )?;
        Self::append_audit_conn(
            &tx,
            "failure.digest_batched",
            None,
            None,
            None,
            None,
            &[],
            slice::from_ref(&artifact_ref),
        )?;
        tx.commit()?;
        Ok(id)
    }
    /// Record a completed (or escalated) headless webhook run in the owner
    /// digest (AD-134). The `text_ref` is an encrypted artifact digest
    /// carrying the non-sensitive run detail; the bounded `summary` is the
    /// only plaintext the store retains. Persisted alongside the
    /// `headless.hook_completed` audit so the run surfaces only via the
    /// digest, never an owner conversation.
    pub fn record_headless_hook_completion(
        &self,
        class: &str,
        summary: &str,
        text_ref: &str,
        task_grant_id: Option<Ulid>,
    ) -> Result<Ulid, StoreError> {
        let artifact_ref = ArtifactRef {
            digest: Digest::parse(text_ref)
                .map_err(|_| StoreError::BadDigest(text_ref.to_string()))?,
            schema_version: 1,
        };
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let id = Ulid::new();
        let now = Timestamp::now();
        let summary = if summary.chars().count() > MAX_DIGEST_SUMMARY_CHARS {
            summary
                .chars()
                .take(MAX_DIGEST_SUMMARY_CHARS)
                .collect::<String>()
        } else {
            summary.to_string()
        };
        tx.execute(
            "INSERT INTO digest_items (id, ts, class, summary, text_ref, resolved) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
            params![id.to_string(), now.to_string(), class, summary, text_ref],
        )?;
        Self::append_audit_conn(
            &tx,
            "headless.hook_completed",
            None,
            None,
            None,
            task_grant_id,
            &[],
            slice::from_ref(&artifact_ref),
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// Record a digest item marking a failure's detail as unavailable, without
    /// storing any encrypted artifact. The marker is non-secret, so it must not
    /// depend on the (possibly inoperable) artifact store -- a crypto-erased
    /// counterparty or a key the kernel cannot unwrap must still let the owner
    /// be told "detail unavailable" without leaking the cause. `text_ref` is
    /// NULL, matching the legacy/unavailable convention (never a dangling ref
    /// to a blob that cannot be decrypted).
    pub fn record_unavailable_failure(&self, class: &str) -> Result<Ulid, StoreError> {
        let mut conn = self.conn.lock();
        // One terminal marker per class is enough: repeated views of legacy or
        // unresolvable rows must not keep inserting identical NULL-ref markers.
        // Lookup and insert share one write transaction so concurrent views
        // cannot each observe absence and both insert.
        let summary = format!("[{class}] detail unavailable");
        let tx = conn.transaction()?;
        if let Some(existing) = tx
            .query_row(
                "SELECT id FROM digest_items \
                 WHERE resolved = 0 AND text_ref IS NULL AND class = ?1 AND summary = ?2 \
                 ORDER BY seq LIMIT 1",
                params![class, summary],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            tx.commit()?;
            return Ulid::from_string(&existing)
                .map_err(|_| StoreError::BadDigest(format!("digest_items.id {existing}")));
        }
        let id = Ulid::new();
        let now = Timestamp::now();
        tx.execute(
            "INSERT INTO digest_items (id, ts, class, summary, text_ref, resolved) VALUES (?1, ?2, ?3, ?4, NULL, 0)",
            params![id.to_string(), now.to_string(), class, summary],
        )?;
        Self::append_audit_conn(
            &tx,
            "failure.digest_unavailable",
            None,
            None,
            None,
            None,
            &[],
            &[],
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// Whether an item is the canonical terminal marker written by
    /// [`Self::record_unavailable_failure`]. Legacy NULL-ref rows have a
    /// different summary and are intentionally not terminal until one marker
    /// has been surfaced for them.
    pub(crate) fn is_canonical_unavailable_failure(&self, item: &DigestItem) -> bool {
        item.text_ref.is_none()
            && item
                .summary
                .strip_prefix('[')
                .and_then(|summary| summary.strip_prefix(&item.class))
                == Some("] detail unavailable")
    }

    /// Test-only legacy fixture: no ref and no claim that the old summary is
    /// encrypted. Production ingestion always uses `batch_digest_failure`.
    #[cfg(test)]
    pub fn insert_legacy_digest_failure(
        &self,
        class: &str,
        summary: &str,
    ) -> Result<Ulid, StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let id = Ulid::new();
        let now = Timestamp::now();
        let summary = if summary.chars().count() > MAX_DIGEST_SUMMARY_CHARS {
            summary
                .chars()
                .take(MAX_DIGEST_SUMMARY_CHARS)
                .collect::<String>()
        } else {
            summary.to_string()
        };
        tx.execute(
            "INSERT INTO digest_items (id, ts, class, summary, text_ref, resolved) VALUES (?1, ?2, ?3, ?4, NULL, 0)",
            params![id.to_string(), now.to_string(), class, summary],
        )?;
        Self::append_audit_conn(
            &tx,
            "failure.digest_batched",
            None,
            None,
            None,
            None,
            &[],
            &[],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn owner_digest_items(&self) -> Result<Vec<DigestItem>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, ts, class, summary, text_ref, resolved FROM digest_items \
             WHERE resolved = 0 ORDER BY seq",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(id, ts, class, summary, text_ref, resolved)| {
                Ok(DigestItem {
                    id: Ulid::from_string(&id)
                        .map_err(|_| StoreError::BadDigest(format!("digest_items.id {id}")))?,
                    ts: ts
                        .parse()
                        .map_err(|_| StoreError::BadDigest(format!("digest_items.ts {ts}")))?,
                    class,
                    summary,
                    text_ref,
                    resolved: resolved != 0,
                })
            })
            .collect()
    }

    pub fn owner_digest_item(&self, id: Ulid) -> Result<Option<DigestItem>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, ts, class, summary, text_ref, resolved FROM digest_items WHERE id = ?1",
        )?;
        let mut rows = stmt.query([id.to_string()])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let raw_id: String = row.get(0)?;
        Ok(Some(DigestItem {
            id: Ulid::from_string(&raw_id)
                .map_err(|_| StoreError::BadDigest(format!("digest_items.id {raw_id}")))?,
            ts: row
                .get::<_, String>(1)?
                .parse()
                .map_err(|_| StoreError::BadDigest(format!("digest_items.ts for {raw_id}")))?,
            class: row.get(2)?,
            summary: row.get(3)?,
            text_ref: row.get(4)?,
            resolved: row.get::<_, i64>(5)? != 0,
        }))
    }
}

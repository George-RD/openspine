use super::failure_surfacing_types::{DigestItem, MAX_DIGEST_SUMMARY_CHARS};
use super::{Store, StoreError};
use jiff::Timestamp;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use rusqlite::params;
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

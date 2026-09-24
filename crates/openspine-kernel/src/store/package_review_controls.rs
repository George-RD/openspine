//! Durable identities for offline review, including lost overlay publications.
use super::{Store, StoreError};
use openspine_schemas::digest::Digest;

impl Store {
    /// Enumerate committed Active identities independently of learned rows and
    /// files. Otherwise losing both could make committed authority invisible.
    /// The caller holds the ordinary package-maintenance lifetime lock.
    pub(crate) fn list_active_artifact_versions(
        &self,
    ) -> Result<Vec<(String, String, u32)>, StoreError> {
        self.with_deferred_read(|tx| {
            let mut statement = tx.prepare(
                "SELECT kind, artifact_id, version, yaml_digest FROM proposed_artifacts \
                 WHERE state = 'active' ORDER BY kind, artifact_id, version",
            )?;
            let mut rows = statement.query([])?;
            let mut versions = Vec::new();
            while let Some(row) = rows.next()? {
                let kind: String = row.get(0)?;
                let id: String = row.get(1)?;
                let version =
                    u32::try_from(row.get::<_, i64>(2)?).map_err(|_| StoreError::NumericRange)?;
                let digest: String = row.get(3)?;
                if version == 0
                    || kind.is_empty()
                    || id.is_empty()
                    || kind.len() > 128
                    || id.len() > 1024
                    || versions.len() >= 4096
                {
                    return Err(StoreError::BadLedgerMeta(
                        "package review active artifact controls exceed supported bounds".into(),
                    ));
                }
                Digest::parse(digest).map_err(|_| {
                    StoreError::BadDigest("package review active artifact digest".into())
                })?;
                versions.push((kind, id, version));
            }
            Ok(versions)
        })
    }
}

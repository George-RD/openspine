use super::*;

impl ArtifactStore {
    /// Open captured-state evidence without startup cleanup or key recovery.
    /// A never-started initialized instance may have no artifact/key directory;
    /// that absence is retained, and any referenced missing blob fails closed.
    pub(crate) fn open_without_recovery(
        root: PathBuf,
        master_key: [u8; 32],
    ) -> Result<Self, ArtifactStoreError> {
        match std::fs::symlink_metadata(&root) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            result => {
                return Err(ArtifactStoreError::Io {
                    path: root,
                    source: result.err().unwrap_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "observed artifact storage must be an ordinary directory",
                        )
                    }),
                });
            }
        }
        let keys_dir = if root.file_name().and_then(|n| n.to_str()) == Some("artifacts") {
            root.parent()
                .map(|parent| parent.join("keys"))
                .unwrap_or_else(|| root.join("keys"))
        } else {
            root.join("keys")
        };
        let keys = CounterpartyKeyRing::open_without_recovery(keys_dir, master_key)?;
        Ok(Self {
            root,
            keys,
            #[cfg(test)]
            fault_put: std::sync::atomic::AtomicBool::new(false),
            #[cfg(test)]
            fault_existing_blob_sync: std::sync::atomic::AtomicBool::new(false),
            #[cfg(test)]
            fault_clear_upgrade_pending_sync: std::sync::atomic::AtomicBool::new(false),
        })
    }
}

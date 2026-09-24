use super::*;

impl CounterpartyKeyRing {
    /// Offline observation under the caller's data-root lifetime lock. Missing
    /// storage remains missing; reads of referenced keys then fail normally.
    /// Do not fsync, remove tombstoned keys, sweep temporary files or migrate.
    pub(crate) fn open_without_recovery(
        dir: PathBuf,
        master_key: [u8; KEY_LEN],
    ) -> Result<Self, CounterpartyKeyError> {
        match std::fs::symlink_metadata(&dir) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            result => {
                return Err(CounterpartyKeyError::Io {
                    path: dir,
                    source: result.err().unwrap_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "observed key storage must be an ordinary directory",
                        )
                    }),
                });
            }
        }
        Ok(Self {
            dir,
            master_cipher: Aes256Gcm::new_from_slice(&master_key).expect("key is exactly 32 bytes"),
            scope_locks: Mutex::new(HashMap::new()),
            closed_scopes: Mutex::new(HashSet::new()),
            #[cfg(test)]
            fsync_count: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            fail_fsync_at: std::sync::atomic::AtomicUsize::new(usize::MAX),
        })
    }
}

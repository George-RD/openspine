use super::*;

impl CounterpartyKeyRing {
    /// Startup recovery: for every tombstone whose physical key file still
    /// exists (a crash between the tombstone's fsync and the key unlink in
    /// `erase_locked`), complete the physical deletion now. Also clean up any
    /// orphaned temporary publication/migration files left by crashes.
    pub(super) fn recover_pending_erasures(&self) -> Result<(), CounterpartyKeyError> {
        let entries = std::fs::read_dir(&self.dir).map_err(|source| CounterpartyKeyError::Io {
            path: self.dir.clone(),
            source,
        })?;
        let mut any_recovered = false;
        for entry in entries {
            let entry = entry.map_err(|source| CounterpartyKeyError::Io {
                path: self.dir.clone(),
                source,
            })?;
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };

            // Remove orphaned temporary files
            if name.contains(".tmp.") {
                let tmp_path = self.dir.join(name);
                match std::fs::remove_file(&tmp_path) {
                    Ok(()) => any_recovered = true,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(source) => {
                        return Err(CounterpartyKeyError::Io {
                            path: tmp_path,
                            source,
                        });
                    }
                }
                continue;
            }

            // Remove unlinked keys for tombstoned counterparties
            let Some(id_str) = name.strip_suffix(".erased") else {
                continue;
            };
            let Ok(id) = Ulid::from_string(id_str) else {
                continue;
            };
            let key_path = self.key_path(id);
            match std::fs::remove_file(&key_path) {
                Ok(()) => any_recovered = true,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(CounterpartyKeyError::Io {
                        path: key_path,
                        source,
                    });
                }
            }

            // Also clean up any temp files for this erased id
            let pattern = format!("{id}.tmp.");
            if let Ok(dir_entries) = std::fs::read_dir(&self.dir) {
                for dir_entry in dir_entries.flatten() {
                    if let Some(entry_name) = dir_entry.file_name().to_str() {
                        if entry_name.starts_with(&pattern) {
                            if let Err(source) = std::fs::remove_file(dir_entry.path()) {
                                if source.kind() != std::io::ErrorKind::NotFound {
                                    return Err(CounterpartyKeyError::Io {
                                        path: dir_entry.path(),
                                        source,
                                    });
                                }
                            }
                            any_recovered = true;
                        }
                    }
                }
            }
        }
        if any_recovered {
            self.fsync_dir(&self.dir)?;
        }
        Ok(())
    }

    pub(super) fn key_pending_path(&self, counterparty_id: Ulid) -> PathBuf {
        // Durable key-publication marker. Keep the legacy ".migpending" name
        // so markers left by an interrupted legacy upgrade remain recoverable.
        // It must NOT contain ".tmp." so startup orphan-temp recovery leaves
        // it for read-time or create-time retry.
        self.dir.join(format!("{counterparty_id}.migpending"))
    }

    pub(super) fn mark_key_pending_locked(
        &self,
        counterparty_id: Ulid,
    ) -> Result<(), CounterpartyKeyError> {
        let path = self.key_pending_path(counterparty_id);
        if self.require_regular_file_or_absent(&path)? {
            // A prior attempt may have created the marker but failed its
            // directory fsync. Re-establish durability before publication.
            let f = std::fs::File::open(&path).map_err(|source| CounterpartyKeyError::Io {
                path: path.clone(),
                source,
            })?;
            f.sync_all().map_err(|source| CounterpartyKeyError::Io {
                path: path.clone(),
                source,
            })?;
            return self.fsync_dir(&self.dir);
        }
        let f = std::fs::File::create(&path).map_err(|source| CounterpartyKeyError::Io {
            path: path.clone(),
            source,
        })?;
        f.sync_all().map_err(|source| CounterpartyKeyError::Io {
            path: path.clone(),
            source,
        })?;
        self.fsync_dir(&self.dir)
    }

    pub(super) fn clear_key_pending_locked(
        &self,
        counterparty_id: Ulid,
    ) -> Result<(), CounterpartyKeyError> {
        let path = self.key_pending_path(counterparty_id);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // Already absent; still fsync the directory so a prior unlink
                // that never reached the dir journal is repaired.
            }
            Err(source) => {
                return Err(CounterpartyKeyError::Io { path, source });
            }
        }
        if let Err(err) = self.fsync_dir(&self.dir) {
            // Unlink already happened. Re-plant and durably publish the marker
            // so a subsequent current-key read still takes the recovery path;
            // otherwise a failed clear-dir fsync would silently drop the retry
            // signal (and a crash right after create would lose the entry).
            (|| -> Result<(), CounterpartyKeyError> {
                let f =
                    std::fs::File::create(&path).map_err(|source| CounterpartyKeyError::Io {
                        path: path.clone(),
                        source,
                    })?;
                f.sync_all().map_err(|source| CounterpartyKeyError::Io {
                    path: path.clone(),
                    source,
                })?;
                self.fsync_dir(&self.dir)
            })()?;
            return Err(err);
        }
        Ok(())
    }

    pub(super) fn recover_pending_key_locked(
        &self,
        key_path: &Path,
        counterparty_id: Ulid,
    ) -> Result<(), CounterpartyKeyError> {
        if !self.require_regular_file_or_absent(&self.key_pending_path(counterparty_id))? {
            return Ok(());
        }
        if self.require_regular_file_or_absent(&self.tombstone_path(counterparty_id))? {
            return self.clear_key_pending_locked(counterparty_id);
        }
        self.sync_key_file_and_dir(key_path)?;
        self.clear_key_pending_locked(counterparty_id)
    }
}

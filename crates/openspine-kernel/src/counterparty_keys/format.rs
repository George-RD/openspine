use super::*;

impl CounterpartyKeyRing {
    pub(super) fn wrap_v1_key(
        &self,
        counterparty_id: Ulid,
        key: &[u8; KEY_LEN],
    ) -> Result<Vec<u8>, CounterpartyKeyError> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::try_from(nonce_bytes.as_slice()).expect("nonce is 12 bytes");
        let aad = make_v1_aad(counterparty_id);
        let payload = Payload {
            msg: key.as_slice(),
            aad: &aad,
        };
        let ciphertext = self
            .master_cipher
            .encrypt(&nonce, payload)
            .map_err(|_| CounterpartyKeyError::Wrap)?;
        let mut wrapped = Vec::with_capacity(V1_HEADER.len() + NONCE_LEN + ciphertext.len());
        wrapped.extend_from_slice(V1_HEADER);
        wrapped.extend_from_slice(&nonce_bytes);
        wrapped.extend_from_slice(&ciphertext);
        Ok(wrapped)
    }

    pub(super) fn unwrap_file(
        &self,
        path: &Path,
        counterparty_id: Ulid,
    ) -> Result<([u8; KEY_LEN], bool), CounterpartyKeyError> {
        let wrapped = std::fs::read(path).map_err(|source| CounterpartyKeyError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        // A genuine v1 file is header + 12-byte nonce + AEAD ciphertext of a
        // 32-byte key (ciphertext is 32 + 16 tag = 48 bytes). Minimum shape:
        // 4 + 12 + 48 = 64 bytes. Anything shorter that merely starts with
        // OSK1 is treated as a possible legacy file whose random nonce
        // happened to begin with those four bytes.
        const V1_MIN_LEN: usize = 4 + NONCE_LEN + KEY_LEN + 16;
        if wrapped.starts_with(V1_HEADER) && wrapped.len() >= V1_MIN_LEN {
            let nonce_bytes = &wrapped[V1_HEADER.len()..V1_HEADER.len() + NONCE_LEN];
            let ciphertext = &wrapped[V1_HEADER.len() + NONCE_LEN..];
            let nonce = Nonce::try_from(nonce_bytes)
                .map_err(|_| CounterpartyKeyError::Truncated(path.to_path_buf()))?;
            let aad = make_v1_aad(counterparty_id);
            let payload = Payload {
                msg: ciphertext,
                aad: &aad,
            };
            match self.master_cipher.decrypt(&nonce, payload) {
                Ok(plaintext) if plaintext.len() == KEY_LEN => {
                    let mut key = [0u8; KEY_LEN];
                    key.copy_from_slice(&plaintext);
                    return Ok((key, false));
                }
                Ok(_) => return Err(CounterpartyKeyError::Decrypt),
                // Proven decryption failed. Fall through and try the legacy
                // layout so a legacy nonce that collides with the OSK1 prefix
                // is still recoverable; if that also fails we surface Decrypt.
                Err(_) => {}
            }
        }

        if wrapped.len() < NONCE_LEN {
            return Err(CounterpartyKeyError::Truncated(path.to_path_buf()));
        }
        let (nonce_bytes, ciphertext) = wrapped.split_at(NONCE_LEN);
        let nonce = Nonce::try_from(nonce_bytes)
            .map_err(|_| CounterpartyKeyError::Truncated(path.to_path_buf()))?;
        let payload = Payload {
            msg: ciphertext,
            aad: &[],
        };
        let plaintext = self
            .master_cipher
            .decrypt(&nonce, payload)
            .map_err(|_| CounterpartyKeyError::Decrypt)?;
        if plaintext.len() != KEY_LEN {
            return Err(CounterpartyKeyError::Decrypt);
        }
        let mut key = [0u8; KEY_LEN];
        key.copy_from_slice(&plaintext);
        Ok((key, true))
    }

    pub(super) fn migrate_legacy_key_to_v1_locked(
        &self,
        path: &Path,
        counterparty_id: Ulid,
        key: &[u8; KEY_LEN],
    ) -> Result<(), CounterpartyKeyError> {
        // Check tombstone under scope lock: do NOT resurrect if erased concurrently
        if self.require_regular_file_or_absent(&self.tombstone_path(counterparty_id))? {
            return Ok(());
        }

        // Marker first: if we crash after rename but before the key+dir sync,
        // a later v1 read will retry the durability work while the marker remains.
        self.mark_key_pending_locked(counterparty_id)?;

        let wrapped = self.wrap_v1_key(counterparty_id, key)?;
        let tmp_path = path.with_extension(format!("tmp.mig.{}", Ulid::new()));
        let mut guard = TempFileGuard::new(tmp_path.clone());

        let write_res = (|| -> Result<(), std::io::Error> {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_path)?;
            f.write_all(&wrapped)?;
            f.sync_all()
        })();

        if let Err(source) = write_res {
            if let Err(cleanup_err) = guard.remove() {
                return Err(CounterpartyKeyError::Io {
                    path: tmp_path,
                    source: cleanup_err,
                });
            }
            return Err(CounterpartyKeyError::Io {
                path: tmp_path,
                source,
            });
        }

        if self.require_regular_file_or_absent(&self.tombstone_path(counterparty_id))? {
            guard.remove().map_err(|source| CounterpartyKeyError::Io {
                path: tmp_path,
                source,
            })?;
            // No rename happened; drop the pending marker so we do not force a
            // key sync for a never-published upgrade.
            let _ = self.clear_key_pending_locked(counterparty_id);
            return Ok(());
        }

        if let Err(source) = std::fs::rename(&tmp_path, path) {
            if let Err(cleanup_err) = guard.remove() {
                return Err(CounterpartyKeyError::Io {
                    path: tmp_path,
                    source: cleanup_err,
                });
            }
            return Err(CounterpartyKeyError::Io {
                path: path.to_path_buf(),
                source,
            });
        }

        guard.0 = None;
        // Keep the marker until key+dir durability succeeds so a post-rename
        // fsync failure leaves a retry signal for the next current-key read.
        self.sync_key_file_and_dir(path)?;
        self.clear_key_pending_locked(counterparty_id)?;
        Ok(())
    }

    pub(crate) fn get_key_locked(
        &self,
        counterparty_id: Ulid,
    ) -> Result<Option<[u8; KEY_LEN]>, CounterpartyKeyError> {
        if self.scope_is_closed(counterparty_id)
            || self.require_regular_file_or_absent(&self.tombstone_path(counterparty_id))?
        {
            return Ok(None);
        }
        let path = self.key_path(counterparty_id);
        if !self.require_regular_file_or_absent(&path)? {
            return Ok(None);
        }
        let (key, is_legacy) = self.unwrap_file(&path, counterparty_id)?;
        if is_legacy {
            self.migrate_legacy_key_to_v1_locked(&path, counterparty_id, &key)?;
        } else {
            self.recover_pending_key_locked(&path, counterparty_id)?;
        }
        Ok(Some(key))
    }

    #[allow(dead_code)] // public key-ring API; scoped store paths use the already-locked variant
    pub fn get_key(
        &self,
        counterparty_id: Ulid,
    ) -> Result<Option<[u8; KEY_LEN]>, CounterpartyKeyError> {
        self.with_scope_lock(counterparty_id, || self.get_key_locked(counterparty_id))
    }
}

use super::*;

impl ArtifactStore {
    /// Store `plaintext` under the reserved `SYSTEM_SCOPE` (owner-authored
    /// and internal payloads). Identical to
    /// [`Self::put_scoped`] with `SYSTEM_SCOPE`. Signature retained for the
    /// many pre-existing callers.
    pub fn put(&self, plaintext: &[u8]) -> Result<ArtifactRef, ArtifactStoreError> {
        #[cfg(test)]
        if self.fault_put.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(ArtifactStoreError::Io {
                path: self.root.clone(),
                source: std::io::Error::other("injected artifact put failure (test)"),
            });
        }
        self.put_scoped(SYSTEM_SCOPE, plaintext)
    }

    /// Store `plaintext`, encrypting it under `counterparty_id`'s per-
    /// counterparty key. Returns the [`ArtifactRef`] the caller must use to
    /// retrieve it. Idempotent within a scope: storing the same plaintext
    /// twice for the same counterparty is a no-op the second time (the blob is
    /// content-addressed, so nothing new needs writing). Two DIFFERENT
    /// counterparties storing the same plaintext get the same `ArtifactRef`
    /// but two separate, independently-keyed on-disk blobs.
    ///
    /// **Crypto-erasure is permanent (AD-140).** If `counterparty_id` has
    /// been crypto-erased, this is REJECTED with
    /// `Err(KeyRing(Erased(_)))` — never a silent "self-heal" that would
    /// mint a fresh key and make previously-erased plaintext (or any new
    /// plaintext) storable again under that scope.
    ///
    /// The ENTIRE operation runs under `counterparty_id`'s per-counterparty
    /// in-process lock (see `CounterpartyKeyRing::with_scope_lock`), the
    /// SAME lock `Store::mark_learned_artifacts_erased` holds across its
    /// whole invalidate-through-key-delete sequence: this makes the write
    /// atomic w.r.t. a concurrently-racing erase for the SAME id — a write
    /// can never complete holding a key that gets deleted moments later,
    /// silently orphaning an `Ok(ref)` that was never actually readable
    /// ("dead on arrival").
    pub fn put_scoped(
        &self,
        counterparty_id: Ulid,
        plaintext: &[u8],
    ) -> Result<ArtifactRef, ArtifactStoreError> {
        self.keys.with_scope_lock(counterparty_id, || {
            let digest = digest_of_bytes(plaintext);
            let path = self.blob_path(counterparty_id, Self::digest_hex(&digest));

            let key_present = self.keys.get_key_locked(counterparty_id)?.is_some();
            if key_present && path.exists() {
                self.sync_existing_blob(&path)?;
                return Ok(ArtifactRef {
                    digest,
                    schema_version: 1,
                });
            }

            let key = self.keys.get_or_create_key_locked(counterparty_id)?;
            self.write_current_blob(&path, counterparty_id, plaintext, &key)?;

            Ok(ArtifactRef {
                digest,
                schema_version: 1,
            })
        })
    }

    /// Retrieve and decrypt a `SYSTEM_SCOPE` payload — convenience wrapper
    /// for the many pre-existing callers, every one of which only ever
    /// stores owner-authored or internal payloads. Per-counterparty
    /// retrieval goes through [`Self::get_scoped`].
    pub fn get(&self, artifact_ref: &ArtifactRef) -> Result<Vec<u8>, ArtifactStoreError> {
        self.get_scoped(SYSTEM_SCOPE, artifact_ref)
    }

    /// Retrieve and decrypt the plaintext for `artifact_ref` stored under
    /// `scope`, verifying the decrypted content's digest matches the ref
    /// before returning it. The blob's own header-recorded scope MUST match
    /// the requested `scope` — refused otherwise, a defense-in-depth check
    /// against ever decrypting a blob under the wrong counterparty's key even
    /// if a foreign/corrupted file ended up at the wrong path. A
    /// crypto-erased counterparty yields `Decrypt`/`KeyRing` because its key
    /// no longer exists.
    pub fn get_scoped(
        &self,
        scope: Ulid,
        artifact_ref: &ArtifactRef,
    ) -> Result<Vec<u8>, ArtifactStoreError> {
        self.keys.with_scope_lock(scope, || {
            let hex = Self::digest_hex(&artifact_ref.digest);
            let path = self.blob_path(scope, hex);
            let blob = std::fs::read(&path).map_err(|source| ArtifactStoreError::Io {
                path: path.clone(),
                source,
            })?;

            if blob.is_empty() {
                return Err(ArtifactStoreError::Truncated(path));
            }
            let tag = blob[0];
            if tag != RECOVERED_FORMAT && tag != CURRENT_FORMAT {
                return Err(ArtifactStoreError::UnknownFormat(path));
            }
            if blob.len() < HEADER_LEN + NONCE_LEN {
                return Err(ArtifactStoreError::Truncated(path));
            }
            let header = &blob[..HEADER_LEN];
            let mut header_scope = [0u8; SCOPE_LEN];
            header_scope.copy_from_slice(&header[1..HEADER_LEN]);
            if Ulid::from_bytes(header_scope) != scope {
                return Err(ArtifactStoreError::UnknownFormat(path));
            }

            let rest = &blob[HEADER_LEN..];
            let (nonce_bytes, ciphertext) = rest.split_at(NONCE_LEN);
            let nonce = Nonce::try_from(nonce_bytes)
                .map_err(|_| ArtifactStoreError::Truncated(path.clone()))?;
            let key = self
                .keys
                .get_key_locked(scope)?
                .ok_or(ArtifactStoreError::Decrypt)?;
            let cipher = Aes256Gcm::new_from_slice(&key).expect("key is exactly 32 bytes");

            let plaintext = if tag == CURRENT_FORMAT {
                cipher
                    .decrypt(
                        &nonce,
                        Payload {
                            msg: ciphertext,
                            aad: header,
                        },
                    )
                    .map_err(|_| ArtifactStoreError::Decrypt)?
            } else {
                cipher
                    .decrypt(&nonce, ciphertext)
                    .map_err(|_| ArtifactStoreError::Decrypt)?
            };

            if digest_of_bytes(&plaintext) != artifact_ref.digest {
                return Err(ArtifactStoreError::DigestMismatch);
            }
            if tag == RECOVERED_FORMAT {
                self.persist_upgrade_pending(&path)?;
                self.write_current_blob(&path, scope, &plaintext, &key)?;
                self.clear_upgrade_pending(&path)?;
            } else {
                self.recover_pending_upgrade(&path)?;
            }
            Ok(plaintext)
        })
    }

    #[allow(dead_code)] // consumed via tests and future erasure call sites
    /// Crypto-erase a counterparty's payload key (AD-140). Returns `true` if a
    /// key existed and was deleted, `false` if there was nothing to erase.
    /// After this call, every blob encrypted under that key is undecryptable.
    pub fn erase_counterparty_key(
        &self,
        counterparty_id: Ulid,
    ) -> Result<bool, ArtifactStoreError> {
        Ok(self.keys.erase(counterparty_id)?)
    }

    /// Run `f` while holding `counterparty_id`'s per-counterparty
    /// in-process lock — the SAME lock instance `put_scoped` uses
    /// internally. `pub(crate)` so `Store::mark_learned_artifacts_erased`
    /// can wrap its own full invalidate-through-key-delete sequence in
    /// this lock, making it atomic w.r.t. a concurrently-racing
    /// `put_scoped` for the same id (closes the "write completes under a
    /// key that gets deleted moments later" race — see
    /// `CounterpartyKeyRing`'s module doc for the single-process-only
    /// caveat).
    pub(crate) fn with_scope_lock<T, E>(
        &self,
        counterparty_id: Ulid,
        f: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        self.keys.with_scope_lock(counterparty_id, f)
    }

    /// Mark `counterparty_id` closed in this process while the caller still
    /// holds its scope lock. Store erasure calls this immediately after the
    /// database transaction commits, before fallible filesystem cleanup.
    pub(crate) fn close_counterparty_scope_in_memory(&self, counterparty_id: Ulid) {
        self.keys.close_scope_in_memory(counterparty_id);
    }

    /// Locked variant of [`Self::erase_counterparty_key`]: the CALLER MUST
    /// already hold `with_scope_lock(counterparty_id, ..)` (used by
    /// `Store::mark_learned_artifacts_erased` from within its own held
    /// lock — calling the public, re-locking `erase_counterparty_key`
    /// there would deadlock on the same non-reentrant mutex).
    pub(crate) fn erase_counterparty_key_locked(
        &self,
        counterparty_id: Ulid,
    ) -> Result<bool, ArtifactStoreError> {
        Ok(self.keys.erase_locked(counterparty_id)?)
    }
}

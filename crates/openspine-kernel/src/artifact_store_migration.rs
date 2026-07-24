use super::*;

impl ArtifactStore {
    /// Re-key every legacy (untagged) blob under `legacy_master_key` into the
    /// new per-counterparty format under `SYSTEM_SCOPE`. Idempotent: a blob
    /// already migrated (no flat file left at the root) is simply never seen
    /// again by the scan. Returns the number of blobs freshly migrated in
    /// THIS call (crash-recovery cleanups, see below, are not counted as
    /// fresh migrations).
    ///
    /// Blobs are content-addressed by the digest of their plaintext, so a
    /// flat root-level file's NAME is already the digest hex — legacy or not
    /// — which lets the post-migration target path be computed without
    /// decrypting anything first. Detection itself is NOT based on that name
    /// or on any 1-byte format-tag heuristic (a legacy nonce's first byte
    /// equalling `FORMAT_TAG` is a real, if rare, collision that a heuristic
    /// would silently lose data on): a blob is legacy iff it actually
    /// decrypts under `legacy_master_key` as `[nonce:12][ciphertext]`.
    ///
    /// Durability/crash-safety: the re-encrypted blob is written to a temp
    /// file, `fsync`ed, and renamed onto its SYSTEM_SCOPE target BEFORE the
    /// flat legacy source is ever touched; the destination directory is then
    /// `fsync`ed so the rename's directory-entry update survives a
    /// power-loss crash. Only after that does the flat source get removed
    /// (and the root directory `fsync`ed). A crash before the target rename
    /// leaves only the untouched legacy source (safely re-migrated on the
    /// next run, counted normally). A crash after the target rename but
    /// before the source removal leaves BOTH paths present; the next run
    /// detects the already-existing target, VERIFIES it actually decrypts
    /// and its plaintext still hashes to the expected digest (never trusting
    /// existence alone), and — only if verified — removes the stale flat
    /// source without re-decrypting or double-counting it.
    pub fn migrate_legacy_blobs(
        &self,
        legacy_master_key: [u8; 32],
    ) -> Result<u64, ArtifactStoreError> {
        let legacy_cipher =
            Aes256Gcm::new_from_slice(&legacy_master_key).expect("key is exactly 32 bytes");
        let mut count = 0u64;
        let entries = std::fs::read_dir(&self.root).map_err(|source| ArtifactStoreError::Io {
            path: self.root.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| ArtifactStoreError::Io {
                path: self.root.clone(),
                source,
            })?;
            let path = entry.path();
            if !path.is_file() {
                continue; // scope subdirs (and the sibling keys dir) are not flat blobs
            }
            if let Some(ext) = path.extension() {
                if ext.to_string_lossy().starts_with("tmp.") {
                    continue; // orphaned in-flight write; not a stored blob
                }
            }

            let Some(digest_hex) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !Self::is_valid_digest_hex(digest_hex) {
                continue;
            }
            let target_path = self.blob_path(SYSTEM_SCOPE, digest_hex);

            if target_path.exists() {
                // Crash-recovery: a prior run wrote the SYSTEM_SCOPE target
                // but crashed before removing the flat legacy source. Do NOT
                // trust existence alone — verify the target actually
                // decrypts under SYSTEM_SCOPE and its plaintext still hashes
                // to this digest before treating the flat source as
                // redundant. A corrupt or unrelated file at the target path
                // must never cause the still-good legacy source to be
                // deleted.
                let mut s = String::with_capacity(71);
                s.push_str("sha256:");
                s.push_str(digest_hex);
                let Ok(digest) = openspine_schemas::digest::Digest::parse(s) else {
                    continue;
                };
                let artifact_ref = ArtifactRef {
                    digest,
                    schema_version: 1,
                };
                self.get_scoped(SYSTEM_SCOPE, &artifact_ref)?;
                std::fs::remove_file(&path).map_err(|source| ArtifactStoreError::Io {
                    path: path.clone(),
                    source,
                })?;
                fsync_dir(&self.root)?;
                // Crash-recovery cleanup is not counted as a fresh migration.
                continue;
            }

            let blob = std::fs::read(&path).map_err(|source| ArtifactStoreError::Io {
                path: path.clone(),
                source,
            })?;
            if blob.is_empty() || blob.len() < NONCE_LEN {
                continue; // empty or too short to be a legacy `[nonce][ct]`
            }
            let (old_nonce_bytes, old_ciphertext) = blob.split_at(NONCE_LEN);
            let old_nonce = match Nonce::try_from(old_nonce_bytes) {
                Ok(n) => n,
                Err(_) => continue,
            };
            let Ok(plaintext) = legacy_cipher.decrypt(&old_nonce, old_ciphertext) else {
                continue; // not a legacy blob under this key; nothing to migrate
            };
            let mut s = String::with_capacity(71);
            s.push_str("sha256:");
            s.push_str(digest_hex);
            let Ok(digest) = openspine_schemas::digest::Digest::parse(s) else {
                continue;
            };
            if digest_of_bytes(&plaintext) != digest {
                continue;
            }

            let key = self.keys.get_or_create_key(SYSTEM_SCOPE)?;
            self.write_current_blob(&target_path, SYSTEM_SCOPE, &plaintext, &key)?;
            std::fs::remove_file(&path).map_err(|source| ArtifactStoreError::Io {
                path: path.clone(),
                source,
            })?;
            fsync_dir(&self.root)?;

            count += 1;
        }
        Ok(count)
    }
}

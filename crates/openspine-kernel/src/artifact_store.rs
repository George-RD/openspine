//! Encrypted content-addressed artifact blob store (PRD §18, build plan 4a).
//!
//! Every private payload (a raw Telegram message, an email body, a model
//! prompt/output, a draft body) is stored here, never as plaintext anywhere
//! else — audit rows, logs, and the wire only ever carry an [`ArtifactRef`].
//! Blobs are content-addressed by the digest of their *plaintext*, so the
//! same logical content always resolves to the same [`ArtifactRef`]
//! regardless of when or how many times it is stored.

use std::path::PathBuf;

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::digest_of_bytes;
use rand::Rng;

const NONCE_LEN: usize = 12;

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, thiserror::Error)]
pub enum ArtifactStoreError {
    #[error("artifact store I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to encrypt artifact blob")]
    Encrypt,
    #[error("failed to decrypt artifact blob (wrong key or corrupted blob)")]
    Decrypt,
    #[error("stored blob at {0} is shorter than the {NONCE_LEN}-byte nonce prefix")]
    Truncated(PathBuf),
    #[error("decrypted content digest does not match the requested artifact ref — store is corrupted or tampered")]
    DigestMismatch,
}

/// AES-256-GCM encrypted blob store at `<data_dir>/artifacts/<sha256-hex>`.
pub struct ArtifactStore {
    root: PathBuf,
    cipher: Aes256Gcm,
    #[cfg(test)]
    fault_put: std::sync::atomic::AtomicBool,
}

impl ArtifactStore {
    pub fn open(root: PathBuf, key: [u8; 32]) -> Result<Self, ArtifactStoreError> {
        std::fs::create_dir_all(&root).map_err(|source| ArtifactStoreError::Io {
            path: root.clone(),
            source,
        })?;
        let cipher = Aes256Gcm::new_from_slice(&key).expect("key is exactly 32 bytes");
        Ok(Self {
            root,
            cipher,
            #[cfg(test)]
            fault_put: std::sync::atomic::AtomicBool::new(false),
        })
    }

    fn blob_path(&self, digest_hex: &str) -> PathBuf {
        self.root.join(digest_hex)
    }
    /// Test-only: the on-disk path for `artifact_ref`'s encrypted blob, so
    /// tests can corrupt or remove it to exercise missing/corrupt reads.
    #[cfg(test)]
    pub(crate) fn blob_path_for_test(&self, artifact_ref: &ArtifactRef) -> PathBuf {
        self.blob_path(Self::digest_hex(&artifact_ref.digest))
    }

    fn digest_hex(digest: &openspine_schemas::digest::Digest) -> &str {
        digest
            .as_str()
            .strip_prefix("sha256:")
            .expect("Digest always carries the sha256: prefix")
    }

    /// Store `plaintext`, encrypting it under a fresh random nonce. Returns
    /// the [`ArtifactRef`] the caller must use to retrieve it. Idempotent:
    /// storing the same plaintext twice is a no-op the second time (the
    /// blob is content-addressed, so nothing new needs writing).
    pub fn put(&self, plaintext: &[u8]) -> Result<ArtifactRef, ArtifactStoreError> {
        #[cfg(test)]
        if self.fault_put.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(ArtifactStoreError::Io {
                path: self.root.clone(),
                source: std::io::Error::other("injected artifact put failure (test)"),
            });
        }
        let digest = digest_of_bytes(plaintext);
        let path = self.blob_path(Self::digest_hex(&digest));

        if !path.exists() {
            let mut nonce_bytes = [0u8; NONCE_LEN];
            rand::rng().fill_bytes(&mut nonce_bytes);
            let nonce = Nonce::try_from(nonce_bytes.as_slice()).expect("nonce is exactly 12 bytes");
            let ciphertext = self
                .cipher
                .encrypt(&nonce, plaintext)
                .map_err(|_| ArtifactStoreError::Encrypt)?;

            let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
            blob.extend_from_slice(&nonce_bytes);
            blob.extend_from_slice(&ciphertext);

            // Write and fsync the blob before publishing its name, then fsync
            // the containing directory so the rename survives power loss.
            let tmp_path = path.with_extension(format!("tmp.{}", hex_encode(&nonce_bytes)));
            std::fs::write(&tmp_path, &blob).map_err(|source| ArtifactStoreError::Io {
                path: tmp_path.clone(),
                source,
            })?;
            std::fs::File::open(&tmp_path)
                .and_then(|file| file.sync_all())
                .map_err(|source| ArtifactStoreError::Io {
                    path: tmp_path.clone(),
                    source,
                })?;
            std::fs::rename(&tmp_path, &path).map_err(|source| ArtifactStoreError::Io {
                path: path.clone(),
                source,
            })?;
            std::fs::File::open(&self.root)
                .and_then(|directory| directory.sync_all())
                .map_err(|source| ArtifactStoreError::Io {
                    path: self.root.clone(),
                    source,
                })?;
        }
        // Also re-sync an existing blob and directory entry. This closes the
        // rename-success/directory-fsync-failure retry hole and upgrades blobs
        // written by older versions of the store.
        std::fs::File::open(&path)
            .and_then(|file| file.sync_all())
            .map_err(|source| ArtifactStoreError::Io {
                path: path.clone(),
                source,
            })?;
        std::fs::File::open(&self.root)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| ArtifactStoreError::Io {
                path: self.root.clone(),
                source,
            })?;

        Ok(ArtifactRef {
            digest,
            schema_version: 1,
        })
    }

    /// Retrieve and decrypt the plaintext for `artifact_ref`, verifying the
    /// decrypted content's digest matches the ref before returning it.
    pub fn get(&self, artifact_ref: &ArtifactRef) -> Result<Vec<u8>, ArtifactStoreError> {
        let hex = Self::digest_hex(&artifact_ref.digest);
        let path = self.blob_path(hex);
        let blob = std::fs::read(&path).map_err(|source| ArtifactStoreError::Io {
            path: path.clone(),
            source,
        })?;

        if blob.len() < NONCE_LEN {
            return Err(ArtifactStoreError::Truncated(path));
        }
        let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
        let nonce = Nonce::try_from(nonce_bytes)
            .map_err(|_| ArtifactStoreError::Truncated(path.clone()))?;
        let plaintext = self
            .cipher
            .decrypt(&nonce, ciphertext)
            .map_err(|_| ArtifactStoreError::Decrypt)?;

        if digest_of_bytes(&plaintext) != artifact_ref.digest {
            return Err(ArtifactStoreError::DigestMismatch);
        }
        Ok(plaintext)
    }
}

#[cfg(test)]
impl ArtifactStore {
    /// Test-only: write `plaintext` (encrypted) at the content-addressed path
    /// for `digest`, bypassing the normal "match the bytes to the digest" check
    /// that `put` enforces. Lets a test stage a stored blob whose decrypted
    /// bytes do NOT hash to `digest`, so `get` returns `DigestMismatch` — the
    /// exact condition `create_approved_draft` must deny on (D-055.4).
    pub(crate) fn put_tampered_for_test(
        &self,
        digest: &openspine_schemas::digest::Digest,
        plaintext: &[u8],
    ) -> Result<(), ArtifactStoreError> {
        let hex = Self::digest_hex(digest);
        let path = self.blob_path(hex);
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::try_from(nonce_bytes.as_slice()).expect("nonce is exactly 12 bytes");
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| ArtifactStoreError::Encrypt)?;
        let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ciphertext);
        std::fs::write(&path, &blob).map_err(|source| ArtifactStoreError::Io { path, source })?;
        Ok(())
    }
    /// Test-only: make subsequent `put` calls fail with an I/O error, so a
    /// caller can exercise retryable-on-artifact-failure paths. Per-instance,
    /// so concurrent tests using other stores are unaffected.
    pub(crate) fn set_fault_put_for_test(&self, fail: bool) {
        self.fault_put
            .store(fail, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; 32] {
        [7u8; 32]
    }

    #[test]
    fn round_trips_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let plaintext = b"hello, owner";
        let artifact_ref = store.put(plaintext).unwrap();
        let back = store.get(&artifact_ref).unwrap();
        assert_eq!(back, plaintext);
    }

    #[test]
    fn same_content_is_content_addressed() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let a = store.put(b"same bytes").unwrap();
        let b = store.put(b"same bytes").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn different_content_is_different_ref() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let a = store.put(b"one").unwrap();
        let b = store.put(b"two").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn stored_blob_never_contains_the_plaintext_substring() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("artifacts");
        let store = ArtifactStore::open(root.clone(), key()).unwrap();
        let secret = b"my telegram message is very secret";
        let artifact_ref = store.put(secret).unwrap();
        let hex = artifact_ref
            .digest
            .as_str()
            .strip_prefix("sha256:")
            .unwrap();
        let on_disk = std::fs::read(root.join(hex)).unwrap();
        assert!(!on_disk.windows(secret.len()).any(|window| window == secret));
    }

    #[test]
    fn wrong_key_fails_to_decrypt() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("artifacts");
        let store_a = ArtifactStore::open(root.clone(), [1u8; 32]).unwrap();
        let artifact_ref = store_a.put(b"top secret").unwrap();

        let store_b = ArtifactStore::open(root, [2u8; 32]).unwrap();
        let result = store_b.get(&artifact_ref);
        assert!(matches!(result, Err(ArtifactStoreError::Decrypt)));
    }

    #[test]
    fn get_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let artifact_ref = store.put(b"repeat read").unwrap();
        assert_eq!(
            store.get(&artifact_ref).unwrap(),
            store.get(&artifact_ref).unwrap()
        );
    }
}

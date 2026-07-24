//! Kernel-owned encrypted connector secret vault (D-014/D-025).
//!
//! Values are name-addressed and mutable (rotation overwrites a slot), unlike
//! the immutable content-addressed [`crate::artifact_store::ArtifactStore`].
//! Plaintext is returned only to kernel-owned connector code.

use std::path::PathBuf;

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use openspine_schemas::digest::{digest_of_bytes, Digest};
use rand::Rng;

const NONCE_LEN: usize = 12;
const MAX_SLOT_LEN: usize = 96;

#[derive(Debug, thiserror::Error)]
pub enum SecretStoreError {
    #[error("secret store I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("secret slot name is invalid")]
    InvalidSlot,
    #[error("stored secret is truncated")]
    Truncated,
    #[error("stored secret could not be decrypted")]
    Decrypt,
    #[error("stored secret could not be encrypted")]
    Encrypt,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OAuthTokens {
    pub provider_id: String,
    pub refresh_token: String,
    pub access_token: String,
    pub expires_at: String,
    pub account_email: Option<String>,
    pub account_id: Option<String>,
    pub identity_key: Option<String>,
    pub disabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct OAuthIdentityMetadata {
    pub account_email: Option<String>,
    pub account_id: Option<String>,
    pub identity_key: Option<String>,
}

#[derive(Clone)]
pub struct SecretStore {
    root: PathBuf,
    cipher: Aes256Gcm,
    #[cfg(test)]
    fault_put: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    #[cfg(test)]
    fault_delete: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl SecretStore {
    pub fn open(root: PathBuf, key: [u8; 32]) -> Result<Self, SecretStoreError> {
        std::fs::create_dir_all(&root).map_err(|source| SecretStoreError::Io {
            path: root.clone(),
            source,
        })?;
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| SecretStoreError::Encrypt)?;
        Ok(Self {
            root,
            cipher,
            #[cfg(test)]
            fault_put: std::sync::Arc::new(std::sync::Mutex::new(None)),
            #[cfg(test)]
            fault_delete: std::sync::Arc::new(std::sync::Mutex::new(None)),
        })
    }

    pub fn validate_slot(slot: &str) -> bool {
        !slot.is_empty()
            && slot.len() <= MAX_SLOT_LEN
            && slot.as_bytes()[0].is_ascii_alphanumeric()
            && slot != "."
            && slot != ".."
            && slot
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
    }

    fn path(&self, slot: &str) -> Result<PathBuf, SecretStoreError> {
        if !Self::validate_slot(slot) {
            return Err(SecretStoreError::InvalidSlot);
        }
        Ok(self.root.join(slot))
    }

    pub fn contains(&self, slot: &str) -> Result<bool, SecretStoreError> {
        Ok(self.path(slot)?.exists())
    }

    pub fn put(&self, slot: &str, plaintext: &[u8]) -> Result<Digest, SecretStoreError> {
        #[cfg(test)]
        {
            let mut guard = self.fault_put.lock().expect("fault_put mutex poisoned");
            if guard.as_deref() == Some(slot) {
                let _ = guard.take();
                return Err(SecretStoreError::Io {
                    path: self.path(slot)?,
                    source: std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "injected put failure",
                    ),
                });
            }
        }
        let path = self.path(slot)?;
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce =
            Nonce::try_from(nonce_bytes.as_slice()).map_err(|_| SecretStoreError::Encrypt)?;
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| SecretStoreError::Encrypt)?;
        let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ciphertext);
        let tmp = path.with_extension(format!("tmp.{}", hex(&nonce_bytes)));
        std::fs::write(&tmp, blob).map_err(|source| SecretStoreError::Io {
            path: tmp.clone(),
            source,
        })?;
        std::fs::rename(&tmp, &path).map_err(|source| SecretStoreError::Io { path, source })?;
        Ok(digest_of_bytes(plaintext))
    }

    pub fn delete(&self, slot: &str) -> Result<(), SecretStoreError> {
        #[cfg(test)]
        {
            let mut guard = self
                .fault_delete
                .lock()
                .expect("fault_delete mutex poisoned");
            if guard.as_deref() == Some(slot) {
                let _ = guard.take();
                return Err(SecretStoreError::Io {
                    path: self.path(slot)?,
                    source: std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "injected delete failure",
                    ),
                });
            }
        }
        let path = self.path(slot)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(_source) if !path.exists() => Ok(()),
            Err(source) => Err(SecretStoreError::Io { path, source }),
        }
    }

    pub fn seed_if_absent(&self, slot: &str, plaintext: &[u8]) -> Result<bool, SecretStoreError> {
        if self.contains(slot)? {
            return Ok(false);
        }
        self.put(slot, plaintext)?;
        Ok(true)
    }

    pub fn get_with_version(
        &self,
        slot: &str,
    ) -> Result<Option<(Vec<u8>, Digest)>, SecretStoreError> {
        let path = self.path(slot)?;
        if !path.exists() {
            return Ok(None);
        }
        let blob = std::fs::read(&path).map_err(|source| SecretStoreError::Io {
            path: path.clone(),
            source,
        })?;
        if blob.len() < NONCE_LEN {
            return Err(SecretStoreError::Truncated);
        }
        let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
        let nonce = Nonce::try_from(nonce_bytes).map_err(|_| SecretStoreError::Truncated)?;
        let plaintext = self
            .cipher
            .decrypt(&nonce, ciphertext)
            .map_err(|_| SecretStoreError::Decrypt)?;
        let version = digest_of_bytes(&plaintext);
        Ok(Some((plaintext, version)))
    }

    #[allow(dead_code)]
    pub fn get(&self, slot: &str) -> Result<Option<Vec<u8>>, SecretStoreError> {
        self.get_with_version(slot)
            .map(|value| value.map(|(bytes, _)| bytes))
    }
    #[allow(dead_code)]
    pub fn version(&self, slot: &str) -> Result<Option<Digest>, SecretStoreError> {
        self.get_with_version(slot)
            .map(|value| value.map(|(_, version)| version))
    }

    pub fn get_string(&self, slot: &str) -> Result<Option<String>, SecretStoreError> {
        self.get_with_version(slot)?
            .map(|(bytes, _)| String::from_utf8(bytes).map_err(|_| SecretStoreError::Decrypt))
            .transpose()
    }
    pub fn store_oauth_tokens(
        &self,
        provider_id: &str,
        refresh_token: &str,
        access_token: &str,
        expires_at: &str,
        identity_metadata: Option<OAuthIdentityMetadata>,
    ) -> Result<(), SecretStoreError> {
        let meta = identity_metadata.unwrap_or_default();
        self.put(&format!("provider.{provider_id}.auth_mode"), b"oauth")?;
        self.put(
            &format!("provider.{provider_id}.refresh_token"),
            refresh_token.as_bytes(),
        )?;
        self.put(
            &format!("provider.{provider_id}.access_token"),
            access_token.as_bytes(),
        )?;
        self.put(
            &format!("provider.{provider_id}.expires_at"),
            expires_at.as_bytes(),
        )?;
        self.put(&format!("provider.{provider_id}.disabled"), b"false")?;

        if let Some(email) = meta.account_email {
            self.put(
                &format!("provider.{provider_id}.account_email"),
                email.as_bytes(),
            )?;
        }
        if let Some(id) = meta.account_id {
            self.put(&format!("provider.{provider_id}.account_id"), id.as_bytes())?;
        }
        if let Some(key) = meta.identity_key {
            self.put(
                &format!("provider.{provider_id}.identity_key"),
                key.as_bytes(),
            )?;
        }
        Ok(())
    }

    pub fn get_oauth_tokens(
        &self,
        provider_id: &str,
    ) -> Result<Option<OAuthTokens>, SecretStoreError> {
        let refresh_token =
            match self.get_string(&format!("provider.{provider_id}.refresh_token"))? {
                Some(t) => t,
                None => {
                    match self.get_string(&format!("provider.{provider_id}.oauth.refresh_token"))? {
                        Some(t) => t,
                        None => return Ok(None),
                    }
                }
            };

        let access_token = self
            .get_string(&format!("provider.{provider_id}.access_token"))?
            .or_else(|| {
                self.get_string(&format!("provider.{provider_id}.oauth.access_token"))
                    .ok()
                    .flatten()
            })
            .unwrap_or_default();

        let expires_at = self
            .get_string(&format!("provider.{provider_id}.expires_at"))?
            .or_else(|| {
                self.get_string(&format!("provider.{provider_id}.oauth.expires_at"))
                    .ok()
                    .flatten()
            })
            .unwrap_or_default();

        let disabled = self
            .get_string(&format!("provider.{provider_id}.disabled"))?
            .map(|s| s == "true")
            .unwrap_or(false);

        let account_email = self.get_string(&format!("provider.{provider_id}.account_email"))?;
        let account_id = self.get_string(&format!("provider.{provider_id}.account_id"))?;
        let identity_key = self.get_string(&format!("provider.{provider_id}.identity_key"))?;

        Ok(Some(OAuthTokens {
            provider_id: provider_id.to_string(),
            refresh_token,
            access_token,
            expires_at,
            account_email,
            account_id,
            identity_key,
            disabled,
        }))
    }

    pub fn update_access_token(
        &self,
        provider_id: &str,
        access_token: &str,
        expires_at: &str,
    ) -> Result<(), SecretStoreError> {
        self.put(
            &format!("provider.{provider_id}.access_token"),
            access_token.as_bytes(),
        )?;
        self.put(
            &format!("provider.{provider_id}.expires_at"),
            expires_at.as_bytes(),
        )?;
        Ok(())
    }

    pub fn disable_oauth_credential(
        &self,
        provider_id: &str,
        cause: &str,
    ) -> Result<(), SecretStoreError> {
        self.put(&format!("provider.{provider_id}.disabled"), b"true")?;
        self.put(
            &format!("provider.{provider_id}.disable_cause"),
            cause.as_bytes(),
        )?;
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn arm_fault_put(&self, slot: &str) {
        *self.fault_put.lock().expect("fault_put mutex poisoned") = Some(slot.to_string());
    }
    #[cfg(test)]
    pub(crate) fn arm_fault_delete(&self, slot: &str) {
        *self
            .fault_delete
            .lock()
            .expect("fault_delete mutex poisoned") = Some(slot.to_string());
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_introduction_and_rotation_are_decryptable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SecretStore::open(dir.path().join("credentials"), [7; 32]).expect("open");
        assert!(store.put("gmail.refresh", b"first").is_ok());
        assert_eq!(
            store.get_string("gmail.refresh").expect("get").as_deref(),
            Some("first")
        );
        let first = store.version("gmail.refresh").expect("version");
        assert!(store.put("gmail.refresh", b"second").is_ok());
        assert_eq!(
            store.get_string("gmail.refresh").expect("get").as_deref(),
            Some("second")
        );
        assert_ne!(first, store.version("gmail.refresh").expect("version"));
    }

    #[test]
    fn seed_is_idempotent_and_slot_validation_is_strict() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SecretStore::open(dir.path().join("credentials"), [8; 32]).expect("open");
        assert!(store.seed_if_absent("telegram.bot", b"one").expect("seed"));
        assert!(!store.seed_if_absent("telegram.bot", b"two").expect("seed"));
        assert_eq!(
            store.get_string("telegram.bot").expect("get").as_deref(),
            Some("one")
        );
        assert!(!SecretStore::validate_slot("../escape"));
        assert!(!SecretStore::validate_slot("."));
        assert!(!SecretStore::validate_slot(".."));
        assert!(!SecretStore::validate_slot(""));
    }

    #[test]
    fn truncated_or_corrupted_ciphertext_fails_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("credentials");
        let store = SecretStore::open(root.clone(), [9; 32]).expect("open");
        std::fs::write(root.join("broken"), [1_u8, 2, 3]).expect("write broken");
        assert!(matches!(
            store.get("broken"),
            Err(SecretStoreError::Truncated)
        ));
        std::fs::write(root.join("corrupt"), [0_u8; 32]).expect("write corrupt");
        assert!(matches!(
            store.get("corrupt"),
            Err(SecretStoreError::Decrypt)
        ));
    }

    #[test]
    fn oauth_tokens_stored_encrypted_in_secret_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SecretStore::open(dir.path().join("credentials"), [12; 32]).expect("open");

        let meta = OAuthIdentityMetadata {
            account_email: Some("user@example.com".to_string()),
            account_id: Some("acc-123".to_string()),
            identity_key: Some("email:user@example.com".to_string()),
        };

        store
            .store_oauth_tokens(
                "google-antigravity",
                "refresh-secret-token",
                "access-secret-token",
                "2026-07-24T18:00:00Z",
                Some(meta),
            )
            .expect("store oauth tokens");

        let raw = std::fs::read_to_string(
            dir.path()
                .join("credentials/provider.google-antigravity.refresh_token"),
        )
        .unwrap_or_default();
        assert!(
            !raw.contains("refresh-secret-token"),
            "secret must not be plaintext on disk"
        );

        let loaded = store
            .get_oauth_tokens("google-antigravity")
            .expect("load")
            .expect("some");
        assert_eq!(loaded.refresh_token, "refresh-secret-token");
        assert_eq!(loaded.access_token, "access-secret-token");
        assert_eq!(loaded.account_email.as_deref(), Some("user@example.com"));
        assert!(!loaded.disabled);

        store
            .update_access_token(
                "google-antigravity",
                "new-access-token",
                "2026-07-24T19:00:00Z",
            )
            .expect("update access token");
        let updated = store
            .get_oauth_tokens("google-antigravity")
            .expect("load")
            .expect("some");
        assert_eq!(updated.access_token, "new-access-token");
        assert_eq!(updated.refresh_token, "refresh-secret-token");

        store
            .disable_oauth_credential("google-antigravity", "revoked")
            .expect("disable");
        let disabled = store
            .get_oauth_tokens("google-antigravity")
            .expect("load")
            .expect("some");
        assert!(disabled.disabled);
    }

    #[test]
    fn secret_store_oauth_keys_are_isolated_per_provider() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SecretStore::open(dir.path().join("credentials"), [13; 32]).expect("open");

        store
            .store_oauth_tokens("provider-a", "ref-a", "acc-a", "1000", None)
            .expect("store a");
        store
            .store_oauth_tokens("provider-b", "ref-b", "acc-b", "2000", None)
            .expect("store b");

        let tokens_a = store.get_oauth_tokens("provider-a").unwrap().unwrap();
        let tokens_b = store.get_oauth_tokens("provider-b").unwrap().unwrap();

        assert_eq!(tokens_a.refresh_token, "ref-a");
        assert_eq!(tokens_b.refresh_token, "ref-b");
    }
}

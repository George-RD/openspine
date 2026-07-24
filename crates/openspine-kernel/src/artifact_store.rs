// openspine:allow-large-module reason: cohesive encrypted blob store implementing the on-disk format, per-counterparty key wrapping, and legacy migration in one module; splitting would separate the format constants from their only reader/writer.
//! Encrypted content-addressed artifact blob store (PRD §18, build plan 4a).
//!
//! Every private payload (a raw Telegram message, an email body, a model
//! prompt/output, a draft body) is stored here, never as plaintext anywhere
//! else — audit rows, logs, and the wire only ever carry an [`ArtifactRef`].
//! Blobs are content-addressed by the digest of their *plaintext* WITHIN a
//! counterparty scope: the same logical content stored by the SAME
//! counterparty always resolves to the same [`ArtifactRef`] and the same
//! on-disk blob, but two counterparties storing identical plaintext get
//! separate, independently-keyed blobs. Sharing one blob under the first
//! writer's key would let one counterparty's erasure collaterally destroy
//! (or fail to destroy) another counterparty's data.
//!
//! **Per-counterparty encryption (AD-140).** Each blob is encrypted under a
//! *per-counterparty* AES-256-GCM key held by the [`CounterpartyKeyRing`],
//! whose wrapped key files live in a SEPARATE sibling `<data_dir>/keys`
//! directory — never nested under the blob tree, because AD-139 treats
//! "SQLite DB + artifact blobs + keys" as three independent elements of one
//! backup/restore snapshot set. The blob path itself is `(scope, digest)`
//! keyed (`<data_dir>/artifacts/<scope-ulid>/<sha256-hex>`), and the
//! counterparty id is ALSO recorded in the on-disk blob header — redundant
//! with the path, kept as a defense-in-depth cross-check: [`ArtifactStore::get_scoped`]
//! refuses to decrypt if the two disagree. Deleting a counterparty's key
//! makes every blob encrypted under it permanently undecryptable while the
//! audit hash chain (which only grows, never mutates) keeps its tamper
//! evidence intact.
//!
//! The legacy single global key (pre-AD-140) is migrated into the new format
//! by [`ArtifactStore::migrate_legacy_blobs`], re-keying every old blob under
//! the reserved `SYSTEM_SCOPE` scope. [`ArtifactStore::get`]/[`ArtifactStore::put`]
//! are thin `SYSTEM_SCOPE` convenience wrappers around
//! `get_scoped`/`put_scoped`, so every pre-existing caller — which only ever
//! stores owner-authored or internal payloads — is unaffected by the
//! per-counterparty model.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::digest_of_bytes;
use rand::Rng;
use ulid::Ulid;

use crate::counterparty_keys::{CounterpartyKeyError, CounterpartyKeyRing, SYSTEM_SCOPE};

const NONCE_LEN: usize = 12;
/// Length of a serialized `Ulid` scope id stored in each blob header.
const SCOPE_LEN: usize = 16;
/// Format 2 is `[tag=2:1][scope:16][nonce:12][ciphertext]`, unauthenticated AAD.
const RECOVERED_FORMAT: u8 = 2;
/// Format 3 is `[tag=3:1][scope:16][nonce:12][ciphertext]`, with `[tag=3:1][scope:16]` as AEAD associated data.
const CURRENT_FORMAT: u8 = 3;
const UPGRADE_PENDING_EXTENSION: &str = "upgrade-pending";

#[cfg(test)]
const FORMAT_TAG: u8 = CURRENT_FORMAT;
const HEADER_LEN: usize = 1 + SCOPE_LEN;
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Directory fsync: makes a preceding rename's/write's directory-entry
/// update durable against a power-loss crash, not just the renamed file's
/// own `sync_all`. Callers (`put_scoped`, `migrate_legacy_blobs`) promise
/// their `ArtifactRef`/migrated-blob durability BEFORE returning, so a
/// failure here is surfaced via `?`, never silently swallowed — an
/// `Ok(ref)` returned after a failed durability sync would be a false
/// promise.
fn fsync_dir(dir: &Path) -> Result<(), ArtifactStoreError> {
    let f = std::fs::File::open(dir).map_err(|source| ArtifactStoreError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    f.sync_all().map_err(|source| ArtifactStoreError::Io {
        path: dir.to_path_buf(),
        source,
    })
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
    #[error("stored blob at {0} uses an unknown format tag")]
    UnknownFormat(PathBuf),
    #[error(
        "decrypted content digest does not match the requested artifact ref — store is corrupted or tampered"
    )]
    DigestMismatch,
    /// Wraps a key-ring error (e.g. a crypto-erased counterparty whose key
    /// can no longer be recovered).
    #[error("counterparty payload key error: {0}")]
    KeyRing(#[from] CounterpartyKeyError),
}

/// AES-256-GCM encrypted blob store at `<data_dir>/artifacts/<scope>/<sha256-hex>`.
///
/// Holds a [`CounterpartyKeyRing`] (keyed by the master key once passed to
/// [`Self::open`]) rather than a single cipher, so payloads can be encrypted
/// per counterparty.
pub struct ArtifactStore {
    root: PathBuf,
    keys: CounterpartyKeyRing,
    #[cfg(test)]
    fault_put: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    fault_existing_blob_sync: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    fault_clear_upgrade_pending_sync: std::sync::atomic::AtomicBool,
}

impl ArtifactStore {
    pub fn open(root: PathBuf, master_key: [u8; 32]) -> Result<Self, ArtifactStoreError> {
        std::fs::create_dir_all(&root).map_err(|source| ArtifactStoreError::Io {
            path: root.clone(),
            source,
        })?;
        // Keys live in a SEPARATE directory, never nested under the
        // artifacts/blobs tree: AD-139 treats "SQLite DB + artifact blobs
        // + keys" as THREE independent elements of one backup/restore
        // snapshot set. The sibling `<data_dir>/keys` layout applies ONLY
        // when `root` is the conventional `<data_dir>/artifacts` directory
        // (filename == "artifacts") — that is the production shape and the
        // design's stated path. Any other root (notably a `tempfile`
        // dir whose parent is a shared system temp) MUST fall back to a
        // `keys` subdir UNDER `root`, so independent stores never collapse
        // onto one shared parent `keys` directory and clobber each
        // other's keys (a cross-test/data-leakage trap).
        let keys_dir = if root.file_name().and_then(|n| n.to_str()) == Some("artifacts") {
            root.parent()
                .map(|parent| parent.join("keys"))
                .unwrap_or_else(|| root.join("keys"))
        } else {
            root.join("keys")
        };
        let keys = CounterpartyKeyRing::open(keys_dir, master_key)?;
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

    /// The on-disk path for `digest_hex`'s blob under `scope`. Content
    /// addressing is `(scope, digest)`-keyed, not digest-only: two
    /// counterparties storing identical plaintext get the SAME `ArtifactRef`
    /// (digest) but separate, independently-keyed blobs at separate paths —
    /// erasing one counterparty's key can never affect the other's copy, and
    /// can never leave the erased counterparty's own copy recoverable
    /// through another counterparty's still-live key.
    fn blob_path(&self, scope: Ulid, digest_hex: &str) -> PathBuf {
        self.root.join(scope.to_string()).join(digest_hex)
    }

    /// Test-only: the on-disk path for `artifact_ref`'s `SYSTEM_SCOPE`
    /// blob, so tests can corrupt or remove it to exercise missing/corrupt
    /// reads. Matches [`Self::get`]'s implicit `SYSTEM_SCOPE`.
    #[cfg(test)]
    pub(crate) fn blob_path_for_test(&self, artifact_ref: &ArtifactRef) -> PathBuf {
        self.blob_path(SYSTEM_SCOPE, Self::digest_hex(&artifact_ref.digest))
    }

    /// Test-only: the on-disk path for `artifact_ref`'s blob under `scope`,
    /// for tests exercising per-counterparty storage.
    #[cfg(test)]
    pub(crate) fn blob_path_for_test_scoped(
        &self,
        scope: Ulid,
        artifact_ref: &ArtifactRef,
    ) -> PathBuf {
        self.blob_path(scope, Self::digest_hex(&artifact_ref.digest))
    }

    /// Header-reading attribution API (AD-140; restores the attribution
    /// primitive the design mandates). Reads the counterparty id embedded
    /// in `artifact_ref`'s blob header WITHOUT touching any key material,
    /// so a caller can attribute a stored payload to its producing
    /// counterparty even after that counterparty's key has been
    /// crypto-erased (the plaintext is gone, but the on-disk header
    /// attribution survives — D-012). The embedded scope MUST match the
    /// `scope` the caller is asking about; a mismatch means a foreign or
    /// corrupted file landed at this `(scope, ref)` path and is reported
    /// as `UnknownFormat` rather than silently trusted.
    #[allow(dead_code)] // public kernel primitive; production erase action is a follow-up
    pub fn scope_of(
        &self,
        scope: Ulid,
        artifact_ref: &ArtifactRef,
    ) -> Result<Ulid, ArtifactStoreError> {
        self.keys.with_scope_lock(scope, || {
            let path = self.blob_path(scope, Self::digest_hex(&artifact_ref.digest));
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
            if blob.len() < HEADER_LEN {
                return Err(ArtifactStoreError::Truncated(path));
            }
            let mut header_scope = [0u8; SCOPE_LEN];
            header_scope.copy_from_slice(&blob[1..HEADER_LEN]);
            let header_scope = Ulid::from_bytes(header_scope);
            if header_scope != scope {
                // The blob's own header disagrees with the path/scope the caller
                // asked about. Refuse rather than attribute ambiguously.
                return Err(ArtifactStoreError::UnknownFormat(path));
            }
            if tag == CURRENT_FORMAT {
                self.recover_pending_upgrade(&path)?;
            }
            Ok(header_scope)
        })
    }

    fn digest_hex(digest: &openspine_schemas::digest::Digest) -> &str {
        digest
            .as_str()
            .strip_prefix("sha256:")
            .expect("Digest always carries the sha256: prefix")
    }

    fn is_valid_digest_hex(hex: &str) -> bool {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    }

    fn blob_header(format: u8, scope: Ulid) -> [u8; HEADER_LEN] {
        let mut header = [0u8; HEADER_LEN];
        header[0] = format;
        header[1..].copy_from_slice(&scope.to_bytes());
        header
    }

    fn upgrade_pending_path(path: &Path) -> PathBuf {
        path.with_extension(UPGRADE_PENDING_EXTENSION)
    }

    fn persist_upgrade_pending(&self, path: &Path) -> Result<(), ArtifactStoreError> {
        let marker_path = Self::upgrade_pending_path(path);
        let marker =
            std::fs::File::create(&marker_path).map_err(|source| ArtifactStoreError::Io {
                path: marker_path.clone(),
                source,
            })?;
        marker.sync_all().map_err(|source| ArtifactStoreError::Io {
            path: marker_path,
            source,
        })?;
        let scope_dir = path
            .parent()
            .expect("blob_path always nests under a scope subdirectory");
        fsync_dir(scope_dir)?;
        fsync_dir(&self.root)
    }

    fn clear_upgrade_pending(&self, path: &Path) -> Result<(), ArtifactStoreError> {
        let marker_path = Self::upgrade_pending_path(path);
        match std::fs::remove_file(&marker_path) {
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(ArtifactStoreError::Io {
                    path: marker_path.clone(),
                    source,
                });
            }
        }
        #[cfg(test)]
        if self
            .fault_clear_upgrade_pending_sync
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            // Recreate a durable marker before surfacing the clear failure so
            // a later current-format read still retries recovery.
            self.persist_upgrade_pending(path)?;
            return Err(ArtifactStoreError::Io {
                path: marker_path,
                source: std::io::Error::other(
                    "injected clear-upgrade-pending durability failure (test)",
                ),
            });
        }
        if let Err(err) = {
            let scope_dir = path
                .parent()
                .expect("blob_path always nests under a scope subdirectory");
            fsync_dir(scope_dir).and_then(|_| fsync_dir(&self.root))
        } {
            // Unlink already happened; the clear is not durable until the
            // directory entries are synced. Recreate the marker with the
            // same durable helper used on the happy path so a recovery
            // failure never strands the next current-format read without a
            // pending marker. Prefer reporting recreate failure if that is
            // what leaves the store state unknown.
            self.persist_upgrade_pending(path)?;
            return Err(err);
        }
        Ok(())
    }

    fn upgrade_pending(&self, path: &Path) -> Result<bool, ArtifactStoreError> {
        Self::upgrade_pending_path(path)
            .try_exists()
            .map_err(|source| ArtifactStoreError::Io {
                path: Self::upgrade_pending_path(path),
                source,
            })
    }

    fn maybe_fail_existing_blob_sync(&self, _path: &Path) -> Result<(), ArtifactStoreError> {
        #[cfg(test)]
        if self
            .fault_existing_blob_sync
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(ArtifactStoreError::Io {
                path: _path.to_path_buf(),
                source: std::io::Error::other("injected existing-blob sync failure (test)"),
            });
        }
        Ok(())
    }

    fn sync_existing_blob(&self, path: &Path) -> Result<(), ArtifactStoreError> {
        let file = std::fs::File::open(path).map_err(|source| ArtifactStoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        file.sync_all().map_err(|source| ArtifactStoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        self.maybe_fail_existing_blob_sync(path)?;
        let scope_dir = path
            .parent()
            .expect("blob_path always nests under a scope subdirectory");
        fsync_dir(scope_dir)?;
        fsync_dir(&self.root)
    }

    fn recover_pending_upgrade(&self, path: &Path) -> Result<(), ArtifactStoreError> {
        if self.upgrade_pending(path)? {
            self.sync_existing_blob(path)?;
            self.clear_upgrade_pending(path)?;
        }
        Ok(())
    }

    fn write_current_blob(
        &self,
        path: &Path,
        scope: Ulid,
        plaintext: &[u8],
        key: &[u8; 32],
    ) -> Result<(), ArtifactStoreError> {
        let scope_dir = path
            .parent()
            .expect("blob_path always nests under a scope subdirectory");
        std::fs::create_dir_all(scope_dir).map_err(|source| ArtifactStoreError::Io {
            path: scope_dir.to_path_buf(),
            source,
        })?;

        let cipher = Aes256Gcm::new_from_slice(key).expect("key is exactly 32 bytes");
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::try_from(nonce_bytes.as_slice()).expect("nonce is exactly 12 bytes");
        let header = Self::blob_header(CURRENT_FORMAT, scope);
        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: &header,
                },
            )
            .map_err(|_| ArtifactStoreError::Encrypt)?;

        let mut blob = Vec::with_capacity(HEADER_LEN + NONCE_LEN + ciphertext.len());
        blob.extend_from_slice(&header);
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ciphertext);

        let tmp_path = path.with_extension(format!("tmp.{}", hex_encode(&nonce_bytes)));
        {
            let mut tmp_file =
                std::fs::File::create(&tmp_path).map_err(|source| ArtifactStoreError::Io {
                    path: tmp_path.clone(),
                    source,
                })?;
            tmp_file
                .write_all(&blob)
                .and_then(|_| tmp_file.sync_all())
                .map_err(|source| ArtifactStoreError::Io {
                    path: tmp_path.clone(),
                    source,
                })?;
        }
        std::fs::rename(&tmp_path, path).map_err(|source| ArtifactStoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        self.maybe_fail_existing_blob_sync(path)?;
        fsync_dir(scope_dir)?;
        fsync_dir(&self.root)
    }
}

#[path = "artifact_store_io.rs"]
mod artifact_store_io;
#[path = "artifact_store_migration.rs"]
mod artifact_store_migration;

#[cfg(test)]
impl ArtifactStore {
    /// Test-only: write `plaintext` (encrypted, under `SYSTEM_SCOPE`) at the
    /// content-addressed path for `digest`, bypassing the normal "match the
    /// bytes to the digest" check that `put` enforces. Lets a test stage a
    /// stored blob whose decrypted bytes do NOT hash to `digest`, so `get`
    /// returns `DigestMismatch` — the exact condition `create_approved_draft`
    /// must deny on (D-055.4).
    pub(crate) fn put_tampered_for_test(
        &self,
        digest: &openspine_schemas::digest::Digest,
        plaintext: &[u8],
    ) -> Result<(), ArtifactStoreError> {
        let hex = Self::digest_hex(digest);
        let path = self.blob_path(SYSTEM_SCOPE, hex);
        let key = self.keys.get_or_create_key(SYSTEM_SCOPE)?;
        self.write_current_blob(&path, SYSTEM_SCOPE, plaintext, &key)
    }

    pub(crate) fn set_fault_existing_blob_sync_for_test(&self, fail: bool) {
        self.fault_existing_blob_sync
            .store(fail, std::sync::atomic::Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn set_fault_clear_upgrade_pending_sync_for_test(&self, fail: bool) {
        self.fault_clear_upgrade_pending_sync
            .store(fail, std::sync::atomic::Ordering::SeqCst);
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
#[path = "artifact_store_tests.rs"]
mod tests;

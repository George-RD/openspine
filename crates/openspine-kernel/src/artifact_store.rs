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
    #[error("decrypted content digest does not match the requested artifact ref — store is corrupted or tampered")]
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
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let secret = b"my telegram message is very secret";
        let artifact_ref = store.put(secret).unwrap();
        let on_disk = std::fs::read(store.blob_path_for_test(&artifact_ref)).unwrap();
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
        // A wrong master key cannot unwrap the wrapped per-counterparty key,
        // so the failure surfaces as `KeyRing(Decrypt)` (the key ring layer)
        // rather than a blob-level `Decrypt` -- either way it fails closed.
        assert!(matches!(
            result,
            Err(ArtifactStoreError::Decrypt | ArtifactStoreError::KeyRing(_))
        ));
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

    #[test]
    fn per_counterparty_keys_do_not_collide_on_identical_content() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let alice = Ulid::new();
        let bob = Ulid::new();
        let a = store.put_scoped(alice, b"same text").unwrap();
        let b = store.put_scoped(bob, b"same text").unwrap();
        assert_eq!(a, b, "content addressing is by plaintext digest, not key");
        // Each is its OWN blob, independently decryptable under its own
        // scope -- identical plaintext must NOT collapse to one shared blob
        // under the first writer's key (see
        // `cross_counterparty_dedup_does_not_defeat_erasure` for why that
        // would defeat erasure).
        assert_eq!(store.get_scoped(alice, &a).unwrap(), b"same text");
        assert_eq!(store.get_scoped(bob, &b).unwrap(), b"same text");
        assert_ne!(
            store.blob_path_for_test_scoped(alice, &a),
            store.blob_path_for_test_scoped(bob, &b),
            "identical plaintext must still live at two distinct on-disk paths"
        );
    }

    #[test]
    fn cross_counterparty_dedup_does_not_defeat_erasure() {
        // Two counterparties independently storing identical plaintext must
        // NOT share one blob under the first writer's key: erasing one
        // counterparty must leave the other's copy of the same plaintext
        // fully readable, and must make the erased counterparty's own copy
        // fully unrecoverable (not "still readable via the survivor's key").
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let alice = Ulid::new();
        let bob = Ulid::new();
        let shared = store.put_scoped(alice, b"see you at 3pm").unwrap();
        assert_eq!(store.put_scoped(bob, b"see you at 3pm").unwrap(), shared);

        // Erase alice only.
        assert!(store.erase_counterparty_key(alice).unwrap());

        // Alice's own copy is gone...
        assert!(matches!(
            store.get_scoped(alice, &shared),
            Err(ArtifactStoreError::Decrypt | ArtifactStoreError::KeyRing(_))
        ));
        // ...but bob's independently-keyed copy of the SAME plaintext
        // survives untouched.
        assert_eq!(store.get_scoped(bob, &shared).unwrap(), b"see you at 3pm");
    }

    #[test]
    fn erased_counterparty_payload_is_unrecoverable() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let counterparty = Ulid::new();
        let artifact_ref = store.put_scoped(counterparty, b"private message").unwrap();

        store.keys.erase(counterparty).unwrap();

        let result = store.get_scoped(counterparty, &artifact_ref);
        assert!(matches!(
            result,
            Err(ArtifactStoreError::Decrypt | ArtifactStoreError::KeyRing(_))
        ));
    }

    #[test]
    fn header_scope_survives_key_erasure() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let counterparty = Ulid::new();
        let artifact_ref = store.put_scoped(counterparty, b"private message").unwrap();
        let path = store.blob_path_for_test_scoped(counterparty, &artifact_ref);

        let read_header_scope = || {
            let blob = std::fs::read(&path).unwrap();
            let mut scope = [0u8; SCOPE_LEN];
            scope.copy_from_slice(&blob[1..1 + SCOPE_LEN]);
            Ulid::from_bytes(scope)
        };
        assert_eq!(read_header_scope(), counterparty);

        store.keys.erase(counterparty).unwrap();
        // The plaintext scope header survives on disk; only the decrypting
        // key is gone. (The scope is also encoded in the blob's own
        // subdirectory path -- this asserts the in-band header copy, kept
        // for D-012 provenance-without-key readability and
        // `get_scoped`'s defense-in-depth header/path cross-check, also
        // survives.)
        assert_eq!(read_header_scope(), counterparty);
    }

    #[test]
    fn scope_of_reads_embedded_header_without_a_key() {
        // `scope_of` is the design-mandated header-reading attribution
        // API: it reads the producing counterparty id from the blob header
        // WITHOUT touching any key material, so attribution survives even
        // after the counterparty's key is crypto-erased (D-012). It also
        // refuses to attribute when the header disagrees with the scope the
        // caller asked about (a foreign/corrupted file at the wrong path).
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let counterparty = Ulid::new();
        let other = Ulid::new();
        let artifact_ref = store
            .put_scoped(counterparty, b"attributed payload")
            .unwrap();

        // Attribution reads the embedded scope, no key needed.
        assert_eq!(
            store.scope_of(counterparty, &artifact_ref).unwrap(),
            counterparty
        );

        // A scope with no blob at that path is simply not found.
        assert!(matches!(
            store.scope_of(other, &artifact_ref),
            Err(ArtifactStoreError::Io { .. })
        ));

        // Header/path mismatch: stage the SAME bytes at `other`'s scoped
        // path (a foreign file landing in the wrong place). `scope_of`
        // must refuse to attribute rather than silently trust it.
        let alice_blob =
            std::fs::read(store.blob_path_for_test_scoped(counterparty, &artifact_ref)).unwrap();
        let mismatched_path = store.blob_path_for_test_scoped(other, &artifact_ref);
        std::fs::create_dir_all(mismatched_path.parent().unwrap()).unwrap();
        std::fs::write(&mismatched_path, &alice_blob).unwrap();
        assert!(matches!(
            store.scope_of(other, &artifact_ref),
            Err(ArtifactStoreError::UnknownFormat(_))
        ));

        // Even after the key is gone, the header attribution survives.
        store.keys.erase(counterparty).unwrap();
        assert_eq!(
            store.scope_of(counterparty, &artifact_ref).unwrap(),
            counterparty
        );
    }

    #[test]
    fn legacy_blobs_migrate_under_system_scope() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("artifacts");
        std::fs::create_dir_all(&root).unwrap();
        let legacy_key = [3u8; 32];
        let legacy_cipher = Aes256Gcm::new_from_slice(&legacy_key).unwrap();
        let plaintext = b"legacy payload";
        let digest = digest_of_bytes(plaintext);
        let hex = ArtifactStore::digest_hex(&digest).to_string();
        let mut nonce = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce);
        let nonce = Nonce::try_from(nonce.as_slice()).unwrap();
        let ciphertext = legacy_cipher.encrypt(&nonce, plaintext.as_slice()).unwrap();
        let mut blob = Vec::new();
        blob.extend_from_slice(nonce.as_slice());
        blob.extend_from_slice(&ciphertext);
        std::fs::write(root.join(&hex), &blob).unwrap();

        // Open a store with the SAME master key that happens to equal the
        // legacy key here — the migration path takes the legacy key
        // explicitly, decoupled from the ring's master key.
        let store = ArtifactStore::open(root.clone(), legacy_key).unwrap();
        let migrated = store.migrate_legacy_blobs(legacy_key).unwrap();
        assert_eq!(migrated, 1);

        let artifact_ref = ArtifactRef {
            digest,
            schema_version: 1,
        };
        // `get` (the SYSTEM_SCOPE wrapper) succeeding IS the proof the
        // migration wrote the re-keyed blob under SYSTEM_SCOPE.
        assert_eq!(store.get(&artifact_ref).unwrap(), plaintext);
        // The flat legacy source is gone.
        assert!(!root.join(&hex).exists());
        // Idempotent on re-run.
        assert_eq!(store.migrate_legacy_blobs(legacy_key).unwrap(), 0);
    }

    #[test]
    fn legacy_blob_with_tag_colliding_nonce_still_migrates() {
        // A pre-AD-140 blob is `[nonce:12][ciphertext]` with no format tag.
        // Detecting legacy purely by `first byte == FORMAT_TAG` is unsafe: a
        // legacy nonce whose first byte happens to equal the tag (1/256) would
        // be skipped as "already migrated" yet be undecryptable under the new
        // format -- silent data loss on upgrade. This test forces that
        // collision and asserts the blob is still migrated and readable.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("artifacts");
        std::fs::create_dir_all(&root).unwrap();
        let legacy_key = [3u8; 32];
        let legacy_cipher = Aes256Gcm::new_from_slice(&legacy_key).unwrap();
        let plaintext = b"legacy payload with colliding nonce";
        let digest = digest_of_bytes(plaintext);
        let hex = digest.as_str().strip_prefix("sha256:").unwrap();
        // Force the first nonce byte to equal FORMAT_TAG (2) so the naive
        // first-byte check would misclassify this as already-migrated.
        let mut nonce = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce);
        nonce[0] = FORMAT_TAG;
        let nonce = Nonce::try_from(nonce.as_slice()).unwrap();
        let ciphertext = legacy_cipher.encrypt(&nonce, plaintext.as_slice()).unwrap();
        let mut blob = Vec::new();
        blob.extend_from_slice(nonce.as_slice());
        blob.extend_from_slice(&ciphertext);
        assert_eq!(blob[0], FORMAT_TAG, "setup: colliding first byte");
        std::fs::write(root.join(hex), &blob).unwrap();

        let store = ArtifactStore::open(root.clone(), legacy_key).unwrap();
        let migrated = store.migrate_legacy_blobs(legacy_key).unwrap();
        assert_eq!(migrated, 1, "colliding legacy blob must be migrated");

        let artifact_ref = ArtifactRef {
            digest,
            schema_version: 1,
        };
        assert_eq!(store.get(&artifact_ref).unwrap(), plaintext);
        // Re-run is still idempotent (now new-format).
        assert_eq!(store.migrate_legacy_blobs(legacy_key).unwrap(), 0);
    }

    #[test]
    fn legacy_migration_recovers_from_crash_between_write_and_cleanup() {
        // Simulate a crash that landed the migration target (SYSTEM_SCOPE
        // subdir) but was interrupted before the flat legacy source could be
        // removed: both paths exist going into this run. Migration must
        // clean up the stale flat source (verifying the target first, never
        // trusting existence alone) without erroring, re-decrypting, or
        // double-counting -- and the content must remain correctly readable
        // throughout.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("artifacts");
        std::fs::create_dir_all(&root).unwrap();
        let legacy_key = [3u8; 32];
        let legacy_cipher = Aes256Gcm::new_from_slice(&legacy_key).unwrap();
        let plaintext = b"payload mid-migration-crash";
        let digest = digest_of_bytes(plaintext);
        let hex = digest.as_str().strip_prefix("sha256:").unwrap().to_string();

        // The (untouched) flat legacy source.
        let mut legacy_nonce = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut legacy_nonce);
        let legacy_nonce = Nonce::try_from(legacy_nonce.as_slice()).unwrap();
        let legacy_ciphertext = legacy_cipher
            .encrypt(&legacy_nonce, plaintext.as_slice())
            .unwrap();
        let mut legacy_blob = Vec::new();
        legacy_blob.extend_from_slice(legacy_nonce.as_slice());
        legacy_blob.extend_from_slice(&legacy_ciphertext);
        std::fs::write(root.join(&hex), &legacy_blob).unwrap();

        let store = ArtifactStore::open(root.clone(), legacy_key).unwrap();

        // Pre-populate the migration TARGET directly, as if a prior run's
        // rename had already succeeded before crashing.
        let target_ref = ArtifactRef {
            digest: digest.clone(),
            schema_version: 1,
        };
        let target_path = store.blob_path_for_test(&target_ref);
        std::fs::create_dir_all(target_path.parent().unwrap()).unwrap();
        let new_key = store.keys.get_or_create_key(SYSTEM_SCOPE).unwrap();
        let new_cipher = Aes256Gcm::new_from_slice(&new_key).unwrap();
        let mut new_nonce = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut new_nonce);
        let new_nonce = Nonce::try_from(new_nonce.as_slice()).unwrap();
        let new_ciphertext = new_cipher
            .encrypt(&new_nonce, plaintext.as_slice())
            .unwrap();
        let mut new_blob = Vec::new();
        new_blob.push(RECOVERED_FORMAT);
        new_blob.extend_from_slice(&SYSTEM_SCOPE.to_bytes());
        new_blob.extend_from_slice(new_nonce.as_slice());
        new_blob.extend_from_slice(&new_ciphertext);
        std::fs::write(&target_path, &new_blob).unwrap();

        assert!(root.join(&hex).exists(), "flat legacy source still present");
        assert!(
            target_path.exists(),
            "target already written (simulated crash)"
        );

        // Recovery: no error, stale flat source removed, target still reads
        // correctly, and this is NOT counted as a fresh migration (nothing
        // new was decrypted/migrated -- it was already done).
        let migrated = store.migrate_legacy_blobs(legacy_key).unwrap();
        assert_eq!(
            migrated, 0,
            "crash-recovery cleanup is not a fresh migration"
        );
        assert!(
            !root.join(&hex).exists(),
            "stale flat legacy source must be cleaned up"
        );
        assert_eq!(store.get(&target_ref).unwrap(), plaintext);

        // Fully idempotent from here on.
        assert_eq!(store.migrate_legacy_blobs(legacy_key).unwrap(), 0);
    }

    #[test]
    fn corrupt_scoped_target_with_valid_flat_source_fails_migration() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("artifacts");
        std::fs::create_dir_all(&root).unwrap();
        let legacy_key = [3u8; 32];
        let legacy_cipher = Aes256Gcm::new_from_slice(&legacy_key).unwrap();
        let plaintext = b"valid flat source beside corrupt target";
        let digest = digest_of_bytes(plaintext);
        let hex = ArtifactStore::digest_hex(&digest).to_string();

        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::try_from(nonce_bytes.as_slice()).unwrap();
        let ciphertext = legacy_cipher.encrypt(&nonce, plaintext.as_slice()).unwrap();
        let mut legacy_blob = Vec::new();
        legacy_blob.extend_from_slice(&nonce_bytes);
        legacy_blob.extend_from_slice(&ciphertext);
        let flat_path = root.join(&hex);
        std::fs::write(&flat_path, legacy_blob).unwrap();

        let store = ArtifactStore::open(root, legacy_key).unwrap();
        let artifact_ref = ArtifactRef {
            digest,
            schema_version: 1,
        };
        let target_path = store.blob_path_for_test(&artifact_ref);
        std::fs::create_dir_all(target_path.parent().unwrap()).unwrap();
        std::fs::write(&target_path, [CURRENT_FORMAT]).unwrap();

        assert!(matches!(
            store.migrate_legacy_blobs(legacy_key),
            Err(ArtifactStoreError::Truncated(path)) if path == target_path
        ));
        assert!(
            flat_path.exists(),
            "verification failure must preserve the valid flat source"
        );
    }

    #[test]
    fn legacy_key_supports_scoped_put_and_get_without_relocking() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("artifacts");
        let master_key = key();
        let store = ArtifactStore::open(root, master_key).unwrap();
        let scope = Ulid::new();
        let raw_key = [11u8; 32];
        let key_path = store.keys.key_path_for_test(scope);

        let write_legacy_key = || {
            let master_cipher = Aes256Gcm::new_from_slice(&master_key).unwrap();
            let mut nonce_bytes = [0u8; NONCE_LEN];
            rand::rng().fill_bytes(&mut nonce_bytes);
            let nonce = Nonce::try_from(nonce_bytes.as_slice()).unwrap();
            let ciphertext = master_cipher
                .encrypt(
                    &nonce,
                    Payload {
                        msg: raw_key.as_slice(),
                        aad: &[],
                    },
                )
                .unwrap();
            let mut legacy_bytes = Vec::new();
            legacy_bytes.extend_from_slice(&nonce_bytes);
            legacy_bytes.extend_from_slice(&ciphertext);
            std::fs::write(&key_path, legacy_bytes).unwrap();
        };

        write_legacy_key();
        let artifact_ref = store
            .put_scoped(scope, b"payload using a legacy wrapped key")
            .unwrap();
        assert!(std::fs::read(&key_path).unwrap().starts_with(b"OSK1"));

        write_legacy_key();
        assert_eq!(
            store.get_scoped(scope, &artifact_ref).unwrap(),
            b"payload using a legacy wrapped key"
        );
        assert!(std::fs::read(&key_path).unwrap().starts_with(b"OSK1"));
    }

    #[test]
    fn tampered_blob_fails_digest_check() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let artifact_ref = store.put(b"original content").unwrap();

        // Overwrite the same content-addressed path with a fresh blob that
        // decrypts cleanly (valid AEAD tag) but to bytes that do not hash to
        // the ref -- a content-substitution tamper (D-055.4 class). The store
        // must fail closed on the digest check, never return the wrong bytes.
        store
            .put_tampered_for_test(&artifact_ref.digest, b"substituted content")
            .unwrap();

        let result = store.get(&artifact_ref);
        assert!(matches!(result, Err(ArtifactStoreError::DigestMismatch)));
    }

    #[test]
    fn truncated_blob_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let artifact_ref = store.put(b"some content").unwrap();

        // Corrupt the stored blob down to just the format tag byte (disk
        // corruption clipping the file). Must fail closed, not panic.
        std::fs::write(store.blob_path_for_test(&artifact_ref), [FORMAT_TAG]).unwrap();

        let result = store.get(&artifact_ref);
        assert!(matches!(result, Err(ArtifactStoreError::Truncated(_))));
    }

    #[test]
    fn bit_flipped_ciphertext_fails_aead() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let artifact_ref = store.put(b"integrity-sensitive payload").unwrap();
        let path = store.blob_path_for_test(&artifact_ref);

        // Flip the final ciphertext byte on disk -- a genuine AEAD tamper. The
        // authentication tag must reject it (Decrypt), never yield plaintext.
        let mut bytes = std::fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        std::fs::write(&path, &bytes).unwrap();

        let result = store.get(&artifact_ref);
        assert!(matches!(result, Err(ArtifactStoreError::Decrypt)));
    }
    #[test]
    fn put_scoped_after_erase_rejects_forever() {
        // A crypto-erased scope is PERMANENTLY closed (AD-140): the
        // tombstone marker + deleted key must reject every subsequent
        // `put_scoped` for that counterparty, so a fresh `Ok(ref)` can
        // never be returned that points at a blob no key can decrypt. A
        // "resurrect the key and re-encrypt" behavior would silently undo
        // the erasure — a correctness and privacy regression.
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let counterparty = Ulid::new();
        let artifact_ref = store.put_scoped(counterparty, b"private message").unwrap();
        assert_eq!(
            store.get_scoped(counterparty, &artifact_ref).unwrap(),
            b"private message"
        );

        // Erase (the durable, unconditional path the production erasure
        // flow uses): tombstone marker written, key file deleted.
        assert!(store.erase_counterparty_key(counterparty).unwrap());
        assert!(matches!(
            store.get_scoped(counterparty, &artifact_ref),
            Err(ArtifactStoreError::Decrypt | ArtifactStoreError::KeyRing(_))
        ));
        // (The erase wrote a permanent tombstone marker and deleted the key
        // file; the rejection below — plus the still-unreadable blob — is
        // the behavioral proof that the scope is permanently closed.)

        // Re-store the SAME plaintext: the scope is closed, so this MUST
        // fail rather than hand back a decryptable ref (no resurrection).
        let re = store.put_scoped(counterparty, b"private message");
        assert!(
            matches!(
                re,
                Err(ArtifactStoreError::KeyRing(CounterpartyKeyError::Erased(_)))
            ),
            "erased scope must reject put_scoped; got {re:?}"
        );
        // And the rejection must not have brought the key back: the blob
        // is still unreadable.
        assert!(matches!(
            store.get_scoped(counterparty, &artifact_ref),
            Err(ArtifactStoreError::Decrypt | ArtifactStoreError::KeyRing(_))
        ));
    }
    #[test]
    fn existing_blob_retry_durability_resyncs() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let counterparty = Ulid::new();
        let payload = b"retry durable payload";
        let artifact_ref = store.put_scoped(counterparty, payload).unwrap();

        store.set_fault_existing_blob_sync_for_test(true);
        let retry = store.put_scoped(counterparty, payload);
        assert!(matches!(retry, Err(ArtifactStoreError::Io { .. })));

        store.set_fault_existing_blob_sync_for_test(false);
        let ok_retry = store.put_scoped(counterparty, payload).unwrap();
        assert_eq!(ok_retry, artifact_ref);
    }

    #[test]
    fn format2_recovered_blob_reads_and_migrates_to_format3() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let counterparty = Ulid::new();
        let plaintext = b"legacy format 2 payload";
        let digest = digest_of_bytes(plaintext);
        let artifact_ref = ArtifactRef {
            digest,
            schema_version: 1,
        };

        let path = store.blob_path_for_test_scoped(counterparty, &artifact_ref);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        let key = store.keys.get_or_create_key(counterparty).unwrap();
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let mut nonce = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce);
        let cipher_nonce = Nonce::try_from(nonce.as_slice()).unwrap();
        let ciphertext = cipher.encrypt(&cipher_nonce, plaintext.as_slice()).unwrap();

        let mut raw = Vec::new();
        raw.push(RECOVERED_FORMAT);
        raw.extend_from_slice(&counterparty.to_bytes());
        raw.extend_from_slice(&nonce);
        raw.extend_from_slice(&ciphertext);
        std::fs::write(&path, &raw).unwrap();

        let read_back = store.get_scoped(counterparty, &artifact_ref).unwrap();
        assert_eq!(read_back, plaintext);

        let migrated_disk = std::fs::read(&path).unwrap();
        assert_eq!(migrated_disk[0], CURRENT_FORMAT);
    }

    #[test]
    fn format2_post_rename_sync_failure_recovers_once_without_syncing_steady_state_reads() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let counterparty = Ulid::new();
        let plaintext = b"format 2 retry payload";
        let artifact_ref = ArtifactRef {
            digest: digest_of_bytes(plaintext),
            schema_version: 1,
        };
        let path = store.blob_path_for_test_scoped(counterparty, &artifact_ref);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        let key = store.keys.get_or_create_key(counterparty).unwrap();
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let mut nonce = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce);
        let cipher_nonce = Nonce::try_from(nonce.as_slice()).unwrap();
        let ciphertext = cipher.encrypt(&cipher_nonce, plaintext.as_slice()).unwrap();
        let mut raw = Vec::new();
        raw.push(RECOVERED_FORMAT);
        raw.extend_from_slice(&counterparty.to_bytes());
        raw.extend_from_slice(&nonce);
        raw.extend_from_slice(&ciphertext);
        std::fs::write(&path, raw).unwrap();

        store.set_fault_existing_blob_sync_for_test(true);
        assert!(matches!(
            store.get_scoped(counterparty, &artifact_ref),
            Err(ArtifactStoreError::Io { .. })
        ));
        assert_eq!(std::fs::read(&path).unwrap()[0], CURRENT_FORMAT);
        let marker_path = ArtifactStore::upgrade_pending_path(&path);
        assert!(marker_path.exists());

        store.set_fault_existing_blob_sync_for_test(false);
        assert_eq!(
            store.get_scoped(counterparty, &artifact_ref).unwrap(),
            plaintext
        );
        assert!(!marker_path.exists());

        store.set_fault_existing_blob_sync_for_test(true);
        assert_eq!(
            store.get_scoped(counterparty, &artifact_ref).unwrap(),
            plaintext
        );
    }

    #[test]
    fn clear_upgrade_pending_sync_failure_retains_marker_for_retry() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let counterparty = Ulid::new();
        let plaintext = b"clear marker durability payload";
        let artifact_ref = ArtifactRef {
            digest: digest_of_bytes(plaintext),
            schema_version: 1,
        };
        let path = store.blob_path_for_test_scoped(counterparty, &artifact_ref);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        let key = store.keys.get_or_create_key(counterparty).unwrap();
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let mut nonce = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce);
        let cipher_nonce = Nonce::try_from(nonce.as_slice()).unwrap();
        let ciphertext = cipher.encrypt(&cipher_nonce, plaintext.as_slice()).unwrap();
        let mut raw = Vec::new();
        raw.push(RECOVERED_FORMAT);
        raw.extend_from_slice(&counterparty.to_bytes());
        raw.extend_from_slice(&nonce);
        raw.extend_from_slice(&ciphertext);
        std::fs::write(&path, raw).unwrap();

        // First upgrade write succeeds fully; force only marker-clear
        // durability to fail so the blob is format 3 with a retained marker.
        store.set_fault_clear_upgrade_pending_sync_for_test(true);
        assert!(matches!(
            store.get_scoped(counterparty, &artifact_ref),
            Err(ArtifactStoreError::Io { .. })
        ));
        assert_eq!(std::fs::read(&path).unwrap()[0], CURRENT_FORMAT);
        let marker_path = ArtifactStore::upgrade_pending_path(&path);
        assert!(
            marker_path.exists(),
            "post-unlink clear durability failure must retain/recreate the marker"
        );

        store.set_fault_clear_upgrade_pending_sync_for_test(false);
        assert_eq!(
            store.get_scoped(counterparty, &artifact_ref).unwrap(),
            plaintext
        );
        assert!(!marker_path.exists());
    }

    #[test]
    fn concurrent_marker_recovery_and_missing_marker_clear_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let store =
            std::sync::Arc::new(ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap());
        let scope = Ulid::new();
        let plaintext = b"concurrent marker recovery payload";
        let artifact_ref = store.put_scoped(scope, plaintext).unwrap();
        let path = store.blob_path_for_test_scoped(scope, &artifact_ref);
        let marker_path = ArtifactStore::upgrade_pending_path(&path);
        store.persist_upgrade_pending(&path).unwrap();

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let scope_store = std::sync::Arc::clone(&store);
        let scope_barrier = std::sync::Arc::clone(&barrier);
        let scope_ref = artifact_ref.clone();
        let scope_reader = std::thread::spawn(move || {
            scope_barrier.wait();
            scope_store.scope_of(scope, &scope_ref)
        });

        let get_store = std::sync::Arc::clone(&store);
        let get_barrier = std::sync::Arc::clone(&barrier);
        let get_ref = artifact_ref.clone();
        let plaintext_reader = std::thread::spawn(move || {
            get_barrier.wait();
            get_store.get_scoped(scope, &get_ref)
        });

        barrier.wait();
        assert_eq!(scope_reader.join().unwrap().unwrap(), scope);
        assert_eq!(plaintext_reader.join().unwrap().unwrap(), plaintext);
        assert!(!marker_path.exists());

        // A racing recovery process can observe the marker before another
        // process unlinks it. Missing-marker clears remain successful and
        // still execute the directory-sync path.
        store.clear_upgrade_pending(&path).unwrap();
        store.clear_upgrade_pending(&path).unwrap();
    }

    #[test]
    fn aead_associated_data_scope_tamper_fails_decrypt() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::open(dir.path().join("artifacts"), key()).unwrap();
        let alice = Ulid::new();
        let bob = Ulid::new();
        let artifact_ref = store.put_scoped(alice, b"alice secret").unwrap();

        let alice_path = store.blob_path_for_test_scoped(alice, &artifact_ref);
        let bob_path = store.blob_path_for_test_scoped(bob, &artifact_ref);
        std::fs::create_dir_all(bob_path.parent().unwrap()).unwrap();

        let mut tampered = std::fs::read(&alice_path).unwrap();
        tampered[1..17].copy_from_slice(&bob.to_bytes());
        std::fs::write(&bob_path, &tampered).unwrap();

        let err = store.get_scoped(bob, &artifact_ref);
        assert!(matches!(
            err,
            Err(ArtifactStoreError::Decrypt | ArtifactStoreError::UnknownFormat(_))
        ));
    }
}

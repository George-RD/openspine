//!
//! Per-counterparty payload key ring (AD-140, resolves OQ-7).
//!
//! Each counterparty (an `identity_id`, including the owner's own identity —
//! see `openspine_schemas::briefcase::CounterpartyRef`) gets its own random
//! 256-bit AES-GCM payload key. Keys are stored one-file-per-counterparty
//! under `<data_dir>/keys/<ulid>`, each wrapped (encrypted) at rest under the
//! kernel's master key — the same key previously used to encrypt artifact
//! content directly (see `crate::artifact_store`). The master key now only
//! ever wraps/unwraps 32-byte key material; it never touches payload content.
//!
//! **Crypto-erase (AD-140) is permanent.** `erase` (1) writes a marker-only
//! tombstone file `<ulid>.erased` (NO key material — just an empty marker),
//! fsync'd, so the scope is PERMANENTLY closed to future key creation, then
//! (2) physically unlinks the wrapped-key file, fsync'd — the substantive
//! erasure act matching the spec requirement "the store MUST fail to decrypt
//! it": once this returns, the wrapped key bytes are actually gone from
//! disk, not merely shadowed by application logic. From that point on,
//! `get_key` returns `Ok(None)` (the key file is gone) and
//! `get_or_create_key` permanently refuses with `Erased` (the tombstone is
//! present) — no plaintext payload key is ever cached in memory across
//! calls, so there is no lingering hot key to invalidate, and the scope can
//! never be "resurrected" by a later write re-minting a fresh key for the
//! same id.
//!
//! **In-process concurrency.** `with_scope_lock` provides a per-counterparty
//! `Mutex` that `get_or_create_key`/`erase` (and, externally,
//! `ArtifactStore::put_scoped` / `Store::mark_learned_artifacts_erased`) use
//! to make their multi-step read-then-act sequences atomic w.r.t. each
//! other for the SAME scope: without it, a write could complete holding a
//! key that a concurrently-racing erase deletes moments later, silently
//! orphaning that write (a "dead on arrival" `ArtifactRef`), or two
//! concurrent first-use creates could race. This is a single-PROCESS
//! guarantee only (a `std::sync::Mutex` inside one `CounterpartyKeyRing`
//! instance) — it does NOT provide cross-process exclusion for multiple
//! kernel processes sharing one data directory (that would need a
//! filesystem-level lock/lease; a documented limitation, not addressed
//! here since this codebase's assumption throughout is a single kernel
//! process per data directory).
//!
//! AD-139 treats "SQLite DB + artifact blobs + keys" as three independent
//! elements of one backup/restore snapshot set; keeping the key ring as its
//! own directory (rather than a new SQL table) matches that split directly
//! and keeps this change out of the `store/migrations.rs` schema lane.

use std::collections::{HashMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use rand::Rng;
use ulid::Ulid;

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

const V1_HEADER: &[u8] = b"OSK1";
const KEY_WRAP_DOMAIN_V1: &[u8] = b"openspine.counterparty_key.v1";

/// Reserved scope for payloads not attributable to any specific external
/// counterparty (owner-authored content, system/internal artifacts, and
/// every pre-AD-140 blob migrated from the old single global key). It is an
/// ordinary scope as far as key access is concerned, but MUST NOT be erasable.
pub const SYSTEM_SCOPE: Ulid = Ulid::nil();

#[derive(Debug, thiserror::Error)]
pub enum CounterpartyKeyError {
    #[error("counterparty key store I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to wrap counterparty payload key")]
    Wrap,
    #[error("failed to decrypt counterparty payload key (wrong master key or corrupted key file)")]
    Decrypt,
    #[error("stored key file at {0} is shorter than the minimum prefix length")]
    Truncated(PathBuf),
    #[error("counterparty {0} was crypto-erased; its scope is permanently closed to new keys")]
    Erased(Ulid),
    #[error("cannot erase reserved system scope {0}")]
    ReservedScope(Ulid),
}

/// The per-counterparty payload key ring. `open` is cheap and idempotent —
/// it does not read or create any counterparty's key, only the containing
/// directory.
pub struct CounterpartyKeyRing {
    dir: PathBuf,
    master_cipher: Aes256Gcm,
    /// Per-counterparty in-process locks (lazily created, never removed —
    /// bounded by the number of DISTINCT counterparty ids ever touched in
    /// this process's lifetime, an acceptable small map for this workload).
    scope_locks: Mutex<HashMap<Ulid, Arc<Mutex<()>>>>,
    /// Scopes closed after a committed erasure in this process. This is
    /// checked while the per-scope lock is held, before every key lookup or
    /// creation, so a filesystem cleanup failure cannot reopen plaintext
    /// access in the same process.
    closed_scopes: Mutex<HashSet<Ulid>>,
    #[cfg(test)]
    pub(crate) fsync_count: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    fail_fsync_at: std::sync::atomic::AtomicUsize,
}

/// RAII guard ensuring temporary files created during publication or migration
/// are unconditionally deleted if dropped before explicit cleanup/publication.
struct TempFileGuard(Option<PathBuf>);

impl TempFileGuard {
    fn new(path: PathBuf) -> Self {
        Self(Some(path))
    }

    fn remove(&mut self) -> Result<(), std::io::Error> {
        if let Some(path) = self.0.take() {
            match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err),
            }
        } else {
            Ok(())
        }
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn make_v1_aad(counterparty_id: Ulid) -> Vec<u8> {
    let mut aad = Vec::with_capacity(KEY_WRAP_DOMAIN_V1.len() + 16);
    aad.extend_from_slice(KEY_WRAP_DOMAIN_V1);
    aad.extend_from_slice(&counterparty_id.to_bytes());
    aad
}
mod recovery;

mod format;

impl CounterpartyKeyRing {
    pub fn open(dir: PathBuf, master_key: [u8; KEY_LEN]) -> Result<Self, CounterpartyKeyError> {
        std::fs::create_dir_all(&dir).map_err(|source| CounterpartyKeyError::Io {
            path: dir.clone(),
            source,
        })?;
        let master_cipher =
            Aes256Gcm::new_from_slice(&master_key).expect("key is exactly 32 bytes");
        let ring = Self {
            dir,
            master_cipher,
            scope_locks: Mutex::new(HashMap::new()),
            closed_scopes: Mutex::new(HashSet::new()),
            #[cfg(test)]
            fsync_count: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            fail_fsync_at: std::sync::atomic::AtomicUsize::new(usize::MAX),
        };
        // Re-sync both entries on every open. This also repairs a prior open
        // that created the directory but failed while syncing its parent.
        ring.fsync_dir(&ring.dir)?;
        let parent = ring
            .dir
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        ring.fsync_dir(parent)?;
        ring.recover_pending_erasures()?;
        Ok(ring)
    }

    fn fsync_dir(&self, dir: &Path) -> Result<(), CounterpartyKeyError> {
        #[cfg(test)]
        {
            let call = self
                .fsync_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                + 1;
            if self
                .fail_fsync_at
                .compare_exchange(
                    call,
                    usize::MAX,
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                )
                .is_ok()
            {
                return Err(CounterpartyKeyError::Io {
                    path: dir.to_path_buf(),
                    source: std::io::Error::other("injected directory fsync failure"),
                });
            }
        }
        let f = std::fs::File::open(dir).map_err(|source| CounterpartyKeyError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        f.sync_all().map_err(|source| CounterpartyKeyError::Io {
            path: dir.to_path_buf(),
            source,
        })
    }

    fn sync_key_file_and_dir(&self, key_path: &Path) -> Result<(), CounterpartyKeyError> {
        let f = std::fs::File::open(key_path).map_err(|source| CounterpartyKeyError::Io {
            path: key_path.to_path_buf(),
            source,
        })?;
        f.sync_all().map_err(|source| CounterpartyKeyError::Io {
            path: key_path.to_path_buf(),
            source,
        })?;
        self.fsync_dir(&self.dir)
    }

    fn key_path(&self, counterparty_id: Ulid) -> PathBuf {
        self.dir.join(counterparty_id.to_string())
    }

    fn tombstone_path(&self, counterparty_id: Ulid) -> PathBuf {
        self.dir.join(format!("{counterparty_id}.erased"))
    }

    #[cfg(test)]
    pub(crate) fn key_path_for_test(&self, counterparty_id: Ulid) -> PathBuf {
        self.key_path(counterparty_id)
    }
    #[cfg(test)]
    fn fail_fsync_dir_on_call(&self, call: usize) {
        self.fail_fsync_at
            .store(call, std::sync::atomic::Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn key_pending_path_for_test(&self, counterparty_id: Ulid) -> PathBuf {
        self.key_pending_path(counterparty_id)
    }

    pub(super) fn require_regular_file_or_absent(
        &self,
        path: &Path,
    ) -> Result<bool, CounterpartyKeyError> {
        let _ = self;
        match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_file() => Ok(true),
            Ok(_) => Err(CounterpartyKeyError::Io {
                path: path.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "counterparty key/tombstone path must be a regular file",
                ),
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(CounterpartyKeyError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    pub(crate) fn with_scope_lock<T, E>(
        &self,
        counterparty_id: Ulid,
        f: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let scope_mutex = {
            let mut locks = self
                .scope_locks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            locks
                .entry(counterparty_id)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = scope_mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f()
    }

    fn scope_is_closed(&self, counterparty_id: Ulid) -> bool {
        self.closed_scopes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&counterparty_id)
    }

    pub(crate) fn close_scope_in_memory(&self, counterparty_id: Ulid) {
        self.closed_scopes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(counterparty_id);
    }

    pub(crate) fn get_or_create_key_locked(
        &self,
        counterparty_id: Ulid,
    ) -> Result<[u8; KEY_LEN], CounterpartyKeyError> {
        if self.scope_is_closed(counterparty_id) {
            return Err(CounterpartyKeyError::Erased(counterparty_id));
        }
        if self.require_regular_file_or_absent(&self.tombstone_path(counterparty_id))? {
            return Err(CounterpartyKeyError::Erased(counterparty_id));
        }
        let path = self.key_path(counterparty_id);
        if self.require_regular_file_or_absent(&path)? {
            let (key, is_legacy) = self.unwrap_file(&path, counterparty_id)?;
            if is_legacy {
                self.migrate_legacy_key_to_v1_locked(&path, counterparty_id, &key)?;
            } else {
                // Existing-key path with a pending marker must re-sync key+dir
                // and only then clear the marker before returning success.
                self.recover_pending_key_locked(&path, counterparty_id)?;
            }
            return Ok(key);
        }

        let mut key = [0u8; KEY_LEN];
        rand::rng().fill_bytes(&mut key);

        let wrapped = self.wrap_v1_key(counterparty_id, &key)?;

        // Marker first: if we crash after hard-link publication but before the
        // key+dir durability work finishes, a later create/get retry still
        // sees durable pending evidence and re-runs the recovery path.
        self.mark_key_pending_locked(counterparty_id)?;

        let tmp_path = path.with_extension(format!("tmp.{}", Ulid::new()));
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
            // Publication never became visible; drop the pending marker so we
            // do not force a key sync for a never-published key.
            let _ = self.clear_key_pending_locked(counterparty_id);
            return Err(CounterpartyKeyError::Io {
                path: tmp_path,
                source,
            });
        }

        match std::fs::hard_link(&tmp_path, &path) {
            Ok(()) => {
                guard.remove().map_err(|source| CounterpartyKeyError::Io {
                    path: tmp_path.clone(),
                    source,
                })?;
                // Keep the marker until key+dir durability succeeds so a
                // post-hardlink fsync failure leaves a retry signal.
                self.sync_key_file_and_dir(&path)?;
                self.clear_key_pending_locked(counterparty_id)?;
                Ok(key)
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                guard.remove().map_err(|source| CounterpartyKeyError::Io {
                    path: tmp_path.clone(),
                    source,
                })?;
                // Another publisher won. Complete any outstanding durability for
                // the already-visible key (including our own pending marker if
                // we planted one before losing the race) before returning.
                let (key, is_legacy) = self.unwrap_file(&path, counterparty_id)?;
                if is_legacy {
                    self.migrate_legacy_key_to_v1_locked(&path, counterparty_id, &key)?;
                } else {
                    self.recover_pending_key_locked(&path, counterparty_id)?;
                }
                Ok(key)
            }
            Err(source) => {
                if let Err(cleanup_err) = guard.remove() {
                    return Err(CounterpartyKeyError::Io {
                        path: tmp_path,
                        source: cleanup_err,
                    });
                }
                let _ = self.clear_key_pending_locked(counterparty_id);
                Err(CounterpartyKeyError::Io { path, source })
            }
        }
    }

    pub fn get_or_create_key(
        &self,
        counterparty_id: Ulid,
    ) -> Result<[u8; KEY_LEN], CounterpartyKeyError> {
        self.with_scope_lock(counterparty_id, || {
            self.get_or_create_key_locked(counterparty_id)
        })
    }

    pub(crate) fn erase_locked(&self, counterparty_id: Ulid) -> Result<bool, CounterpartyKeyError> {
        if counterparty_id == SYSTEM_SCOPE {
            return Err(CounterpartyKeyError::ReservedScope(counterparty_id));
        }

        let tombstone_path = self.tombstone_path(counterparty_id);

        // Tombstones must be regular files. A directory/symlink at the
        // `.erased` path is not a valid tombstone and must fail closed.
        if !self.require_regular_file_or_absent(&tombstone_path)? {
            let f = std::fs::File::create(&tombstone_path).map_err(|source| {
                CounterpartyKeyError::Io {
                    path: tombstone_path.clone(),
                    source,
                }
            })?;
            f.sync_all().map_err(|source| CounterpartyKeyError::Io {
                path: tombstone_path.clone(),
                source,
            })?;
        }

        let key_path = self.key_path(counterparty_id);
        let had_key = match std::fs::remove_file(&key_path) {
            Ok(()) => true,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
            Err(source) => {
                return Err(CounterpartyKeyError::Io {
                    path: key_path,
                    source,
                });
            }
        };

        // Sweep and remove any temp alias/migration files for this counterparty_id
        let temp_prefix = format!("{counterparty_id}.tmp.");
        let entries = std::fs::read_dir(&self.dir).map_err(|source| CounterpartyKeyError::Io {
            path: self.dir.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| CounterpartyKeyError::Io {
                path: self.dir.clone(),
                source,
            })?;
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with(&temp_prefix) {
                    if let Err(source) = std::fs::remove_file(entry.path()) {
                        if source.kind() != std::io::ErrorKind::NotFound {
                            return Err(CounterpartyKeyError::Io {
                                path: entry.path(),
                                source,
                            });
                        }
                    }
                }
            }
        }

        // Always fsync directory to guarantee retry sync durability for tombstones and key deletions
        self.fsync_dir(&self.dir)?;

        Ok(had_key)
    }

    pub fn erase(&self, counterparty_id: Ulid) -> Result<bool, CounterpartyKeyError> {
        if counterparty_id == SYSTEM_SCOPE {
            return Err(CounterpartyKeyError::ReservedScope(counterparty_id));
        }
        self.with_scope_lock(counterparty_id, || self.erase_locked(counterparty_id))
    }
}

#[cfg(test)]
#[path = "counterparty_keys_tests.rs"]
mod tests;

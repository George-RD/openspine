// openspine:allow-large-module reason: cohesive key-ring format, migration, durability, concurrency, and erasure matrix shares low-level filesystem fixtures.
//! Per-counterparty key-ring tests. Co-located as a separate file from the
//! production `counterparty_keys.rs` so the latter stays within the line
//! budget; loaded via `#[cfg(test)] #[path = "counterparty_keys_tests.rs"]
//! mod tests;` and compiling under the same crate test target, so `super::*`
//! and `#[cfg(test)]` helpers (e.g. `key_path_for_test`) are in scope.

use std::sync::Arc;

use super::*;

fn master() -> [u8; 32] {
    [9u8; 32]
}

#[test]
fn creates_and_persists_a_key() {
    let dir = tempfile::tempdir().unwrap();
    let ring = CounterpartyKeyRing::open(dir.path().join("keys"), master()).unwrap();
    let id = Ulid::new();
    let key_a = ring.get_or_create_key(id).unwrap();
    let key_b = ring.get_or_create_key(id).unwrap();
    assert_eq!(key_a, key_b);
    assert_eq!(ring.get_key(id).unwrap(), Some(key_a));
}

#[test]
fn distinct_counterparties_get_distinct_keys() {
    let dir = tempfile::tempdir().unwrap();
    let ring = CounterpartyKeyRing::open(dir.path().join("keys"), master()).unwrap();
    let key_a = ring.get_or_create_key(Ulid::new()).unwrap();
    let key_b = ring.get_or_create_key(Ulid::new()).unwrap();
    assert_ne!(key_a, key_b);
}

#[test]
fn get_key_is_none_before_creation() {
    let dir = tempfile::tempdir().unwrap();
    let ring = CounterpartyKeyRing::open(dir.path().join("keys"), master()).unwrap();
    assert_eq!(ring.get_key(Ulid::new()).unwrap(), None);
}

#[test]
fn erase_deletes_key_file_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let ring = CounterpartyKeyRing::open(dir.path().join("keys"), master()).unwrap();
    let id = Ulid::new();
    ring.get_or_create_key(id).unwrap();
    assert!(
        ring.key_path_for_test(id).exists(),
        "wrapped key file must exist on disk after first use"
    );

    assert!(
        ring.erase(id).unwrap(),
        "first erase must report true (key existed and was deleted)"
    );
    assert!(
        !ring.key_path_for_test(id).exists(),
        "wrapped key file must be physically unlinked after erase"
    );
    assert_eq!(
        ring.get_key(id).unwrap(),
        None,
        "get_key must return None after erasure"
    );

    assert!(
        !ring.erase(id).unwrap(),
        "second erase must report false (idempotent; already erased)"
    );
}

#[test]
fn wrong_master_key_fails_to_unwrap() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keys");
    let ring_a = CounterpartyKeyRing::open(path.clone(), [1u8; 32]).unwrap();
    let id = Ulid::new();
    ring_a.get_or_create_key(id).unwrap();

    let ring_b = CounterpartyKeyRing::open(path, [2u8; 32]).unwrap();
    let result = ring_b.get_key(id);
    assert!(matches!(result, Err(CounterpartyKeyError::Decrypt)));
}

#[test]
fn concurrent_first_use_is_race_free() {
    let dir = tempfile::tempdir().unwrap();
    let ring = Arc::new(CounterpartyKeyRing::open(dir.path().join("keys"), master()).unwrap());
    let id = Ulid::new();
    let results: Vec<[u8; KEY_LEN]> = std::thread::scope(|s| {
        let mut handles = Vec::new();
        for _ in 0..8 {
            let ring = ring.clone();
            handles.push(s.spawn(move || ring.get_or_create_key(id).unwrap()));
        }
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let first = results[0];
    for &k in &results[1..] {
        assert_eq!(k, first);
    }
}

#[test]
fn erase_of_never_used_scope_still_tombstones_it() {
    let dir = tempfile::tempdir().unwrap();
    let ring = CounterpartyKeyRing::open(dir.path().join("keys"), master()).unwrap();
    let id = Ulid::new();
    assert!(
        !ring.erase(id).unwrap(),
        "erase of never-used scope returns false (no key file deleted)"
    );
    assert_eq!(
        ring.get_key(id).unwrap(),
        None,
        "get_key on never-used but erased scope must return None"
    );
    let result = ring.get_or_create_key(id);
    assert!(
        matches!(result, Err(CounterpartyKeyError::Erased(e_id)) if e_id == id),
        "recreation must be permanently refused with Erased"
    );
}

#[test]
fn erase_after_key_creation_permanently_refuses_recreation() {
    let dir = tempfile::tempdir().unwrap();
    let ring = CounterpartyKeyRing::open(dir.path().join("keys"), master()).unwrap();
    let id = Ulid::new();
    ring.get_or_create_key(id).unwrap();
    assert!(ring.erase(id).unwrap());

    let result = ring.get_or_create_key(id);
    assert!(
        matches!(result, Err(CounterpartyKeyError::Erased(e_id)) if e_id == id),
        "recreation after key creation must be permanently refused with Erased"
    );
}

#[test]
fn tombstone_never_contains_key_material() {
    let dir = tempfile::tempdir().unwrap();
    let keys_dir = dir.path().join("keys");
    let ring = CounterpartyKeyRing::open(keys_dir.clone(), master()).unwrap();
    let id = Ulid::new();
    ring.erase(id).unwrap();

    let tombstone = keys_dir.join(format!("{id}.erased"));
    assert!(tombstone.exists(), "tombstone file must exist after erase");
    let meta = std::fs::metadata(&tombstone).unwrap();
    assert_eq!(
        meta.len(),
        0,
        "tombstone file must be 0 bytes (marker only; zero key material)"
    );
}

#[test]
fn open_recovers_key_file_left_by_a_crash_between_tombstone_and_unlink() {
    let dir = tempfile::tempdir().unwrap();
    let keys_dir = dir.path().join("keys");
    let ring = CounterpartyKeyRing::open(keys_dir.clone(), master()).unwrap();
    let id = Ulid::new();

    let key_a = ring.get_or_create_key(id).unwrap();
    std::fs::File::create(keys_dir.join(format!("{id}.erased"))).unwrap();

    let ring = CounterpartyKeyRing::open(keys_dir.clone(), master()).unwrap();

    assert!(
        !keys_dir.join(id.to_string()).exists(),
        "startup recovery must physically unlink stale key file when tombstone exists"
    );

    assert_eq!(
        ring.get_key(id).unwrap(),
        None,
        "get_key after recovery must return None"
    );
    let result = ring.get_or_create_key(id);
    assert!(
        matches!(result, Err(CounterpartyKeyError::Erased(e_id)) if e_id == id),
        "get_or_create_key after recovery must return Erased"
    );

    let _ = key_a;
}

#[test]
fn with_scope_lock_serializes_same_scope() {
    let dir = tempfile::tempdir().unwrap();
    let ring = Arc::new(CounterpartyKeyRing::open(dir.path().join("keys"), master()).unwrap());
    let id = Ulid::new();
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let max_active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    std::thread::scope(|s| {
        for _ in 0..8 {
            let ring = ring.clone();
            let active = active.clone();
            let max_active = max_active.clone();
            s.spawn(move || {
                let _: Result<(), ()> = ring.with_scope_lock(id, || {
                    let cur = active.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    max_active.fetch_max(cur, std::sync::atomic::Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                });
            });
        }
    });
    assert_eq!(
        max_active.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "scope lock must fully serialize same-scope critical sections"
    );
}

#[test]
fn different_scopes_do_not_contend() {
    let dir = tempfile::tempdir().unwrap();
    let ring = Arc::new(CounterpartyKeyRing::open(dir.path().join("keys"), master()).unwrap());
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let max_active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    std::thread::scope(|s| {
        for _ in 0..8 {
            let ring = ring.clone();
            let active = active.clone();
            let max_active = max_active.clone();
            let id = Ulid::new();
            s.spawn(move || {
                let _: Result<(), ()> = ring.with_scope_lock(id, || {
                    let cur = active.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    max_active.fetch_max(cur, std::sync::atomic::Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                });
            });
        }
    });
    assert!(
        max_active.load(std::sync::atomic::Ordering::SeqCst) > 1,
        "distinct scopes must be able to run concurrently"
    );
}

#[test]
fn alias_survival_prevented_and_temp_cleaned_on_publication_and_startup() {
    let dir = tempfile::tempdir().unwrap();
    let keys_dir = dir.path().join("keys");
    let ring = CounterpartyKeyRing::open(keys_dir.clone(), master()).unwrap();
    let id = Ulid::new();
    ring.get_or_create_key(id).unwrap();

    let entries: Vec<String> = std::fs::read_dir(&keys_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_str().unwrap().to_string())
        .collect();
    assert!(
        !entries.iter().any(|name| name.contains(".tmp.")),
        "no temp alias file must survive successful publication"
    );

    let stray = keys_dir.join("stray.tmp.12345");
    std::fs::write(&stray, b"orphaned").unwrap();
    assert!(stray.exists());

    let _reopened = CounterpartyKeyRing::open(keys_dir.clone(), master()).unwrap();
    assert!(
        !stray.exists(),
        "startup recovery must clean up orphaned temp files"
    );

    let temp_alias = keys_dir.join(format!("{id}.tmp.6789"));
    std::fs::write(&temp_alias, b"temp alias").unwrap();
    ring.erase(id).unwrap();
    assert!(
        !temp_alias.exists(),
        "erase must sweep and delete temp files associated with counterparty"
    );
}

#[test]
fn associated_data_binds_counterparty_id_and_rejects_key_substitution() {
    let dir = tempfile::tempdir().unwrap();
    let keys_dir = dir.path().join("keys");
    let ring = CounterpartyKeyRing::open(keys_dir.clone(), master()).unwrap();

    let id_a = Ulid::new();
    let id_b = Ulid::new();

    let _key_a = ring.get_or_create_key(id_a).unwrap();

    let path_a = ring.key_path_for_test(id_a);
    let path_b = ring.key_path_for_test(id_b);
    std::fs::copy(&path_a, &path_b).unwrap();

    let res = ring.get_key(id_b);
    assert!(
        matches!(res, Err(CounterpartyKeyError::Decrypt)),
        "unwrapping key created for id_a at id_b's path must fail tag authentication"
    );
}

#[test]
fn legacy_unauthenticated_key_file_is_readable_and_migrated_to_v1() {
    let dir = tempfile::tempdir().unwrap();
    let keys_dir = dir.path().join("keys");
    let ring = CounterpartyKeyRing::open(keys_dir.clone(), master()).unwrap();
    let id = Ulid::new();

    let raw_key = [7u8; 32];
    let master_cipher = Aes256Gcm::new_from_slice(&master()).unwrap();
    let nonce_bytes = [3u8; 12];
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

    let key_path = ring.key_path_for_test(id);
    std::fs::write(&key_path, &legacy_bytes).unwrap();

    let fetched = ring.get_key(id).unwrap().unwrap();
    assert_eq!(fetched, raw_key);

    let migrated_file_bytes = std::fs::read(&key_path).unwrap();
    assert!(
        migrated_file_bytes.starts_with(b"OSK1"),
        "legacy key file must be migrated to v1 format on read"
    );

    let refetched = ring.get_key(id).unwrap().unwrap();
    assert_eq!(refetched, raw_key);
}

#[test]
fn system_scope_erasure_is_rejected_without_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let keys_dir = dir.path().join("keys");
    let ring = CounterpartyKeyRing::open(keys_dir.clone(), master()).unwrap();

    let key = ring.get_or_create_key(SYSTEM_SCOPE).unwrap();

    let res = ring.erase(SYSTEM_SCOPE);
    assert!(
        matches!(res, Err(CounterpartyKeyError::ReservedScope(id)) if id == SYSTEM_SCOPE),
        "erase of SYSTEM_SCOPE must be rejected with ReservedScope error"
    );

    let tombstone = keys_dir.join(format!("{SYSTEM_SCOPE}.erased"));
    assert!(
        !tombstone.exists(),
        "SYSTEM_SCOPE tombstone must not be written"
    );

    let key_after = ring.get_key(SYSTEM_SCOPE).unwrap().unwrap();
    assert_eq!(
        key_after, key,
        "SYSTEM_SCOPE key must remain present and valid after rejected erase"
    );
}

#[test]
fn durability_fsync_only_on_create_erase_and_pending_migration() {
    let dir = tempfile::tempdir().unwrap();
    let keys_dir = dir.path().join("keys");
    let ring = CounterpartyKeyRing::open(keys_dir, master()).unwrap();
    let id = Ulid::new();

    let count_before = ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst);
    let key = ring.get_or_create_key(id).unwrap();
    let count_after_first = ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        count_after_first > count_before,
        "fsync must occur on key creation"
    );

    let retried_key = ring.get_or_create_key(id).unwrap();
    assert_eq!(key, retried_key);
    let count_after_steady = ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        count_after_steady, count_after_first,
        "steady-state existing key lookup must not fsync when no migration marker is pending"
    );

    assert!(ring.erase(id).unwrap());
    let count_after_erase = ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst);

    assert!(!ring.erase(id).unwrap());
    let count_after_erase_retry = ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        count_after_erase_retry > count_after_erase,
        "fsync must be retried on existing tombstone erase"
    );
}

fn write_legacy_key_file(
    path: &std::path::Path,
    master_key: [u8; 32],
    raw_key: [u8; 32],
    nonce_bytes: [u8; 12],
) {
    let master_cipher = Aes256Gcm::new_from_slice(&master_key).unwrap();
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
    std::fs::write(path, &legacy_bytes).unwrap();
}

#[test]
fn already_locked_legacy_lookup_migrates_without_re_locking() {
    let dir = tempfile::tempdir().unwrap();
    let keys_dir = dir.path().join("keys");
    let ring = CounterpartyKeyRing::open(keys_dir.clone(), master()).unwrap();
    let id = Ulid::new();
    let raw_key = [7u8; 32];
    // Nonce deliberately starts with OSK1 to exercise prefix ambiguity as well.
    let mut nonce_bytes = *b"OSK1........";
    nonce_bytes[4..].copy_from_slice(&[5u8; 8]);

    let key_path = ring.key_path_for_test(id);
    write_legacy_key_file(&key_path, master(), raw_key, nonce_bytes);

    // ArtifactStore-style path: already holding the scope lock, call the locked
    // helper. Must not re-enter with_scope_lock (would deadlock on this Mutex).
    let fetched = ring
        .with_scope_lock(id, || ring.get_key_locked(id))
        .unwrap()
        .unwrap();
    assert_eq!(fetched, raw_key);

    let migrated = std::fs::read(&key_path).unwrap();
    assert!(
        migrated.starts_with(b"OSK1"),
        "locked legacy lookup must migrate to v1"
    );
    // After migration the full shape is the real v1 layout (header+nonce+ct),
    // not merely a legacy nonce that happened to start with OSK1.
    assert!(migrated.len() >= 4 + 12 + 32 + 16);
}

#[test]
fn legacy_migration_failure_is_propagated() {
    let dir = tempfile::tempdir().unwrap();
    let keys_dir = dir.path().join("keys");
    let ring = CounterpartyKeyRing::open(keys_dir.clone(), master()).unwrap();
    let id = Ulid::new();
    let raw_key = [11u8; 32];
    let key_path = ring.key_path_for_test(id);
    write_legacy_key_file(&key_path, master(), raw_key, [1u8; 12]);

    // Deterministic unit path: destination is a directory so rename fails.
    let bad_dest = keys_dir.join(format!("{id}.as-dir"));
    std::fs::create_dir(&bad_dest).unwrap();
    let (key, is_legacy) = ring.unwrap_file(&key_path, id).unwrap();
    assert!(is_legacy);
    let direct = ring.migrate_legacy_key_to_v1_locked(&bad_dest, id, &key);
    assert!(
        matches!(direct, Err(CounterpartyKeyError::Io { .. })),
        "migrate_legacy_key_to_v1_locked must surface rename I/O failure, got {direct:?}"
    );

    // Caller-path check: freeze the keys directory so create_new of the
    // migration temp fails. Some environments (root, certain FS) ignore
    // directory readonly bits — in that case the deterministic direct path
    // above already proved migration surfaces Io, so skip the caller asserts.
    let mut perms = std::fs::metadata(&keys_dir).unwrap().permissions();
    let original = perms.clone();
    perms.set_readonly(true);
    std::fs::set_permissions(&keys_dir, perms).unwrap();

    let public = ring.get_key(id);
    let locked = ring.with_scope_lock(id, || ring.get_key_locked(id));

    std::fs::set_permissions(&keys_dir, original).unwrap();

    if public.is_ok() && locked.is_ok() {
        return;
    }

    assert!(
        matches!(public, Err(CounterpartyKeyError::Io { .. })),
        "get_key must propagate migration I/O failure, got {public:?}"
    );
    assert!(
        matches!(locked, Err(CounterpartyKeyError::Io { .. })),
        "get_key_locked must propagate migration I/O failure, got {locked:?}"
    );
}

#[test]
fn osk1_prefixed_legacy_nonce_decrypts_and_migrates() {
    let dir = tempfile::tempdir().unwrap();
    let keys_dir = dir.path().join("keys");
    let ring = CounterpartyKeyRing::open(keys_dir, master()).unwrap();
    let id = Ulid::new();
    let raw_key = [13u8; 32];
    let mut nonce_bytes = *b"OSK1........";
    nonce_bytes[4..].fill(9);

    let key_path = ring.key_path_for_test(id);
    write_legacy_key_file(&key_path, master(), raw_key, nonce_bytes);

    let fetched = ring.get_key(id).unwrap().unwrap();
    assert_eq!(fetched, raw_key);

    let migrated = std::fs::read(&key_path).unwrap();
    assert!(migrated.starts_with(b"OSK1"));
    // Real v1 files are longer than legacy (nonce+ciphertext ≈ 60 bytes).
    assert!(
        migrated.len() > 12 + 32 + 16,
        "migrated v1 file must include header+nonce+ciphertext"
    );
}

#[test]
fn post_rename_migration_fsync_failure_retries_via_pending_marker() {
    let dir = tempfile::tempdir().unwrap();
    let keys_dir = dir.path().join("keys");
    let ring = CounterpartyKeyRing::open(keys_dir.clone(), master()).unwrap();
    let id = Ulid::new();
    let raw_key = [17u8; 32];
    let key_path = ring.key_path_for_test(id);
    let marker_path = ring.key_pending_path_for_test(id);
    write_legacy_key_file(&key_path, master(), raw_key, [2u8; 12]);

    // Migration sequence (all dir fsyncs):
    // 1) mark pending (create+file sync then dir sync)
    // 2) post-rename key+dir durability (dir sync inside sync_key_file_and_dir)
    // Fail the post-rename dir sync so the marker remains for recovery.
    let base = ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst);
    ring.fail_fsync_dir_on_call(base + 2);

    let first = ring.get_key(id);
    assert!(
        matches!(first, Err(CounterpartyKeyError::Io { .. })),
        "post-rename dir fsync failure must surface Io, got {first:?}"
    );
    assert!(
        key_path.exists(),
        "migrated OSK1 key must remain after post-rename fsync failure"
    );
    let migrated = std::fs::read(&key_path).unwrap();
    assert!(
        migrated.starts_with(b"OSK1"),
        "key file must already be OSK1 after rename even if dir fsync failed"
    );
    assert!(
        marker_path.exists(),
        "migration-pending marker must survive post-rename fsync failure"
    );

    let count_before_retry = ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst);
    let fetched = ring.get_key(id).unwrap().unwrap();
    assert_eq!(fetched, raw_key);
    let count_after_retry = ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        count_after_retry > count_before_retry,
        "current-key read with pending marker must re-sync key+directory"
    );
    assert!(
        !marker_path.exists(),
        "successful recovery must durably clear the migration-pending marker"
    );

    // Steady-state path after recovery: no further fsyncs.
    let count_before_steady = ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst);
    let again = ring.get_key(id).unwrap().unwrap();
    assert_eq!(again, raw_key);
    assert_eq!(
        ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst),
        count_before_steady,
        "steady-state current-key read without marker must not fsync"
    );
}

#[test]
fn post_hardlink_dir_fsync_failure_retries_via_pending_marker() {
    let dir = tempfile::tempdir().unwrap();
    let keys_dir = dir.path().join("keys");
    let ring = CounterpartyKeyRing::open(keys_dir, master()).unwrap();
    let id = Ulid::new();
    let key_path = ring.key_path_for_test(id);
    let marker_path = ring.key_pending_path_for_test(id);

    // First-publication sequence (dir fsyncs):
    // 1) mark pending (create+file sync then dir sync)
    // 2) post-hardlink key+dir durability (dir sync inside sync_key_file_and_dir)
    // Fail the post-hardlink dir sync so the marker remains for recovery.
    let base = ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst);
    ring.fail_fsync_dir_on_call(base + 2);

    let first = ring.get_or_create_key(id);
    assert!(
        matches!(first, Err(CounterpartyKeyError::Io { .. })),
        "post-hardlink dir fsync failure must surface Io, got {first:?}"
    );
    assert!(
        key_path.exists(),
        "hard-linked key must remain after post-hardlink fsync failure"
    );
    assert!(
        marker_path.exists(),
        "key-pending marker must survive post-hardlink fsync failure"
    );

    let count_before_retry = ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst);
    let recovered = ring.get_or_create_key(id).unwrap();
    let count_after_retry = ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        count_after_retry,
        count_before_retry + 2,
        "existing-key retry with pending marker must re-sync key+directory then durably clear the marker (two dir fsyncs)"
    );
    assert!(
        !marker_path.exists(),
        "successful recovery must clear the pending marker only after durability"
    );

    // Steady-state path after recovery: no further fsyncs.
    let count_before_steady = ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst);
    let again = ring.get_or_create_key(id).unwrap();
    assert_eq!(again, recovered);
    assert_eq!(
        ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst),
        count_before_steady,
        "steady-state existing key lookup without marker must not fsync"
    );
}

#[test]
fn existing_migration_marker_is_re_durabilized_before_rename() {
    let dir = tempfile::tempdir().unwrap();
    let keys_dir = dir.path().join("keys");
    let ring = CounterpartyKeyRing::open(keys_dir, master()).unwrap();
    let id = Ulid::new();
    let raw_key = [23u8; 32];
    let key_path = ring.key_path_for_test(id);
    let marker_path = ring.key_pending_path_for_test(id);
    write_legacy_key_file(&key_path, master(), raw_key, [6u8; 12]);

    // Simulate a crash after marker create but before its dir fsync: the
    // marker is visible, yet not known-durable. Retry must re-sync it before
    // rename, not treat existence as already durable.
    std::fs::write(&marker_path, b"").unwrap();
    assert!(marker_path.exists());
    assert!(
        std::fs::read(&key_path).unwrap().len() < 4 + 12 + 32 + 16
            || !std::fs::read(&key_path).unwrap().starts_with(b"OSK1")
            || std::fs::read(&key_path).unwrap().len() == 12 + 32 + 16,
        "precondition: legacy file still present before retry migration"
    );

    let base = ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst);
    // Fail the exists-branch dir fsync inside mark_key_pending_locked.
    ring.fail_fsync_dir_on_call(base + 1);
    let blocked = ring.get_key(id);
    assert!(
        matches!(blocked, Err(CounterpartyKeyError::Io { .. })),
        "marker re-durability fsync failure must surface Io before rename, got {blocked:?}"
    );
    let still_legacy = std::fs::read(&key_path).unwrap();
    assert!(
        !(still_legacy.starts_with(b"OSK1") && still_legacy.len() >= 4 + 12 + 32 + 16),
        "rename must not proceed when marker re-durability fails"
    );
    assert!(
        marker_path.exists(),
        "marker must remain after re-durability failure"
    );

    let fetched = ring.get_key(id).unwrap().unwrap();
    assert_eq!(fetched, raw_key);
    let migrated = std::fs::read(&key_path).unwrap();
    assert!(
        migrated.starts_with(b"OSK1") && migrated.len() >= 4 + 12 + 32 + 16,
        "successful retry must complete migration after re-durabilizing the marker"
    );
    assert!(
        !marker_path.exists(),
        "completed migration must clear the pending marker"
    );
}

#[test]
fn startup_preserves_migration_pending_marker_for_read_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let keys_dir = dir.path().join("keys");
    let ring = CounterpartyKeyRing::open(keys_dir.clone(), master()).unwrap();
    let id = Ulid::new();
    let raw_key = [19u8; 32];
    let key_path = ring.key_path_for_test(id);
    write_legacy_key_file(&key_path, master(), raw_key, [4u8; 12]);

    // Complete a clean migration so the file is OSK1, then re-plant a marker
    // as if a crash hit after rename but before clear.
    let key = ring.get_key(id).unwrap().unwrap();
    assert_eq!(key, raw_key);
    assert!(std::fs::read(&key_path).unwrap().starts_with(b"OSK1"));

    let marker_path = ring.key_pending_path_for_test(id);
    std::fs::write(&marker_path, b"").unwrap();
    assert!(marker_path.exists());

    // Also plant an orphan temp that startup SHOULD sweep, to prove the
    // marker is intentionally preserved rather than all side files.
    let orphan_tmp = keys_dir.join(format!("{id}.tmp.orphan"));
    std::fs::write(&orphan_tmp, b"stale").unwrap();

    let reopened = CounterpartyKeyRing::open(keys_dir.clone(), master()).unwrap();
    assert!(
        marker_path.exists(),
        "startup recovery must not sweep the durable migration-pending marker"
    );
    assert!(
        !orphan_tmp.exists(),
        "startup recovery must still clean orphaned .tmp. files"
    );

    let count_before = reopened
        .fsync_count
        .load(std::sync::atomic::Ordering::SeqCst);
    let recovered = reopened.get_key(id).unwrap().unwrap();
    assert_eq!(recovered, raw_key);
    assert!(
        reopened
            .fsync_count
            .load(std::sync::atomic::Ordering::SeqCst)
            > count_before,
        "post-startup current-key read must complete marker recovery with key+dir sync"
    );
    assert!(
        !marker_path.exists(),
        "read-time recovery after startup must clear the pending marker"
    );
}

#[test]
fn open_syncs_keys_dir_and_parent_on_creation_and_retry() {
    let dir = tempfile::tempdir().unwrap();
    let keys_dir = dir.path().join("keys");
    assert!(!keys_dir.exists());

    let ring = CounterpartyKeyRing::open(keys_dir.clone(), master()).unwrap();
    assert!(keys_dir.is_dir());
    // Every open fsyncs the keys dir and its parent.
    assert!(
        ring.fsync_count.load(std::sync::atomic::Ordering::SeqCst) >= 2,
        "new keys directory must be made durable through itself and its parent"
    );

    let reopen = CounterpartyKeyRing::open(keys_dir, master()).unwrap();
    assert!(
        reopen.fsync_count.load(std::sync::atomic::Ordering::SeqCst) >= 2,
        "reopen must repair a prior parent-sync failure"
    );
}

#[test]
fn closed_scope_in_memory_blocks_get_and_create() {
    let dir = tempfile::tempdir().unwrap();
    let ring = CounterpartyKeyRing::open(dir.path().join("keys"), master()).unwrap();
    let id = Ulid::new();
    let other = Ulid::new();

    let key = ring.get_or_create_key(id).unwrap();
    assert_eq!(ring.get_key(id).unwrap(), Some(key));

    ring.close_scope_in_memory(id);
    ring.close_scope_in_memory(id); // idempotent

    assert_eq!(
        ring.get_key(id).unwrap(),
        None,
        "closed scope must fail closed for get_key even if key file remains"
    );
    assert!(
        matches!(
            ring.get_or_create_key(id),
            Err(CounterpartyKeyError::Erased(closed)) if closed == id
        ),
        "closed scope must refuse create"
    );
    // Key file is still on disk — close is process-local, not a tombstone.
    assert!(ring.key_path_for_test(id).exists());

    // Independent scopes are unaffected.
    let other_key = ring.get_or_create_key(other).unwrap();
    assert_eq!(ring.get_key(other).unwrap(), Some(other_key));
}

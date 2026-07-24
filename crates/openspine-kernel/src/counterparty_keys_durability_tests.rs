use super::*;

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

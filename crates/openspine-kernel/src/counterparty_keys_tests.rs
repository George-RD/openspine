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
fn directory_erased_path_is_rejected_not_treated_as_tombstone() {
    let dir = tempfile::tempdir().unwrap();
    let keys = dir.path().join("keys");
    let ring = CounterpartyKeyRing::open(keys.clone(), master()).unwrap();
    let id = Ulid::new();
    ring.get_or_create_key(id).unwrap();

    // Replace any future tombstone path with a directory before erase.
    let tomb = keys.join(format!("{id}.erased"));
    // erase will try to create tombstone file; plant directory first.
    std::fs::create_dir_all(&tomb).unwrap();
    let err = ring.erase(id).unwrap_err();
    assert!(
        matches!(err, CounterpartyKeyError::Io { .. }),
        "directory tombstone must not count as erased, got {err:?}"
    );
    // Key file must still exist — erase failed closed before unlink.
    assert!(ring.key_path_for_test(id).is_file());
    // get_or_create must also reject the directory as invalid state, not Erased.
    let res = ring.get_or_create_key(id);
    assert!(
        matches!(res, Err(CounterpartyKeyError::Io { .. })),
        "expected Io for directory tombstone, got {res:?}"
    );
}

#[path = "counterparty_keys_durability_tests.rs"]
mod durability_tests;

use super::*;

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

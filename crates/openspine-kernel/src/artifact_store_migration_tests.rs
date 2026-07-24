use super::*;

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

#[path = "artifact_store_format_tests.rs"]
mod format_tests;

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

#[path = "artifact_store_migration_tests.rs"]
mod migration_tests;

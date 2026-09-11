use super::*;

const MASTER_KEY: [u8; 32] = [29; 32];

fn write_legacy_wrapped_key(
    path: &std::path::Path,
    master_key: [u8; 32],
    raw_key: [u8; 32],
) -> Vec<u8> {
    let master_cipher = Aes256Gcm::new_from_slice(&master_key).unwrap();
    let nonce_bytes = [9u8; NONCE_LEN];
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
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&nonce_bytes);
    bytes.extend_from_slice(&ciphertext);
    std::fs::write(path, &bytes).unwrap();
    bytes
}

fn write_blob(
    store: &ArtifactStore,
    scope: Ulid,
    plaintext: &[u8],
    key: [u8; 32],
    format: u8,
) -> (ArtifactRef, std::path::PathBuf, Vec<u8>) {
    let artifact_ref = ArtifactRef {
        digest: digest_of_bytes(plaintext),
        schema_version: 1,
    };
    let path = store.blob_path_for_test_scoped(scope, &artifact_ref);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();

    let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
    let nonce_bytes = [7u8; NONCE_LEN];
    let nonce = Nonce::try_from(nonce_bytes.as_slice()).unwrap();
    let mut raw = Vec::new();
    let header = ArtifactStore::blob_header(format, scope);
    raw.extend_from_slice(&header);
    raw.extend_from_slice(&nonce_bytes);
    let ciphertext = if format == CURRENT_FORMAT {
        cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: &header,
                },
            )
            .unwrap()
    } else {
        cipher.encrypt(&nonce, plaintext).unwrap()
    };
    raw.extend_from_slice(&ciphertext);
    std::fs::write(&path, &raw).unwrap();
    (artifact_ref, path, raw)
}

#[test]
fn read_only_scoped_read_does_not_migrate_legacy_key() {
    let dir = tempfile::tempdir().unwrap();
    let master_key = MASTER_KEY;
    let store = ArtifactStore::open(dir.path().join("artifacts"), master_key).unwrap();
    let scope = Ulid::new();
    let raw_key = [17u8; 32];
    let key_path = store.keys.key_path_for_test(scope);
    let legacy_key = write_legacy_wrapped_key(&key_path, master_key, raw_key);
    let (artifact_ref, blob_path, blob_before) =
        write_blob(&store, scope, b"review evidence", raw_key, CURRENT_FORMAT);

    assert_eq!(
        store
            .get_scoped_without_recovery(scope, &artifact_ref)
            .unwrap(),
        b"review evidence"
    );
    assert_eq!(std::fs::read(&key_path).unwrap(), legacy_key);
    assert_eq!(std::fs::read(&blob_path).unwrap(), blob_before);
}

#[test]
fn read_only_scoped_read_does_not_upgrade_recovered_blob() {
    let dir = tempfile::tempdir().unwrap();
    let store = ArtifactStore::open(dir.path().join("artifacts"), MASTER_KEY).unwrap();
    let scope = Ulid::new();
    let raw_key = store.keys.get_or_create_key(scope).unwrap();
    let (artifact_ref, blob_path, blob_before) = write_blob(
        &store,
        scope,
        b"recovered review evidence",
        raw_key,
        RECOVERED_FORMAT,
    );

    assert_eq!(
        store
            .get_scoped_without_recovery(scope, &artifact_ref)
            .unwrap(),
        b"recovered review evidence"
    );
    assert_eq!(std::fs::read(&blob_path).unwrap(), blob_before);
    assert!(!ArtifactStore::upgrade_pending_path(&blob_path).exists());
}

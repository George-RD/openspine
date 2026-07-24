// openspine:allow-large-module reason: cohesive end-to-end crypto-erasure matrix shares scoped-store, provenance, audit-chain, idempotency, and stale-reconfirmation fixtures.
use super::*;
use openspine_schemas::artifact::ArtifactRef;

fn master_key() -> [u8; 32] {
    [4u8; 32]
}

fn learned_row(
    kind: &str,
    artifact_id: &str,
    version: u32,
    source_exchange: &ArtifactRef,
    source_scope: Ulid,
    compatibility: crate::store::learned_artifacts::CompatibilityStatus,
) -> crate::store::learned_artifacts::LearnedArtifact {
    crate::store::learned_artifacts::LearnedArtifact {
        kind: kind.to_string(),
        artifact_id: artifact_id.to_string(),
        version,
        namespace: openspine_schemas::artifact::ArtifactNamespace::Overlay,
        provenance: crate::store::learned_artifacts::Provenance::ProducedBy {
            source_event_id: Ulid::new(),
            source_exchange: source_exchange.clone(),
            source_scope,
        },
        accepted_via: None,
        learned_at: jiff::Timestamp::now(),
        compatibility,
        nomination: crate::store::learned_artifacts::NominationStatus::None,
        pending_reconfirmation_id: None,
        pending_yaml_digest: None,
        accepted_base_epoch: None,
        accepted_dependency_fingerprint: None,
        source_path: None,
    }
}

fn operations_for(
    data_root: &std::path::Path,
    master: [u8; 32],
) -> crate::overlay_export_restore::OverlayOperations {
    let ops = crate::overlay_export_restore::OverlayOperations::acquire(data_root, &master)
        .expect("acquire overlay operations");
    ops.initialize_terminal_ledger()
        .expect("initialize terminal ledger");
    ops
}

#[test]
fn erasing_a_counterparty_invalidates_derived_artifacts_and_unrecovers_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());

    let counterparty = Ulid::new();

    // A private payload belonging to the counterparty...
    let payload_ref = artifacts.put_scoped(counterparty, b"private DM").unwrap();
    // ...and a derived learned artifact mined from that exchange.
    let derived = learned_row(
        "route",
        "r1",
        1,
        &payload_ref,
        counterparty,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    store.record_learned_artifact(&derived).unwrap();

    let report = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap();
    assert_eq!(report.derived_artifacts_invalidated, 1);
    assert!(report.key_deleted);

    // The payload is now unrecoverable.
    let recovered = artifacts.get_scoped(counterparty, &payload_ref);
    assert!(matches!(
        recovered,
        Err(ArtifactStoreError::Decrypt | ArtifactStoreError::KeyRing(_))
    ));

    // The derived artifact is flipped to Erased (terminal).
    let updated = store.list_learned_artifacts().unwrap();
    assert_eq!(updated.len(), 1);
    assert_eq!(
        updated[0].compatibility,
        crate::store::learned_artifacts::CompatibilityStatus::Erased
    );
}

#[test]
fn chain_verification_passes_after_erasure() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());

    // Establish an intact chain (one pre-existing audit row).
    store
        .append_audit("system.boot", None, None, None, None, &[], &[])
        .unwrap();
    assert!(store.verify_audit_chain().unwrap());

    let counterparty = Ulid::new();
    let payload_ref = artifacts.put_scoped(counterparty, b"private DM").unwrap();
    let derived = learned_row(
        "route",
        "r1",
        1,
        &payload_ref,
        counterparty,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    store.record_learned_artifact(&derived).unwrap();

    erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap();

    // The erase appended a `counterparty.erased` audit row but never
    // mutated or deleted an existing one — the chain stays intact.
    assert!(
        store.verify_audit_chain().unwrap(),
        "audit chain must still verify after a crypto-erase"
    );

    // And the new erase audit row is actually present.
    let rows = store
        .count_audit_events_of_kind("counterparty.erased")
        .unwrap();
    assert_eq!(rows, 1);
}

#[test]
fn erase_does_not_touch_other_counterparties() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());

    let alice = Ulid::new();
    let bob = Ulid::new();
    let alice_ref = artifacts.put_scoped(alice, b"alice secret").unwrap();
    let bob_ref = artifacts.put_scoped(bob, b"bob secret").unwrap();

    let alice_derived = learned_row(
        "route",
        "alice_rule",
        1,
        &alice_ref,
        alice,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    store.record_learned_artifact(&alice_derived).unwrap();
    let bob_derived = learned_row(
        "route",
        "bob_rule",
        1,
        &bob_ref,
        bob,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    store.record_learned_artifact(&bob_derived).unwrap();

    erase_counterparty(&store, &artifacts, &operations, alice).unwrap();

    // Alice's payload/blobs gone; Bob's intact.
    assert!(matches!(
        artifacts.get_scoped(alice, &alice_ref),
        Err(ArtifactStoreError::Decrypt | ArtifactStoreError::KeyRing(_))
    ));
    assert_eq!(artifacts.get_scoped(bob, &bob_ref).unwrap(), b"bob secret");

    let rows = store.list_learned_artifacts().unwrap();
    let by_id: std::collections::HashMap<_, _> = rows
        .iter()
        .map(|l| (l.artifact_id.clone(), l.compatibility))
        .collect();
    assert_eq!(
        by_id["alice_rule"],
        crate::store::learned_artifacts::CompatibilityStatus::Erased
    );
    assert_eq!(
        by_id["bob_rule"],
        crate::store::learned_artifacts::CompatibilityStatus::Compatible
    );
}

#[test]
fn ciphertext_remains_on_disk_but_is_unrecoverable_after_erase() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());

    let counterparty = Ulid::new();
    let payload_ref = artifacts.put_scoped(counterparty, b"private DM").unwrap();
    let path = artifacts.blob_path_for_test_scoped(counterparty, &payload_ref);
    assert!(path.exists(), "ciphertext blob is on disk before erase");
    assert_eq!(
        artifacts.get_scoped(counterparty, &payload_ref).unwrap(),
        b"private DM"
    );

    erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap();

    // Crypto-erase deletes the KEY, not the ciphertext. The blob stays
    // physically present (so erasure is never a deniable rewrite), but
    // with the key gone it can never be decrypted again.
    assert!(
        path.exists(),
        "ciphertext blob must remain on disk after crypto-erase"
    );
    let recovered = artifacts.get_scoped(counterparty, &payload_ref);
    assert!(matches!(
        recovered,
        Err(ArtifactStoreError::Decrypt | ArtifactStoreError::KeyRing(_))
    ));
}

#[test]
fn double_erase_is_idempotent_and_chain_still_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());

    store
        .append_audit("system.boot", None, None, None, None, &[], &[])
        .unwrap();
    assert!(store.verify_audit_chain().unwrap());

    let counterparty = Ulid::new();
    let payload_ref = artifacts.put_scoped(counterparty, b"private DM").unwrap();
    let derived = learned_row(
        "route",
        "r1",
        1,
        &payload_ref,
        counterparty,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    store.record_learned_artifact(&derived).unwrap();

    let first = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap();
    assert!(first.key_deleted);
    assert_eq!(first.derived_artifacts_invalidated, 1);

    // Second erase: safe no-op. The key is already gone and the artifact
    // already Erased, so nothing further happens and nothing errors.
    let second = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap();
    assert!(!second.key_deleted, "second erase finds no key to delete");
    assert_eq!(
        second.derived_artifacts_invalidated, 0,
        "second erase re-invalidates nothing"
    );

    // The chain still verifies and exactly one erase audit row exists.
    assert!(store.verify_audit_chain().unwrap());
    assert_eq!(
        store
            .count_audit_events_of_kind("counterparty.erased")
            .unwrap(),
        1
    );

    // A different counterparty is wholly untouched.
    let other = Ulid::new();
    let other_ref = artifacts.put_scoped(other, b"other secret").unwrap();
    assert_eq!(
        artifacts.get_scoped(other, &other_ref).unwrap(),
        b"other secret"
    );
}

#[test]
fn ledger_write_failure_leaves_generation_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    // Deliberately skip initialize_terminal_ledger so record fails with MissingContinuity.
    let operations =
        crate::overlay_export_restore::OverlayOperations::acquire(dir.path(), &master_key())
            .expect("acquire overlay operations");

    let counterparty = Ulid::new();
    let payload_ref = artifacts.put_scoped(counterparty, b"still live").unwrap();
    let derived = learned_row(
        "route",
        "r_ledger_fail",
        1,
        &payload_ref,
        counterparty,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    store.record_learned_artifact(&derived).unwrap();

    let err = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap_err();
    assert!(
        matches!(err, CounterpartyEraseError::Control(_)),
        "ledger failure must surface as control error, got {err:?}"
    );

    // Generation-local state must be untouched.
    assert_eq!(
        store.list_learned_artifacts().unwrap()[0].compatibility,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible
    );
    assert!(store.erased_counterparty_ids().unwrap().is_empty());
    assert_eq!(
        artifacts.get_scoped(counterparty, &payload_ref).unwrap(),
        b"still live"
    );
    // Scope must not have been closed in memory.
    assert_eq!(
        artifacts.put_scoped(counterparty, b"still live").unwrap(),
        payload_ref
    );
}

#[test]
fn post_ledger_db_failure_closes_reads_and_startup_retry_completes() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("kernel.db");
    let store = Store::open(&db_path).unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());

    let counterparty = Ulid::new();
    let payload_ref = artifacts.put_scoped(counterparty, b"retry me").unwrap();
    let derived = learned_row(
        "route",
        "r_retry",
        1,
        &payload_ref,
        counterparty,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    store.record_learned_artifact(&derived).unwrap();

    // After durable ledger write + in-memory close, force DB invalidation to fail
    // by opening a read-only store for the generation-local transaction.
    drop(store);
    let readonly = Store::open_read_only_for_test(&db_path).unwrap();
    let err = erase_counterparty(&readonly, &artifacts, &operations, counterparty).unwrap_err();
    assert!(
        matches!(err, CounterpartyEraseError::Store(_)),
        "expected store failure after ledger write, got {err:?}"
    );
    // Same-process reads/writes must already be closed.
    assert!(
        artifacts.put_scoped(counterparty, b"blocked").is_err(),
        "post-ledger failure must leave scope closed in memory"
    );
    assert!(
        matches!(
            artifacts.get_scoped(counterparty, &payload_ref),
            Err(ArtifactStoreError::Decrypt | ArtifactStoreError::KeyRing(_))
        ),
        "closed scope must fail closed on reads"
    );
    // Ledger id is durable.
    assert!(operations
        .export_terminal_ledger()
        .unwrap()
        .erased_counterparty_ids()
        .contains(&counterparty.to_string()));

    // Startup-style retry with a fresh writable store + same operations completes.
    let store = Store::open(&db_path).unwrap();
    // New artifact store process image: reopen to clear in-memory closure and
    // prove ledger-driven reconciliation restores the fail-closed effects.
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    // Key may still decrypt if only in-memory closed; reconciliation must finish.
    reconcile_overlay_terminal_erasures(&store, &artifacts, &operations).unwrap();
    assert_eq!(
        store.list_learned_artifacts().unwrap()[0].compatibility,
        crate::store::learned_artifacts::CompatibilityStatus::Erased
    );
    assert_eq!(store.erased_counterparty_ids().unwrap(), vec![counterparty]);
    assert!(matches!(
        artifacts.get_scoped(counterparty, &payload_ref),
        Err(ArtifactStoreError::Decrypt | ArtifactStoreError::KeyRing(_))
    ));
    // Idempotent second reconcile.
    reconcile_overlay_terminal_erasures(&store, &artifacts, &operations).unwrap();
}

#[test]
fn erased_directory_is_rejected_not_tombstone() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());
    let counterparty = Ulid::new();
    let payload_ref = artifacts
        .put_scoped(counterparty, b"dir-tombstone")
        .unwrap();

    // Plant a directory at the `.erased` path — must not be treated as a tombstone.
    let tombstone = dir
        .path()
        .join("keys")
        .join(format!("{counterparty}.erased"));
    std::fs::create_dir_all(&tombstone).unwrap();
    assert!(tombstone.is_dir());

    let err = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap_err();
    assert!(
        matches!(
            err,
            CounterpartyEraseError::Store(_) | CounterpartyEraseError::Artifact(_)
        ),
        "directory tombstone must fail closed, got {err:?}"
    );
    // Because ledger write + in-memory close happen first, generation may be
    // partially closed; the directory must still never count as a valid tombstone.
    let ring =
        crate::counterparty_keys::CounterpartyKeyRing::open(dir.path().join("keys"), master_key());
    // Re-open key ring independently: directory must error, not Erased success.
    // open() itself only recovers pending erasures; get_or_create should error.
    // Use the store path: put_scoped after a successful close may be Erased,
    // but a pure key-ring open against a directory tombstone must not treat it
    // as erased success when creating.
    // Direct probe via a fresh ring:
    match crate::counterparty_keys::CounterpartyKeyRing::open(dir.path().join("keys"), master_key())
    {
        Ok(ring) => {
            let res = ring.get_or_create_key(counterparty);
            assert!(
                matches!(
                    res,
                    Err(crate::counterparty_keys::CounterpartyKeyError::Io { .. })
                ),
                "directory .erased must be Io rejection, got {res:?}"
            );
        }
        Err(crate::counterparty_keys::CounterpartyKeyError::Io { .. }) => {}
        Err(other) => panic!("unexpected open error: {other:?}"),
    }
    let _ = (store, artifacts, operations, payload_ref, ring);
}

#[test]
fn reconcile_existing_erasures_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());
    let counterparty = Ulid::new();
    let payload_ref = artifacts.put_scoped(counterparty, b"once").unwrap();
    store
        .record_learned_artifact(&learned_row(
            "route",
            "r_once",
            1,
            &payload_ref,
            counterparty,
            crate::store::learned_artifacts::CompatibilityStatus::Compatible,
        ))
        .unwrap();

    let first = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap();
    assert!(first.ledger_sequence >= 1);
    let second = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap();
    assert_eq!(second.derived_artifacts_invalidated, 0);
    assert_eq!(second.ledger_sequence, first.ledger_sequence);
    reconcile_overlay_terminal_erasures(&store, &artifacts, &operations).unwrap();
    reconcile_overlay_terminal_erasures(&store, &artifacts, &operations).unwrap();
    assert_eq!(
        store.list_learned_artifacts().unwrap()[0].compatibility,
        crate::store::learned_artifacts::CompatibilityStatus::Erased
    );
    assert!(store.verify_audit_chain().unwrap());
}

#[path = "counterparty_erasure_runtime_tests.rs"]
mod runtime_tests;

#[path = "counterparty_erasure_matrix_tests.rs"]
mod matrix_tests;

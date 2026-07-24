use super::*;

#[test]
fn identical_plaintext_erasure_only_flags_the_producing_counterparty() {
    // Two counterparties independently exchange the SAME plaintext (e.g.
    // a common phrase). Both content-address to the same digest, so a
    // digest-keyed resolution (blob header scope, or path existence
    // under the erased scope) cannot by itself tell which counterparty
    // produced which derived artifact -- both would appear to reference
    // "a blob that exists under alice's scope". Only the provenance
    // edge's own recorded `source_scope` disambiguates this.
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());

    let alice = Ulid::new();
    let bob = Ulid::new();
    let shared_ref = artifacts
        .put_scoped(alice, b"thanks, see you then")
        .unwrap();
    assert_eq!(
        artifacts.put_scoped(bob, b"thanks, see you then").unwrap(),
        shared_ref,
        "identical plaintext content-addresses to the same digest"
    );

    // Each counterparty's derived artifact references the SAME
    // source_exchange digest, but records its OWN producing scope.
    let alice_rule = learned_row(
        "route",
        "alice_rule",
        1,
        &shared_ref,
        alice,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    let bob_rule = learned_row(
        "route",
        "bob_rule",
        1,
        &shared_ref,
        bob,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    store.record_learned_artifact(&alice_rule).unwrap();
    store.record_learned_artifact(&bob_rule).unwrap();

    let report = erase_counterparty(&store, &artifacts, &operations, alice).unwrap();
    assert_eq!(
        report.derived_artifacts_invalidated, 1,
        "only alice's derived artifact is flagged, not bob's, despite sharing a digest"
    );

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
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
        "bob's rule must NOT be invalidated by alice's erasure just because it shares a digest"
    );

    // Alice's own scope-keyed copy is unrecoverable; Bob's own
    // scope-keyed copy of the SAME plaintext survives untouched.
    assert!(matches!(
        artifacts.get_scoped(alice, &shared_ref),
        Err(ArtifactStoreError::Decrypt | ArtifactStoreError::KeyRing(_))
    ));
    assert_eq!(
        artifacts.get_scoped(bob, &shared_ref).unwrap(),
        b"thanks, see you then"
    );
}

#[test]
fn erasing_keyless_scope_twice_audits_once_and_keeps_chain_valid() {
    // Regression for the post-COMMIT filesystem-failure retry bug: the
    // `counterparty.erased` audit row must be gated on a durable DB
    // marker (`erased_counterparties`, INSERT OR IGNORE) committed in
    // the SAME transaction as the audit row itself — NOT on filesystem
    // tombstone existence, which is a separate, non-atomic operation.
    // Erasing a scope that has no key and no derived artifacts twice
    // must emit exactly ONE audit row and leave the chain verifiable.
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());
    let counterparty = Ulid::new();
    let first = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap();
    // A keyless scope has no key file to delete, so `key_deleted` is
    // `false` — but the durable tombstone/DB marker still closes the
    // scope permanently. The audit (gated on that durable marker) is
    // the real effect.
    assert!(
        !first.key_deleted,
        "keyless scope has no key file to delete"
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("counterparty.erased")
            .unwrap(),
        1,
        "first erase must emit exactly one audit row"
    );
    assert!(
        store.verify_audit_chain().unwrap(),
        "chain must verify after the first erase"
    );
    // Simulate the exact partial-failure window the bug lived in: the
    // first erase committed its DB marker + audit row, but the
    // filesystem tombstone write did NOT make it to disk before a
    // crash/retry. If the audit gate had relied on FILESYSTEM tombstone
    // existence (the old design), a retry seeing no tombstone would
    // have re-audited. The durable DB marker prevents that: the retry
    // re-creates the marker (INSERT OR IGNORE, 0 rows affected) and
    // appends NO second audit.
    let tombstone = dir
        .path()
        .join("keys")
        .join(format!("{counterparty}.erased"));
    std::fs::remove_file(&tombstone).unwrap();
    let second = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap();
    assert!(!second.key_deleted, "retry still has no key file to delete");
    assert_eq!(
        store
            .count_audit_events_of_kind("counterparty.erased")
            .unwrap(),
        1,
        "retry must NOT append a duplicate counterparty.erased audit row \
         even when the filesystem tombstone was lost after the first commit"
    );

    // And the chain must remain verifiable after both passes.
    assert!(
        store.verify_audit_chain().unwrap(),
        "audit chain must stay verifiable across the idempotent erase"
    );
}
#[test]
fn system_scope_erasure_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());

    let res = erase_counterparty(
        &store,
        &artifacts,
        &operations,
        crate::counterparty_keys::SYSTEM_SCOPE,
    );
    assert!(res.is_err());
}

#[test]
fn erasure_report_contains_exact_invalidated_identities_and_d012_target_refs() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());

    let counterparty = Ulid::new();
    let payload_ref = artifacts.put_scoped(counterparty, b"payload").unwrap();

    let r1 = learned_row(
        "route",
        "r1",
        1,
        &payload_ref,
        counterparty,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    let r2 = learned_row(
        "persona",
        "p1",
        1,
        &payload_ref,
        counterparty,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    store.record_learned_artifact(&r1).unwrap();
    store.record_learned_artifact(&r2).unwrap();

    let report = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap();
    assert_eq!(report.derived_artifacts_invalidated, 2);
    assert_eq!(report.invalidated_identities.len(), 2);
    assert_eq!(report.invalidated_identities[0].artifact_id, "p1");
    assert_eq!(report.invalidated_identities[1].artifact_id, "r1");

    // Verify audit event carries target_refs
    let conn = store.conn.lock();
    let target_refs_json: String = conn
        .query_row(
            "SELECT meta_json FROM audit_log WHERE kind = 'counterparty.erased'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let meta: serde_json::Value = serde_json::from_str(&target_refs_json).unwrap();
    let target_refs = meta["target_refs"].as_array().unwrap();
    assert_eq!(target_refs.len(), 2);
}

#[test]
fn pending_reconfirmation_is_cancelled_on_erasure_and_cannot_revive_erased() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());

    let counterparty = Ulid::new();
    let payload_ref = artifacts.put_scoped(counterparty, b"payload").unwrap();
    let r1 = learned_row(
        "route",
        "r1",
        1,
        &payload_ref,
        counterparty,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    store.record_learned_artifact(&r1).unwrap();

    let request_id = Ulid::new();
    store
        .mark_reconfirmation_required("route", "r1", 1, request_id, "digest_abc")
        .unwrap();

    // Insert pending action request into DB so commit_owner_reconfirmation has a valid request
    {
        let conn = store.conn.lock();
        conn.execute(
            "INSERT INTO action_requests (id, request_json, used) VALUES (?1, '{}', 0)",
            rusqlite::params![request_id.to_string()],
        )
        .unwrap();
    }

    let report = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap();
    assert_eq!(report.derived_artifacts_invalidated, 1);

    let row = store.list_learned_artifacts().unwrap().remove(0);
    assert_eq!(
        row.compatibility,
        crate::store::learned_artifacts::CompatibilityStatus::Erased
    );
    assert_eq!(row.pending_reconfirmation_id, None);

    // Attempting to commit owner reconfirmation on the erased artifact returns Ok(false)
    let commit_res = store.commit_owner_reconfirmation(
        crate::store::learned_reconfirmation::OwnerReconfirmation {
            kind: "route".to_string(),
            artifact_id: "r1".to_string(),
            version: 1,
            provenance: r1.provenance.clone(),
            accepted_via: None,
            base_epoch: "epoch".to_string(),
            accepted_dependency_fingerprint: None,
            request_id,
            grant_id: None,
            review_ref: None,
            proposal_id: None,
            new_proposal: None,
            dangling_refs: vec![],
            superseded_old_version: None,
        },
    );
    assert!(!commit_res.unwrap());
    assert_eq!(
        store.list_learned_artifacts().unwrap()[0].compatibility,
        crate::store::learned_artifacts::CompatibilityStatus::Erased
    );
}

#[test]
fn cleanup_retry_returns_terminal_identities_with_zero_new_changes() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());

    let counterparty = Ulid::new();
    let payload_ref = artifacts
        .put_scoped(counterparty, b"retry payload")
        .unwrap();
    let r1 = learned_row(
        "route",
        "r1",
        1,
        &payload_ref,
        counterparty,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    let r2 = learned_row(
        "persona",
        "p1",
        1,
        &payload_ref,
        counterparty,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    store.record_learned_artifact(&r1).unwrap();
    store.record_learned_artifact(&r2).unwrap();

    let first = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap();
    assert_eq!(first.derived_artifacts_invalidated, 2);
    assert_eq!(first.invalidated_identities.len(), 2);
    assert!(first.key_deleted);

    // Simulate a partial-failure cleanup retry: durable rows are already
    // terminal, but the caller still needs the matching identity set.
    let second = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap();
    assert_eq!(
        second.derived_artifacts_invalidated, 0,
        "retry must count only rows newly transitioned by this pass"
    );
    assert_eq!(
        second.invalidated_identities, first.invalidated_identities,
        "retry must still return every matching terminal identity"
    );
    assert!(!second.key_deleted);
}

#[test]
fn post_commit_cleanup_failure_still_closes_scope_in_memory() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());

    let counterparty = Ulid::new();
    let payload_ref = artifacts.put_scoped(counterparty, b"cleanup fail").unwrap();
    let derived = learned_row(
        "route",
        "r_cleanup",
        1,
        &payload_ref,
        counterparty,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    store.record_learned_artifact(&derived).unwrap();

    // After DB commit, physical cleanup creates a tombstone under keys/.
    // Replacing keys/ with a plain file makes that cleanup fail while the
    // durable marker/invalidation already committed.
    let keys_dir = dir.path().join("keys");
    std::fs::remove_dir_all(&keys_dir).unwrap();
    std::fs::write(&keys_dir, b"not-a-directory").unwrap();

    let err = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap_err();
    assert!(
        matches!(
            err,
            CounterpartyEraseError::Store(_) | CounterpartyEraseError::Artifact(_)
        ),
        "expected cleanup failure, got {err:?}"
    );

    assert_eq!(
        store.list_learned_artifacts().unwrap()[0].compatibility,
        crate::store::learned_artifacts::CompatibilityStatus::Erased
    );
    assert_eq!(store.erased_counterparty_ids().unwrap(), vec![counterparty]);

    // In-memory closure must have happened before the failed filesystem
    // cleanup, so same-process key creation stays fail-closed.
    let recreate = artifacts.put_scoped(counterparty, b"should fail");
    assert!(
        recreate.is_err(),
        "closed-in-memory scope must reject post-failure writes"
    );
}

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

#[test]
fn erasing_a_counterparty_invalidates_derived_artifacts_and_unrecovers_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();

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

    let report = erase_counterparty(&store, &artifacts, counterparty).unwrap();
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

    erase_counterparty(&store, &artifacts, counterparty).unwrap();

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

    erase_counterparty(&store, &artifacts, alice).unwrap();

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

    let counterparty = Ulid::new();
    let payload_ref = artifacts.put_scoped(counterparty, b"private DM").unwrap();
    let path = artifacts.blob_path_for_test_scoped(counterparty, &payload_ref);
    assert!(path.exists(), "ciphertext blob is on disk before erase");
    assert_eq!(
        artifacts.get_scoped(counterparty, &payload_ref).unwrap(),
        b"private DM"
    );

    erase_counterparty(&store, &artifacts, counterparty).unwrap();

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

    let first = erase_counterparty(&store, &artifacts, counterparty).unwrap();
    assert!(first.key_deleted);
    assert_eq!(first.derived_artifacts_invalidated, 1);

    // Second erase: safe no-op. The key is already gone and the artifact
    // already Erased, so nothing further happens and nothing errors.
    let second = erase_counterparty(&store, &artifacts, counterparty).unwrap();
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

    let report = erase_counterparty(&store, &artifacts, alice).unwrap();
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
    let counterparty = Ulid::new();
    let first = erase_counterparty(&store, &artifacts, counterparty).unwrap();
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
    let second = erase_counterparty(&store, &artifacts, counterparty).unwrap();
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

    let res = erase_counterparty(&store, &artifacts, crate::counterparty_keys::SYSTEM_SCOPE);
    assert!(res.is_err());
}

#[test]
fn erasure_report_contains_exact_invalidated_identities_and_d012_target_refs() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();

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

    let report = erase_counterparty(&store, &artifacts, counterparty).unwrap();
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

    let report = erase_counterparty(&store, &artifacts, counterparty).unwrap();
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

    let first = erase_counterparty(&store, &artifacts, counterparty).unwrap();
    assert_eq!(first.derived_artifacts_invalidated, 2);
    assert_eq!(first.invalidated_identities.len(), 2);
    assert!(first.key_deleted);

    // Simulate a partial-failure cleanup retry: durable rows are already
    // terminal, but the caller still needs the matching identity set.
    let second = erase_counterparty(&store, &artifacts, counterparty).unwrap();
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

    let err = erase_counterparty(&store, &artifacts, counterparty).unwrap_err();
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

#[test]
fn activated_standing_rule_becomes_unusable_after_source_scope_erasure() {
    use crate::standing_rules_gate::consult_standing_rule_gate;
    use crate::store::standing_rules_tests::manifest;
    use openspine_schemas::action::ActionId;
    use openspine_schemas::standing_rule::BudgetWindow;

    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();

    let counterparty = Ulid::new();
    let other = Ulid::new();
    let payload_ref = artifacts.put_scoped(counterparty, b"rule source").unwrap();
    let other_payload = artifacts.put_scoped(other, b"other rule source").unwrap();

    let erased_rule = learned_row(
        "standing_rule",
        "rule-erased",
        1,
        &payload_ref,
        counterparty,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    let kept_rule = learned_row(
        "standing_rule",
        "rule-kept",
        1,
        &other_payload,
        other,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    store.record_learned_artifact(&erased_rule).unwrap();
    store.record_learned_artifact(&kept_rule).unwrap();

    let now = jiff::Timestamp::now();
    let erased_manifest = manifest(
        "rule-erased",
        "connector.enable",
        3600,
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        None,
    );
    let kept_manifest = manifest(
        "rule-kept",
        "timer.schedule",
        3600,
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        None,
    );
    store
        .activate_standing_rule(&erased_manifest, None, now)
        .unwrap();
    store
        .activate_standing_rule(&kept_manifest, None, now)
        .unwrap();

    let erased_action = ActionId::new("connector.enable");
    let kept_action = ActionId::new("timer.schedule");
    let before = consult_standing_rule_gate(&store, &erased_action, now, None).unwrap();
    assert!(before.matched && before.allow, "rule usable before erase");
    assert!(store
        .active_standing_rule_for_action(&erased_action, now)
        .unwrap()
        .is_some());

    let report = erase_counterparty(&store, &artifacts, counterparty).unwrap();
    assert_eq!(report.derived_artifacts_invalidated, 1);
    assert_eq!(
        report.invalidated_identities,
        vec![crate::store::learned_artifacts::LearnedArtifactIdentity {
            kind: "standing_rule".into(),
            artifact_id: "rule-erased".into(),
            version: 1,
        }]
    );

    let after = consult_standing_rule_gate(&store, &erased_action, now, None).unwrap();
    assert!(
        !after.matched && !after.allow,
        "erased-scope standing rule must not match gate consultation"
    );
    assert!(
        store
            .active_standing_rule_for_action(&erased_action, now)
            .unwrap()
            .is_none(),
        "revoked runtime standing_rules row is invisible to active lookup"
    );
    let status: String = store
        .conn
        .lock()
        .query_row(
            "SELECT status FROM standing_rules WHERE artifact_id = ?1 AND version = 1",
            rusqlite::params!["rule-erased"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "revoked");

    // Unrelated scope keeps its activated rule.
    let kept = consult_standing_rule_gate(&store, &kept_action, now, None).unwrap();
    assert!(kept.matched && kept.allow);
    assert!(store
        .active_standing_rule_for_action(&kept_action, now)
        .unwrap()
        .is_some());
}

#[test]
fn active_model_swap_disappears_from_active_ids_after_source_scope_erasure() {
    use crate::store::proposed_artifacts::ProposedArtifact;
    use openspine_schemas::artifact::Lifecycle;

    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();

    let counterparty = Ulid::new();
    let other = Ulid::new();
    let payload_ref = artifacts.put_scoped(counterparty, b"swap source").unwrap();
    let other_payload = artifacts.put_scoped(other, b"other swap source").unwrap();

    let erased_swap = learned_row(
        "model_swap",
        "swap-erased",
        1,
        &payload_ref,
        counterparty,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    let kept_swap = learned_row(
        "model_swap",
        "swap-kept",
        1,
        &other_payload,
        other,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    store.record_learned_artifact(&erased_swap).unwrap();
    store.record_learned_artifact(&kept_swap).unwrap();

    let erased_yaml = artifacts.put(b"erased-swap-yaml").unwrap();
    let other_version_yaml = artifacts.put(b"erased-swap-yaml-v2").unwrap();
    let kept_yaml = artifacts.put(b"kept-swap-yaml").unwrap();
    let erased_proposal = Ulid::new();
    let other_version_proposal = Ulid::new();
    let kept_proposal = Ulid::new();

    // Only v1 is Active for the erased identity — this is the recovery
    // input that must leave active_model_swap_ids after source-scope erase.
    store
        .insert_proposed_artifact(&ProposedArtifact {
            id: erased_proposal,
            kind: "model_swap".to_string(),
            artifact_id: "swap-erased".to_string(),
            version: 1,
            state: Lifecycle::Proposed,
            yaml_digest: erased_yaml.digest.as_str().to_string(),
            task_grant_id: Ulid::new(),
            action_request_id: None,
            proposed_at: jiff::Timestamp::now(),
            lineage: None,
        })
        .unwrap();
    store
        .force_proposed_artifact_state_for_test(erased_proposal, Lifecycle::Active)
        .unwrap();

    // Same artifact_id, different version, Active but not in the invalidated
    // identity set (no matching learned provenance for v2). Exact-identity
    // retirement must leave it Active.
    store
        .insert_proposed_artifact(&ProposedArtifact {
            id: other_version_proposal,
            kind: "model_swap".to_string(),
            artifact_id: "swap-erased".to_string(),
            version: 2,
            state: Lifecycle::Proposed,
            yaml_digest: other_version_yaml.digest.as_str().to_string(),
            task_grant_id: Ulid::new(),
            action_request_id: None,
            proposed_at: jiff::Timestamp::now(),
            lineage: None,
        })
        .unwrap();
    store
        .force_proposed_artifact_state_for_test(other_version_proposal, Lifecycle::Active)
        .unwrap();

    store
        .insert_proposed_artifact(&ProposedArtifact {
            id: kept_proposal,
            kind: "model_swap".to_string(),
            artifact_id: "swap-kept".to_string(),
            version: 1,
            state: Lifecycle::Proposed,
            yaml_digest: kept_yaml.digest.as_str().to_string(),
            task_grant_id: Ulid::new(),
            action_request_id: None,
            proposed_at: jiff::Timestamp::now(),
            lineage: None,
        })
        .unwrap();
    store
        .force_proposed_artifact_state_for_test(kept_proposal, Lifecycle::Active)
        .unwrap();

    // Force the disappearance case: only the invalidated version is Active
    // for swap-erased when we observe active_model_swap_ids. Temporarily
    // demote v2, assert disappearance, then re-check version scoping on
    // a second Active same-id row that is not in matching identities.
    store
        .force_proposed_artifact_state_for_test(other_version_proposal, Lifecycle::Approved)
        .unwrap();

    let before = store.active_model_swap_ids().unwrap();
    assert!(before.contains(&("swap-erased".to_string(), 1)));
    assert!(before.contains(&("swap-kept".to_string(), 1)));

    let report = erase_counterparty(&store, &artifacts, counterparty).unwrap();
    assert_eq!(report.derived_artifacts_invalidated, 1);
    assert_eq!(
        report.invalidated_identities,
        vec![crate::store::learned_artifacts::LearnedArtifactIdentity {
            kind: "model_swap".into(),
            artifact_id: "swap-erased".into(),
            version: 1,
        }]
    );

    let after = store.active_model_swap_ids().unwrap();
    assert!(
        !after.iter().any(|(id, _)| id == "swap-erased"),
        "erased-scope active model_swap must leave active_model_swap_ids"
    );
    assert!(
        after.contains(&("swap-kept".to_string(), 1)),
        "unrelated active model_swap must remain visible to recovery"
    );
    assert_eq!(
        store
            .find_proposed_artifact_state("model_swap", "swap-erased", 1)
            .unwrap()
            .map(|(state, _)| state),
        Some(Lifecycle::Retired)
    );

    // Version-scoped: promote same-id v2 (not in matching identities) and
    // re-run erase. Exact identity matching must not retire v2.
    store
        .force_proposed_artifact_state_for_test(other_version_proposal, Lifecycle::Active)
        .unwrap();
    assert!(store
        .active_model_swap_ids()
        .unwrap()
        .contains(&("swap-erased".to_string(), 2)));
    let retry = erase_counterparty(&store, &artifacts, counterparty).unwrap();
    assert_eq!(retry.derived_artifacts_invalidated, 0);
    assert_eq!(
        store
            .find_proposed_artifact_state("model_swap", "swap-erased", 2)
            .unwrap()
            .map(|(state, _)| state),
        Some(Lifecycle::Active),
        "same artifact_id different version must not be retired"
    );
    assert_eq!(
        store
            .find_proposed_artifact_state("model_swap", "swap-kept", 1)
            .unwrap()
            .map(|(state, _)| state),
        Some(Lifecycle::Active)
    );
}

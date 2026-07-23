// openspine:allow-large-module reason: cohesive learned-artifact migration, terminal-state, provenance, and closure-trigger matrix shares one store fixture.
//! Tests for the learned overlay artifact store (AD-023/070/071), split out
//! of `learned_artifacts.rs` to keep that module under the 500-line gate.

use super::*;
use openspine_schemas::digest::digest_of_bytes;

fn sample() -> LearnedArtifact {
    LearnedArtifact {
        kind: "route".into(),
        artifact_id: "learned-route".into(),
        version: 1,
        namespace: ArtifactNamespace::Overlay,
        provenance: Provenance::ProducedBy {
            source_event_id: Ulid::new(),
            source_exchange: ArtifactRef {
                digest: digest_of_bytes(b"exchange"),
                schema_version: 1,
            },
            source_scope: Ulid::new(),
        },
        accepted_via: None,
        learned_at: Timestamp::now(),
        compatibility: CompatibilityStatus::Compatible,
        nomination: NominationStatus::None,
        pending_reconfirmation_id: None,
        pending_yaml_digest: None,
        accepted_dependency_fingerprint: None,
        source_path: None,
        accepted_base_epoch: None,
    }
}

#[test]
fn learned_artifact_round_trip_preserves_typed_provenance() {
    let store = Store::open_in_memory().unwrap();
    let artifact = sample();
    store.record_learned_artifact(&artifact).unwrap();
    assert_eq!(store.list_learned_artifacts().unwrap(), vec![artifact]);
}

#[test]
fn base_namespace_cannot_be_recorded_as_learned() {
    let store = Store::open_in_memory().unwrap();
    let mut artifact = sample();
    artifact.namespace = ArtifactNamespace::Base;
    assert!(store.record_learned_artifact(&artifact).is_err());
}

#[test]
fn owner_accepted_is_durable_across_round_trip() {
    let store = Store::open_in_memory().unwrap();
    let mut artifact = sample();
    artifact.compatibility = CompatibilityStatus::OwnerAccepted;
    store.record_learned_artifact(&artifact).unwrap();
    assert_eq!(
        store.list_learned_artifacts().unwrap()[0].compatibility,
        CompatibilityStatus::OwnerAccepted
    );
}

#[test]
fn accepted_dependency_fingerprint_round_trips() {
    // The canonical non-empty accepted-dangling anchor must survive a
    // record/list round-trip (catches a dropped INSERT/SELECT/migration field).
    let store = Store::open_in_memory().unwrap();
    let mut artifact = sample();
    artifact.compatibility = CompatibilityStatus::OwnerAccepted;
    artifact.accepted_dependency_fingerprint = Some(dependency_fingerprint(&[
        "agent:removed".to_string(),
        "workflow:gone".to_string(),
    ]));
    store.record_learned_artifact(&artifact).unwrap();
    let stored = store.list_learned_artifacts().unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(
        stored[0].accepted_dependency_fingerprint,
        artifact.accepted_dependency_fingerprint
    );
}

#[test]
fn accepted_dependency_fingerprint_migration_is_idempotent() {
    // A pre-existing DB whose learned_artifacts table predates the
    // accepted_dependency_fingerprint column must gain it via the idempotent
    // migration, persist a recorded anchor, and reopen without error.
    let dir = std::env::temp_dir().join(format!("overlay_mig_{}", Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("kernel.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE learned_artifacts (
               kind TEXT NOT NULL, artifact_id TEXT NOT NULL, version INTEGER NOT NULL,
               namespace TEXT NOT NULL DEFAULT 'overlay', provenance TEXT NOT NULL,
               accepted_via TEXT, learned_at TEXT NOT NULL,
               compatibility TEXT NOT NULL DEFAULT 'compatible',
               nomination TEXT NOT NULL DEFAULT 'none', pending_reconfirmation_id TEXT,
               pending_yaml_digest TEXT, source_path TEXT, accepted_base_epoch TEXT,
               PRIMARY KEY(kind, artifact_id, version));",
        )
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    let mut artifact = sample();
    artifact.compatibility = CompatibilityStatus::OwnerAccepted;
    artifact.accepted_dependency_fingerprint =
        Some(dependency_fingerprint(&["agent:removed".to_string()]));
    store.record_learned_artifact(&artifact).unwrap();
    assert_eq!(
        store.list_learned_artifacts().unwrap()[0].accepted_dependency_fingerprint,
        artifact.accepted_dependency_fingerprint
    );
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.list_learned_artifacts().unwrap()[0].accepted_dependency_fingerprint,
        artifact.accepted_dependency_fingerprint
    );
}
#[test]
fn typed_provenance_missing_source_scope_migration_is_idempotent() {
    let dir = std::env::temp_dir().join(format!("overlay_mig_typed_{}", Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("kernel.db");
    let event_id = Ulid::new();
    let exchange_digest = digest_of_bytes(b"exchange_legacy");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE learned_artifacts (
               kind TEXT NOT NULL, artifact_id TEXT NOT NULL, version INTEGER NOT NULL,
               namespace TEXT NOT NULL DEFAULT 'overlay', provenance TEXT NOT NULL,
               accepted_via TEXT, learned_at TEXT NOT NULL,
               compatibility TEXT NOT NULL DEFAULT 'compatible',
               nomination TEXT NOT NULL DEFAULT 'none', pending_reconfirmation_id TEXT,
               pending_yaml_digest TEXT, accepted_dependency_fingerprint TEXT,
               source_path TEXT, accepted_base_epoch TEXT,
               PRIMARY KEY(kind, artifact_id, version));",
        )
        .unwrap();
        // Insert a current-main typed ProducedBy row missing source_scope
        let provenance_json = serde_json::json!({
            "produced_by": {
                "source_event_id": event_id.to_string(),
                "source_exchange": {
                    "digest": exchange_digest.to_string(),
                    "schema_version": 1
                }
            }
        })
        .to_string();
        conn.execute(
            "INSERT INTO learned_artifacts
               (kind, artifact_id, version, namespace, provenance, learned_at, compatibility)
             VALUES ('route', 'r_legacy', 1, 'overlay', ?1, ?2, 'compatible')",
            rusqlite::params![provenance_json, Timestamp::now().to_string()],
        )
        .unwrap();
    }

    let store = Store::open(&path).unwrap();
    let artifacts = store.list_learned_artifacts().unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(
        artifacts[0].provenance,
        Provenance::ProducedBy {
            source_event_id: event_id,
            source_exchange: ArtifactRef {
                digest: exchange_digest.clone(),
                schema_version: 1,
            },
            source_scope: crate::counterparty_keys::SYSTEM_SCOPE,
        }
    );

    // Reopening is idempotent
    drop(store);
    let store = Store::open(&path).unwrap();
    let artifacts = store.list_learned_artifacts().unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(
        artifacts[0].provenance,
        Provenance::ProducedBy {
            source_event_id: event_id,
            source_exchange: ArtifactRef {
                digest: exchange_digest.clone(),
                schema_version: 1,
            },
            source_scope: crate::counterparty_keys::SYSTEM_SCOPE,
        }
    );
}

#[test]
fn record_post_closure_learned_artifact_is_atomically_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts =
        crate::artifact_store::ArtifactStore::open(dir.path().join("artifacts"), [7u8; 32])
            .unwrap();

    let counterparty = Ulid::new();
    store
        .mark_learned_artifacts_erased(counterparty, &artifacts)
        .unwrap();

    let mut new_artifact = sample();
    new_artifact.provenance = Provenance::ProducedBy {
        source_event_id: Ulid::new(),
        source_exchange: ArtifactRef {
            digest: digest_of_bytes(b"post_closure"),
            schema_version: 1,
        },
        source_scope: counterparty,
    };

    let res = store.record_learned_artifact(&new_artifact);
    assert!(res.is_err());
}

#[test]
fn mark_reconfirmation_required_excludes_erased_rows() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts =
        crate::artifact_store::ArtifactStore::open(dir.path().join("artifacts"), [7u8; 32])
            .unwrap();

    let counterparty = Ulid::new();
    let mut artifact = sample();
    artifact.provenance = Provenance::ProducedBy {
        source_event_id: Ulid::new(),
        source_exchange: ArtifactRef {
            digest: digest_of_bytes(b"ex"),
            schema_version: 1,
        },
        source_scope: counterparty,
    };
    store.record_learned_artifact(&artifact).unwrap();

    store
        .mark_learned_artifacts_erased(counterparty, &artifacts)
        .unwrap();

    let res = store.mark_reconfirmation_required(
        &artifact.kind,
        &artifact.artifact_id,
        artifact.version,
        Ulid::new(),
        "digest",
    );
    assert!(res.is_err());
}

#[test]
fn table_rebuild_reinstalls_closure_triggers() {
    // Pre-typed provenance rebuild renames/drops the old table. SQLite moves
    // triggers with the rename and drops them with the old table, so ensure
    // the rebuild reinstalls functional closure enforcement.
    let dir = std::env::temp_dir().join(format!("overlay_mig_rebuild_{}", Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("kernel.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE learned_artifacts (
               kind TEXT NOT NULL, artifact_id TEXT NOT NULL, version INTEGER NOT NULL,
               namespace TEXT NOT NULL DEFAULT 'overlay',
               source_event_id TEXT NOT NULL,
               source_exchange_digest TEXT NOT NULL,
               source_exchange_schema_version INTEGER NOT NULL,
               accepted_via TEXT, learned_at TEXT NOT NULL,
               compatibility TEXT NOT NULL DEFAULT 'compatible',
               nomination TEXT NOT NULL DEFAULT 'none', pending_reconfirmation_id TEXT,
               pending_yaml_digest TEXT,
               PRIMARY KEY(kind, artifact_id, version));
             INSERT INTO learned_artifacts
               (kind, artifact_id, version, namespace, source_event_id,
                source_exchange_digest, source_exchange_schema_version,
                learned_at, compatibility)
             VALUES ('route', 'r_pretyped', 1, 'overlay', '01HX0000000000000000000000',
                     '0000000000000000000000000000000000000000000000000000000000000000',
                     1, '2026-01-01T00:00:00Z', 'compatible');",
        )
        .unwrap();
    }

    let store = Store::open(&path).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let artifacts =
        crate::artifact_store::ArtifactStore::open(dir.path().join("artifacts"), [9u8; 32])
            .unwrap();
    let counterparty = Ulid::new();
    store
        .mark_learned_artifacts_erased(counterparty, &artifacts)
        .unwrap();

    let mut blocked = sample();
    blocked.provenance = Provenance::ProducedBy {
        source_event_id: Ulid::new(),
        source_exchange: ArtifactRef {
            digest: digest_of_bytes(b"post_rebuild"),
            schema_version: 1,
        },
        source_scope: counterparty,
    };
    assert!(
        store.record_learned_artifact(&blocked).is_err(),
        "closure triggers must reject inserts after table rebuild"
    );
}

#[test]
fn fresh_store_marker_reconciliation_exposes_erased_ids() {
    // Fresh stores have no historical markers. After durable erasure, the
    // marker list is what startup uses to re-drive key cleanup.
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let master = [3u8; 32];
    let artifacts_path = dir.path().join("artifacts");
    {
        let artifacts =
            crate::artifact_store::ArtifactStore::open(artifacts_path.clone(), master).unwrap();
        assert!(store.erased_counterparty_ids().unwrap().is_empty());

        let a = Ulid::new();
        let b = Ulid::new();
        // Create keys so there is something for a later process to clean up.
        artifacts.put_scoped(a, b"a-payload").unwrap();
        artifacts.put_scoped(b, b"b-payload").unwrap();
        store.mark_learned_artifacts_erased(a, &artifacts).unwrap();
        store.mark_learned_artifacts_erased(b, &artifacts).unwrap();
    }

    let mut ids = store.erased_counterparty_ids().unwrap();
    ids.sort();
    assert_eq!(ids.len(), 2);

    // Fresh process: in-memory closed set is empty. Startup must re-drive
    // filesystem cleanup from durable markers alone.
    let artifacts = crate::artifact_store::ArtifactStore::open(artifacts_path, master).unwrap();
    for counterparty_id in &ids {
        // Without replay, tombstones may already exist from the first pass;
        // erase_counterparty_key is still the startup reconciliation call.
        artifacts.erase_counterparty_key(*counterparty_id).unwrap();
        assert!(
            artifacts
                .put_scoped(*counterparty_id, b"post-reconcile")
                .is_err(),
            "startup marker replay must keep the scope closed"
        );
    }
}

#[test]
fn activation_cannot_replace_erased_identity_with_other_scope() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts =
        crate::artifact_store::ArtifactStore::open(dir.path().join("artifacts"), [5u8; 32])
            .unwrap();

    let erased_scope = Ulid::new();
    let mut artifact = sample();
    artifact.provenance = Provenance::ProducedBy {
        source_event_id: Ulid::new(),
        source_exchange: ArtifactRef {
            digest: digest_of_bytes(b"activation_erased"),
            schema_version: 1,
        },
        source_scope: erased_scope,
    };
    store.record_learned_artifact(&artifact).unwrap();
    store
        .mark_learned_artifacts_erased(erased_scope, &artifacts)
        .unwrap();

    let proposal_id = Ulid::new();
    {
        let conn = store.conn.lock();
        conn.execute(
            "INSERT INTO proposed_artifacts
               (id, kind, artifact_id, version, state, yaml_digest, task_grant_id, proposed_at)
             VALUES (?1, ?2, ?3, ?4, 'approved', 'digest', ?5, ?6)",
            rusqlite::params![
                proposal_id.to_string(),
                artifact.kind,
                artifact.artifact_id,
                artifact.version as i64,
                Ulid::new().to_string(),
                Timestamp::now().to_string(),
            ],
        )
        .unwrap();
    }

    let other_scope = Ulid::new();
    let mut replacement = artifact.clone();
    replacement.compatibility = CompatibilityStatus::OwnerAccepted;
    replacement.provenance = Provenance::ProducedBy {
        source_event_id: Ulid::new(),
        source_exchange: ArtifactRef {
            digest: digest_of_bytes(b"activation_other"),
            schema_version: 1,
        },
        source_scope: other_scope,
    };

    let err = store
        .commit_artifact_activation(crate::store::activation::ActivationCommit {
            learned: replacement,
            proposed_id: proposal_id,
            grant_id: None,
            payload_ref: None,
            dangling: true,
            superseded_old_version: None,
            standing_rule: None,
        })
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("cannot replace an erased learned artifact identity"),
        "unexpected error: {err}"
    );
    assert_eq!(
        store.list_learned_artifacts().unwrap()[0].compatibility,
        CompatibilityStatus::Erased
    );
}

#[test]
fn owner_reconfirmation_cannot_replace_erased_identity_with_other_scope() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts =
        crate::artifact_store::ArtifactStore::open(dir.path().join("artifacts"), [6u8; 32])
            .unwrap();

    let erased_scope = Ulid::new();
    let mut artifact = sample();
    artifact.provenance = Provenance::ProducedBy {
        source_event_id: Ulid::new(),
        source_exchange: ArtifactRef {
            digest: digest_of_bytes(b"reconfirm_erased"),
            schema_version: 1,
        },
        source_scope: erased_scope,
    };
    store.record_learned_artifact(&artifact).unwrap();
    store
        .mark_learned_artifacts_erased(erased_scope, &artifacts)
        .unwrap();

    let request_id = Ulid::new();
    {
        let conn = store.conn.lock();
        conn.execute(
            "INSERT INTO action_requests (id, request_json, used) VALUES (?1, '{}', 0)",
            rusqlite::params![request_id.to_string()],
        )
        .unwrap();
    }

    let other_scope = Ulid::new();
    let err = store
        .commit_owner_reconfirmation(crate::store::learned_reconfirmation::OwnerReconfirmation {
            kind: artifact.kind.clone(),
            artifact_id: artifact.artifact_id.clone(),
            version: artifact.version,
            provenance: Provenance::ProducedBy {
                source_event_id: Ulid::new(),
                source_exchange: ArtifactRef {
                    digest: digest_of_bytes(b"reconfirm_other"),
                    schema_version: 1,
                },
                source_scope: other_scope,
            },
            accepted_via: None,
            base_epoch: "epoch".into(),
            accepted_dependency_fingerprint: None,
            request_id,
            grant_id: None,
            review_ref: None,
            proposal_id: None,
            new_proposal: None,
            dangling_refs: vec![],
            superseded_old_version: None,
        })
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("cannot replace an erased learned artifact identity"),
        "unexpected error: {err}"
    );
    assert_eq!(
        store.list_learned_artifacts().unwrap()[0].compatibility,
        CompatibilityStatus::Erased
    );
}

#[test]
fn quarantine_leaves_erased_identity_untouched() {
    let store = Store::open_in_memory().unwrap();
    let mut artifact = sample();
    artifact.compatibility = CompatibilityStatus::Erased;
    store.record_learned_artifact(&artifact).unwrap();

    assert!(!store
        .quarantine_learned_artifact(
            &artifact.kind,
            &artifact.artifact_id,
            artifact.version,
            "must not remove terminal erasure",
        )
        .unwrap());

    let retained = store.list_learned_artifacts().unwrap();
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].compatibility, CompatibilityStatus::Erased);
}

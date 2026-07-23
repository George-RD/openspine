//! Tests for the Donna×Leo personality seed bootstrap (AD-080/081/082/083).

use super::*;
use openspine_schemas::digest::digest_of_bytes;
use tempfile::tempdir;

#[test]
fn seed_definitions_are_nine_unique_active_elements() {
    let defs = seed_definitions();
    assert_eq!(defs.len(), 9);
    let mut ids = std::collections::HashSet::new();
    for element in &defs {
        assert_eq!(element.version, 1);
        assert_eq!(element.lifecycle_state, Lifecycle::Active);
        assert!(!element.guidance.is_empty());
        assert!(ids.insert(element.id.clone()), "duplicate persona id");
    }
}

#[test]
fn seed_guidance_keeps_negative_constraints_in_probes_only() {
    let forbidden_markers = [
        "ad-081",
        "ad-083",
        "deferential",
        "double-asking",
        "sycophan",
        "over-explain",
        "info-dump",
        "self-promotional",
        "psychic",
        "faked intimacy",
        "apology theater",
        "presumptuous",
        "need-to-know failure",
        "nagging",
    ];
    for element in seed_definitions() {
        let guidance = element.guidance.to_lowercase();
        for marker in forbidden_markers {
            assert!(
                !guidance.contains(marker),
                "{} guidance leaked probe-only marker {marker:?}: {}",
                element.id,
                element.guidance
            );
        }
        assert!(
            crate::overlay_eval_gate::personality_probes::run_probes(&element.guidance).is_empty(),
            "{} guidance must be behaviorally clean",
            element.id
        );
    }
}

#[test]
fn seed_if_missing_loads_overlay_artifacts_with_real_traceable_provenance() {
    let tmp = tempdir().unwrap();
    let overlay_dir = tmp.path().join("artifacts.d");
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(tmp.path().join("artifacts"), [7u8; 32]).unwrap();

    seed_if_missing(&store, &artifacts, &overlay_dir).unwrap();
    seed_if_missing(&store, &artifacts, &overlay_dir).unwrap();

    let admitted = store
        .list_learned_artifacts()
        .unwrap()
        .into_iter()
        .filter_map(|row| {
            (row.kind == "persona").then(|| {
                (
                    (row.artifact_id, row.version),
                    row.pending_yaml_digest.unwrap(),
                )
            })
        })
        .collect();
    let mut registry = artifact_loader::load_registry(&overlay_dir).unwrap();
    artifact_loader::load_admitted_personas(&mut registry, &overlay_dir, &admitted).unwrap();
    assert_eq!(registry.personas.len(), 9);
    assert!(registry.personas.contains_key("anticipatory_provisioning"));
    assert!(registry.personas.contains_key("digest_brief_default"));
    assert_eq!(
        store
            .count_audit_events_of_kind("personality_seed.bootstrap")
            .unwrap(),
        1
    );
    let bootstrap_event_id = store
        .all_audit_event_jsons()
        .unwrap()
        .iter()
        .filter_map(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .find(|event| event["kind"] == "personality_seed.bootstrap")
        .and_then(|event| event["id"].as_str().map(str::to_string))
        .expect("bootstrap audit event must exist");

    let learned = store.list_learned_artifacts().unwrap();
    let persona_rows: Vec<_> = learned
        .iter()
        .filter(|item| item.kind == "persona")
        .collect();
    assert_eq!(persona_rows.len(), 9);
    for row in persona_rows {
        assert_eq!(row.namespace, ArtifactNamespace::Overlay);
        assert_eq!(row.compatibility, CompatibilityStatus::Compatible);
        match &row.provenance {
            Provenance::ProducedBy {
                source_event_id,
                source_exchange,
                source_scope,
            } => {
                assert_eq!(source_event_id.to_string(), bootstrap_event_id);
                assert!(!source_exchange.digest.to_string().is_empty());
                assert_eq!(*source_scope, SYSTEM_SCOPE);
                assert_eq!(
                    artifacts
                        .get_scoped(*source_scope, source_exchange)
                        .unwrap(),
                    b"openspine personality seed bootstrap: kernel-authored exchange \
establishing ProducedBy provenance for the pre-populated Donna x Leo persona \
overlay artifacts (AD-080). Not a human conversation; a traceable bootstrap event."
                );
            }
            Provenance::LegacyMigration { .. } => {
                panic!("persona seed must use ProducedBy provenance")
            }
        }
    }
    assert_eq!(
        store
            .list_learned_artifacts()
            .unwrap()
            .iter()
            .filter(|item| item.kind == "persona")
            .count(),
        9
    );
}

#[test]
fn seed_survives_fresh_store_restart_without_duplicate_rows() {
    let tmp = tempdir().unwrap();
    let overlay_dir = tmp.path().join("artifacts.d");
    let db_path = tmp.path().join("kernel.db");
    let artifacts = ArtifactStore::open(tmp.path().join("artifacts"), [9u8; 32]).unwrap();
    {
        let store = Store::open(&db_path).unwrap();
        seed_if_missing(&store, &artifacts, &overlay_dir).unwrap();
    }
    let restarted = Store::open(&db_path).unwrap();
    seed_if_missing(&restarted, &artifacts, &overlay_dir).unwrap();
    let startup = crate::overlay_startup::load(
        &crate::test_support::fixtures::repo_lyra_dir(),
        tmp.path(),
        &restarted,
        &artifacts,
    )
    .unwrap();
    assert_eq!(startup.registry.personas.len(), 9);
    assert_eq!(
        std::fs::read_dir(overlay_dir.join("personas"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "yaml"))
            .count(),
        9
    );
}

#[test]
fn durable_persona_file_without_row_is_reconciled_on_retry() {
    let tmp = tempdir().unwrap();
    let overlay_dir = tmp.path().join("artifacts.d");
    let persona_dir = overlay_dir.join("personas");
    std::fs::create_dir_all(&persona_dir).unwrap();
    let element = seed_definitions().into_iter().next().unwrap();
    let path = persona_dir.join(artifact_loader::overlay_filename(
        &element.id,
        element.version,
    ));
    std::fs::write(&path, serde_yaml::to_string(&element).unwrap()).unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(tmp.path().join("artifacts"), [8u8; 32]).unwrap();

    seed_if_missing(&store, &artifacts, &overlay_dir).unwrap();

    assert_eq!(
        store
            .list_learned_artifacts()
            .unwrap()
            .iter()
            .filter(|row| row.kind == "persona" && row.artifact_id == element.id)
            .count(),
        1
    );
    assert!(path.is_file());
}

#[test]
fn digest_brief_default_is_a_learnable_presentation_only_guidance() {
    let tmp = tempdir().unwrap();
    let overlay_dir = tmp.path().join("artifacts.d");
    let persona_dir = overlay_dir.join("personas");
    std::fs::create_dir_all(&persona_dir).unwrap();
    let mut digest = seed_definitions()
        .into_iter()
        .find(|element| element.id == "digest_brief_default")
        .unwrap();
    digest.guidance = "Use the owner's preferred concise digest shape.".to_string();
    let bytes = serde_yaml::to_string(&digest).unwrap().into_bytes();
    let path = persona_dir.join(artifact_loader::overlay_filename(
        &digest.id,
        digest.version,
    ));
    std::fs::write(&path, &bytes).unwrap();
    let mut registry = artifact_loader::ArtifactRegistry::default();
    let admitted = std::collections::HashMap::from([(
        (digest.id.clone(), digest.version),
        digest_of_bytes(&bytes).to_string(),
    )]);
    artifact_loader::load_admitted_personas(&mut registry, &overlay_dir, &admitted).unwrap();
    let loaded = registry.personas.get("digest_brief_default").unwrap();
    assert_eq!(loaded.guidance, digest.guidance);
    assert_eq!(loaded.lifecycle_state, Lifecycle::Active);
}
#[test]
fn a_committed_row_with_a_missing_file_self_heals_without_new_provenance() {
    let tmp = tempdir().unwrap();
    let overlay_dir = tmp.path().join("artifacts.d");
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(tmp.path().join("artifacts"), [7u8; 32]).unwrap();

    seed_if_missing(&store, &artifacts, &overlay_dir).unwrap();
    let before = store.list_learned_artifacts().unwrap();
    let before_provenance = before
        .iter()
        .find(|item| item.kind == "persona" && item.artifact_id == "bounded_autonomy")
        .unwrap()
        .provenance
        .clone();
    let expected_path = overlay_dir
        .join("personas")
        .join(artifact_loader::overlay_filename("bounded_autonomy", 1));
    std::fs::remove_file(&expected_path).unwrap();
    let bootstrap_events_before = store
        .count_audit_events_of_kind("personality_seed.bootstrap")
        .unwrap();

    seed_if_missing(&store, &artifacts, &overlay_dir).unwrap();

    assert!(expected_path.is_file(), "repair must recreate missing file");
    let repaired: PersonaElement =
        serde_yaml::from_slice(&std::fs::read(&expected_path).unwrap()).unwrap();
    assert_eq!(repaired.id, "bounded_autonomy");
    let after = store.list_learned_artifacts().unwrap();
    let persona_rows: Vec<_> = after.iter().filter(|item| item.kind == "persona").collect();
    assert_eq!(persona_rows.len(), 9);
    assert_eq!(
        persona_rows
            .iter()
            .find(|item| item.artifact_id == "bounded_autonomy")
            .unwrap()
            .provenance,
        before_provenance
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("personality_seed.bootstrap")
            .unwrap(),
        bootstrap_events_before
    );
}

#[test]
fn a_corrupt_existing_file_is_repaired_to_the_row_digest() {
    let tmp = tempdir().unwrap();
    let overlay_dir = tmp.path().join("artifacts.d");
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(tmp.path().join("artifacts"), [7u8; 32]).unwrap();

    seed_if_missing(&store, &artifacts, &overlay_dir).unwrap();
    let path = overlay_dir
        .join("personas")
        .join(artifact_loader::overlay_filename("bounded_autonomy", 1));
    std::fs::write(&path, b"corrupt bytes").unwrap();
    seed_if_missing(&store, &artifacts, &overlay_dir).unwrap();

    let row = store
        .list_learned_artifacts()
        .unwrap()
        .into_iter()
        .find(|item| item.kind == "persona" && item.artifact_id == "bounded_autonomy")
        .unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(
        Some(digest_of_bytes(&bytes).to_string()),
        row.pending_yaml_digest
    );
}

#[test]
fn dangling_seed_row_is_quarantined_and_reseeded() {
    let tmp = tempdir().unwrap();
    let overlay_dir = tmp.path().join("artifacts.d");
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(tmp.path().join("artifacts"), [7u8; 32]).unwrap();
    let element = seed_definitions()
        .into_iter()
        .find(|element| element.id == "bounded_autonomy")
        .unwrap();
    let canonical_yaml = serde_yaml::to_string(&element).unwrap();
    let exchange_ref = artifacts.put(b"valid exchange").unwrap();
    let row = LearnedArtifact {
        kind: "persona".to_string(),
        artifact_id: element.id.clone(),
        version: element.version,
        namespace: ArtifactNamespace::Overlay,
        provenance: Provenance::ProducedBy {
            source_event_id: Ulid::new(),
            source_exchange: exchange_ref,
            source_scope: SYSTEM_SCOPE,
        },
        accepted_via: None,
        learned_at: Timestamp::now(),
        compatibility: CompatibilityStatus::Compatible,
        nomination: NominationStatus::None,
        pending_reconfirmation_id: None,
        pending_yaml_digest: Some(digest_of_bytes(canonical_yaml.as_bytes()).to_string()),
        source_path: None,
        accepted_base_epoch: None,
        accepted_dependency_fingerprint: None,
    };
    store.record_learned_artifact(&row).unwrap();

    seed_if_missing(&store, &artifacts, &overlay_dir).unwrap();

    assert!(overlay_dir
        .join("personas")
        .join(artifact_loader::overlay_filename(
            &element.id,
            element.version
        ))
        .is_file());
    assert_eq!(
        store
            .count_audit_events_of_kind("artifact.persona_quarantined")
            .unwrap(),
        1
    );
}
#[test]
fn audit_failure_rolls_back_learned_row_and_seeded_receipt() {
    let tmp = tempdir().unwrap();
    let overlay_dir = tmp.path().join("artifacts.d");
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(tmp.path().join("artifacts"), [7u8; 32]).unwrap();
    store
        .install_audit_append_failure_for_kind("personality_seed.seeded")
        .unwrap();

    let err = seed_if_missing(&store, &artifacts, &overlay_dir).unwrap_err();
    assert!(err
        .to_string()
        .contains("recording persona seed provenance"));
    assert_eq!(
        store
            .list_learned_artifacts()
            .unwrap()
            .iter()
            .filter(|item| item.kind == "persona")
            .count(),
        0
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("personality_seed.seeded")
            .unwrap(),
        0
    );
    // The file was already durably published before the transaction; retry
    // can safely reuse it because the DB row was rolled back.
    assert!(overlay_dir.join("personas").is_dir());
}

#[test]
fn erased_seed_identity_is_not_quarantined_or_reseeded() {
    let tmp = tempdir().unwrap();
    let overlay_dir = tmp.path().join("artifacts.d");
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(tmp.path().join("artifacts"), [17u8; 32]).unwrap();
    let element = seed_definitions().into_iter().next().unwrap();
    let source_scope = Ulid::new();
    let source_exchange = artifacts
        .put_scoped(source_scope, b"erased persona source")
        .unwrap();
    let row = LearnedArtifact {
        kind: "persona".to_string(),
        artifact_id: element.id.clone(),
        version: element.version,
        namespace: ArtifactNamespace::Overlay,
        provenance: Provenance::ProducedBy {
            source_event_id: Ulid::new(),
            source_exchange,
            source_scope,
        },
        accepted_via: None,
        learned_at: Timestamp::now(),
        compatibility: CompatibilityStatus::Erased,
        nomination: NominationStatus::None,
        pending_reconfirmation_id: None,
        pending_yaml_digest: Some(
            digest_of_bytes(serde_yaml::to_string(&element).unwrap().as_bytes()).to_string(),
        ),
        accepted_dependency_fingerprint: None,
        source_path: None,
        accepted_base_epoch: None,
    };
    store.record_learned_artifact(&row).unwrap();

    seed_if_missing(&store, &artifacts, &overlay_dir).unwrap();

    let retained = store
        .list_learned_artifacts()
        .unwrap()
        .into_iter()
        .find(|item| item.kind == "persona" && item.artifact_id == element.id)
        .unwrap();
    assert_eq!(retained.compatibility, CompatibilityStatus::Erased);
    let path = persona_overlay_dir(&overlay_dir).join(artifact_loader::overlay_filename(
        &element.id,
        element.version,
    ));
    assert!(!path.exists(), "terminal erased seed must not be repaired");
}

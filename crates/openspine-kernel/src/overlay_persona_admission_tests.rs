use super::*;
use crate::store::learned_artifacts::{origin_from_producing_scope, NominationStatus};
use jiff::Timestamp;
use openspine_schemas::artifact::ArtifactNamespace;
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::provenance::ProvenanceOrigin;
use ulid::Ulid;

fn learned(id: &str, version: u32) -> LearnedArtifact {
    let at: Timestamp = "2026-01-01T00:00:00Z".parse().unwrap();
    LearnedArtifact {
        kind: "persona".into(),
        artifact_id: id.into(),
        version,
        namespace: ArtifactNamespace::Overlay,
        provenance: Provenance::LegacyMigration { discovered_at: at },
        accepted_via: None,
        learned_at: at,
        compatibility: CompatibilityStatus::Compatible,
        nomination: NominationStatus::None,
        pending_reconfirmation_id: None,
        pending_yaml_digest: None,
        accepted_dependency_fingerprint: None,
        source_path: None,
        accepted_base_epoch: None,
    }
}

fn persona_yaml(id: &str, version: u32) -> String {
    format!(
        "id: {id}\nschema_version: 1\nversion: {version}\nlifecycle_state: active\nguidance: Be practical.\n"
    )
}

struct Fixture {
    root: tempfile::TempDir,
    store: Store,
    artifacts: ArtifactStore,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let artifacts = ArtifactStore::open(root.path().join("objects"), [3; 32]).unwrap();
        Self {
            root,
            store: Store::open_in_memory().unwrap(),
            artifacts,
        }
    }

    fn produced(&self, id: &str, version: u32) -> LearnedArtifact {
        let exchange = self.artifacts.put(b"private producing exchange").unwrap();
        let event = self
            .store
            .append_audit(
                "artifact.superseded",
                None,
                None,
                None,
                None,
                &[],
                std::slice::from_ref(&exchange),
            )
            .unwrap();
        let mut row = learned(id, version);
        row.provenance = Provenance::ProducedBy {
            source_event_id: event.id,
            source_exchange: exchange,
            source_scope: ProvenanceOrigin::system(),
        };
        row.pending_yaml_digest = Some(digest_of_bytes(persona_yaml(id, version).as_bytes()).to_string());
        row
    }

    fn capture(&self, rows: &[LearnedArtifact]) -> anyhow::Result<CapturedPersonaProvenance> {
        CapturedPersonaProvenance::capture_for_startup(&self.store, &self.artifacts, rows)
    }

    fn findings(&self, rows: &[LearnedArtifact]) -> PersonaProvenanceFindings {
        self.capture(rows).unwrap().evaluate()
    }

    fn admit(&self, rows: &[LearnedArtifact]) -> anyhow::Result<ArtifactRegistry> {
        let mut registry = ArtifactRegistry::default();
        admit(
            &self.store,
            &self.artifacts,
            &self.root.path().join("overlay"),
            rows,
            &mut registry,
        )?;
        Ok(registry)
    }
}

fn admit_rows(rows: &[LearnedArtifact]) -> anyhow::Result<()> {
    Fixture::new().admit(rows).map(|_| ())
}

fn assert_excluded(fixture: &Fixture, row: LearnedArtifact, reason: PersonaProvenanceExclusion) {
    let key = (row.artifact_id.clone(), row.version);
    let findings = fixture.findings(&[row]);
    assert!(findings.expected_digests.is_empty());
    assert_eq!(findings.excluded, BTreeMap::from([(key, reason)]));
}

#[test]
fn persona_admission_rejects_duplicate_exact_version_provenance() {
    let row = learned("persona", 1);
    let result = admit_rows(&[row.clone(), row]);
    assert!(
        result.is_err(),
        "duplicate evidence must not be silently collapsed"
    );
}

#[test]
fn persona_admission_rejects_conflicting_exact_version_provenance() {
    let row = learned("persona", 1);
    let mut erased = row.clone();
    erased.compatibility = CompatibilityStatus::Erased;
    for rows in [[row.clone(), erased.clone()], [erased, row]] {
        assert!(
            admit_rows(&rows).is_err(),
            "ambiguous erasure evidence must be refused in either order"
        );
    }
}

#[test]
fn persona_admission_keeps_distinct_versions_and_kinds_separate() {
    let row = learned("persona", 1);
    let mut route = row.clone();
    route.kind = "route".into();
    assert!(admit_rows(&[row, learned("persona", 2), route]).is_ok());
}

#[test]
fn captured_persona_provenance_preserves_exact_version_digests() {
    let fixture = Fixture::new();
    let v1 = fixture.produced("persona", 1);
    let v2 = fixture.produced("persona", 2);
    let expected = BTreeMap::from([
        (("persona".into(), 1), v1.pending_yaml_digest.clone().unwrap()),
        (("persona".into(), 2), v2.pending_yaml_digest.clone().unwrap()),
    ]);
    let findings = fixture.findings(&[v1, v2]);
    assert_eq!(findings.expected_digests, expected);
    assert!(findings.excluded.is_empty());
}

#[test]
fn captured_persona_provenance_findings_are_canonical_across_row_order() {
    let fixture = Fixture::new();
    let mut rows = vec![
        fixture.produced("z", 2),
        learned("b", 1),
        fixture.produced("a", 3),
        learned("a", 2),
    ];
    let expected = fixture.findings(&rows);
    rows.reverse();
    assert_eq!(fixture.findings(&rows), expected);
    assert_eq!(
        expected.excluded.keys().cloned().collect::<Vec<_>>(),
        vec![("a".into(), 2), ("b".into(), 1)]
    );
}

#[test]
fn captured_persona_provenance_replays_after_inputs_and_sources_disappear() {
    let fixture = Fixture::new();
    let mut row = fixture.produced("persona", 1);
    let capture = fixture.capture(std::slice::from_ref(&row)).unwrap();
    let expected = capture.evaluate();
    row.compatibility = CompatibilityStatus::Erased;
    row.pending_yaml_digest = None;
    assert_excluded(&fixture, row, PersonaProvenanceExclusion::Erased);
    fixture.store.break_audit_for_test();
    drop(fixture);
    assert_eq!(capture.evaluate(), expected);
    assert_eq!(capture.evaluate(), expected);
}

#[test]
fn captured_persona_provenance_does_not_waive_legacy_for_owner_acceptance() {
    let fixture = Fixture::new();
    let mut row = learned("persona", 1);
    row.compatibility = CompatibilityStatus::OwnerAccepted;
    row.pending_yaml_digest = Some(digest_of_bytes(b"approved").to_string());
    fixture.store.break_audit_for_test();
    assert_excluded(&fixture, row, PersonaProvenanceExclusion::NotProducedBy);
}

#[test]
fn captured_persona_provenance_erasure_precedes_unavailable_producing_event() {
    let fixture = Fixture::new();
    let mut row = fixture.produced("persona", 1);
    row.compatibility = CompatibilityStatus::Erased;
    fixture.store.break_audit_for_test();
    assert_excluded(&fixture, row, PersonaProvenanceExclusion::Erased);
}

#[test]
fn captured_persona_provenance_distinguishes_missing_and_unbound_event() {
    let fixture = Fixture::new();
    let mut missing = fixture.produced("persona", 1);
    let Provenance::ProducedBy { source_event_id, .. } = &mut missing.provenance else {
        panic!("produced fixture");
    };
    *source_event_id = Ulid::new();
    assert_excluded(&fixture, missing, PersonaProvenanceExclusion::EventUnavailable);

    let mut unbound = fixture.produced("persona", 2);
    let Provenance::ProducedBy { source_exchange, .. } = &mut unbound.provenance else {
        panic!("produced fixture");
    };
    *source_exchange = fixture.artifacts.put(b"unrelated exchange").unwrap();
    assert_excluded(&fixture, unbound, PersonaProvenanceExclusion::ExchangeNotBound);
}

#[test]
fn captured_persona_provenance_requires_the_recorded_scope_not_any_matching_blob() {
    let fixture = Fixture::new();
    let mut row = fixture.produced("persona", 1);
    let Provenance::ProducedBy { source_scope, .. } = &mut row.provenance else {
        panic!("produced fixture");
    };
    *source_scope = origin_from_producing_scope(Ulid::new());
    assert_excluded(&fixture, row, PersonaProvenanceExclusion::ExchangeUnavailable);
}

#[test]
fn captured_persona_provenance_requires_a_readable_exchange() {
    let fixture = Fixture::new();
    let row = fixture.produced("persona", 1);
    let Provenance::ProducedBy { source_exchange, .. } = &row.provenance else {
        panic!("produced fixture");
    };
    std::fs::remove_file(fixture.artifacts.blob_path_for_test(source_exchange)).unwrap();
    assert_excluded(&fixture, row, PersonaProvenanceExclusion::ExchangeUnavailable);
}

#[test]
fn captured_persona_provenance_requires_a_recorded_yaml_digest() {
    let fixture = Fixture::new();
    let mut row = fixture.produced("persona", 1);
    row.pending_yaml_digest = None;
    assert_excluded(&fixture, row, PersonaProvenanceExclusion::DigestMissing);
}

#[test]
fn captured_persona_provenance_propagates_unreadable_audit_instead_of_empty_findings() {
    let fixture = Fixture::new();
    let row = fixture.produced("persona", 1);
    fixture.store.break_audit_for_test();
    assert!(fixture.capture(&[row]).is_err());
}

#[test]
fn captured_persona_provenance_rejects_ambiguity_before_store_reads() {
    let fixture = Fixture::new();
    let row = fixture.produced("persona", 1);
    fixture.store.break_audit_for_test();
    let error = fixture.capture(&[row.clone(), row]).err().unwrap();
    assert!(error.to_string().contains("duplicate persona provenance"));
}

#[test]
fn captured_persona_provenance_and_evaluation_do_not_write_the_ledger() {
    let fixture = Fixture::new();
    let row = fixture.produced("persona", 1);
    let changes = || {
        fixture.store.with_conn_for_test(|conn| {
            conn.query_row("SELECT total_changes()", [], |row| row.get::<_, i64>(0))
                .unwrap()
        })
    };
    let before = changes();
    let kv_before = fixture.store.all_kv_for_test();
    let capture = fixture.capture(&[row]).unwrap();
    assert!(!capture.evaluate().expected_digests.is_empty());
    assert_eq!(capture.evaluate(), capture.evaluate());
    assert_eq!(changes(), before);
    assert_eq!(fixture.store.all_kv_for_test(), kv_before);
}

#[test]
fn persona_admission_still_requires_exact_yaml_after_provenance_succeeds() {
    let fixture = Fixture::new();
    let row = fixture.produced("persona", 1);
    let dir = fixture.root.path().join("overlay/personas");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("persona.yaml");
    std::fs::write(&path, persona_yaml("persona", 1)).unwrap();
    let admitted = fixture.admit(std::slice::from_ref(&row)).unwrap();
    assert_eq!(admitted.personas["persona"].version, 1);
    std::fs::write(&path, persona_yaml("persona", 1).replace("practical", "different")).unwrap();
    assert!(fixture.admit(&[row]).unwrap().personas.is_empty());
}

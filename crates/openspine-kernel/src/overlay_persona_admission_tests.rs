use super::*;
use crate::store::learned_artifacts::NominationStatus;
use jiff::Timestamp;
use openspine_schemas::artifact::ArtifactNamespace;

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

fn admit_rows(rows: &[LearnedArtifact]) -> anyhow::Result<()> {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(root.path().join("objects"), [3; 32]).unwrap();
    admit(
        &store,
        &artifacts,
        &root.path().join("overlay"),
        rows,
        &mut ArtifactRegistry::default(),
    )
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

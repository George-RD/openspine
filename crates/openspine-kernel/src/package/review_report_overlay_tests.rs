use super::*;
use crate::artifact_store::ArtifactStore;
use crate::store::learned_artifacts::{
    CompatibilityStatus, LearnedArtifact, NominationStatus, Provenance,
};
use crate::store::proposed_artifacts::ProposedArtifact;
use crate::store::Store;
use openspine_schemas::artifact::{ArtifactNamespace, Lifecycle};
use openspine_schemas::package::PackageDeclaration;
use openspine_schemas::route::{Route, RouteEffect};

fn affected_overlay(erased: bool) -> ReviewReport {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(root.path().join("artifacts"), [48; 32]).unwrap();
    let mut route = Route {
        id: "learned-owner".into(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        priority: Some(8),
        effect: RouteEffect::Allow,
        when: Default::default(),
        agent: Some("main_assistant_agent".into()),
        workflow: Some("owner_control_conversation".into()),
        capability_pack: Some("owner_control_basic_pack".into()),
        persona: None,
    };
    route.when.channel_account = Some("private-binding-no-render-if-erased".into());
    let yaml = serde_yaml::to_string(&route).unwrap();
    let digest = digest_of_bytes(yaml.as_bytes()).to_string();
    std::fs::create_dir_all(root.path().join("artifacts.d/routes")).unwrap();
    std::fs::write(root.path().join("artifacts.d/routes/route-v1.yaml"), &yaml).unwrap();
    // The capture's highest on-disk typed row is v2; DB admission must choose v1.
    route.version = 2;
    route.agent = Some("unadmitted-v2-agent".into());
    std::fs::write(
        root.path().join("artifacts.d/routes/route-v2.yaml"),
        serde_yaml::to_string(&route).unwrap(),
    )
    .unwrap();
    let at = "2026-01-01T00:00:00Z".parse().unwrap();
    let proposal_id = ulid::Ulid::new();
    store
        .insert_proposed_artifact(&ProposedArtifact {
            id: proposal_id,
            kind: "route".into(),
            artifact_id: "learned-owner".into(),
            version: 1,
            state: Lifecycle::Proposed,
            yaml_digest: digest.clone(),
            task_grant_id: ulid::Ulid::new(),
            action_request_id: None,
            proposed_at: at,
            lineage: None,
        })
        .unwrap();
    store
        .force_proposed_artifact_state_for_test(proposal_id, Lifecycle::Active)
        .unwrap();
    store
        .record_learned_artifact(&LearnedArtifact {
            kind: "route".into(),
            artifact_id: "learned-owner".into(),
            version: 1,
            namespace: ArtifactNamespace::Overlay,
            provenance: Provenance::LegacyMigration { discovered_at: at },
            accepted_via: None,
            learned_at: at,
            compatibility: if erased {
                CompatibilityStatus::Erased
            } else {
                CompatibilityStatus::Compatible
            },
            nomination: NominationStatus::None,
            pending_reconfirmation_id: None,
            pending_yaml_digest: Some(digest),
            accepted_dependency_fingerprint: None,
            source_path: None,
            accepted_base_epoch: None,
        })
        .unwrap();
    let original = candidate();
    let mut files: BTreeMap<String, Vec<u8>> = original
        .files()
        .map(|(p, b)| (p.to_owned(), b.to_vec()))
        .collect();
    let mut declaration: PackageDeclaration =
        serde_yaml::from_slice(&files["package.yaml"]).unwrap();
    declaration.version += 1;
    declaration.artifacts.routes.push("learned-owner".into());
    route.version = 1;
    route.effect = RouteEffect::Deny;
    route.agent = None;
    route.workflow = None;
    route.capability_pack = None;
    route.when.channel_account = None;
    files.insert(
        "routes/learned-owner.yaml".into(),
        serde_yaml::to_string(&route).unwrap().into_bytes(),
    );
    files.insert(
        "package.yaml".into(),
        serde_yaml::to_string(&declaration).unwrap().into_bytes(),
    );
    let candidate = super::super::super::PackageCapture::new(files)
        .validate()
        .unwrap();
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra");
    let current =
        CapturedCurrentState::capture(&path, "lyra", root.path(), &store, &artifacts).unwrap();
    let overlay =
        super::super::super::review_overlay::assess(&current, candidate.registry()).unwrap();
    let receipt = review(&candidate, 1).plan.candidate;
    let work = store.package_outstanding_work(at).unwrap();
    build(&current, &candidate, receipt, overlay, work)
}

#[test]
fn affected_learned_route_retains_exact_admitted_bindings_and_effective_dispositions() {
    let report = affected_overlay(false);
    let json: Value = serde_json::from_str(&report.json()).unwrap();
    let details = json["overlay_semantics"]["artifacts"]
        .as_array()
        .expect("typed overlay details are required");
    let route = details
        .iter()
        .find(|value| value["artifact"]["id"] == "learned-owner")
        .unwrap();
    assert_eq!(route["artifact"]["version"], 1);
    assert_eq!(route["artifact"]["fields"]["agent"], "main_assistant_agent");
    assert_eq!(
        route["artifact"]["fields"]["workflow"],
        "owner_control_conversation"
    );
    assert_eq!(
        route["artifact"]["fields"]["capability_pack"],
        "owner_control_basic_pack"
    );
    assert_eq!(route["before"]["effective"], true);
    assert_eq!(route["after"]["effective"], false);
    assert!(!report.json().contains("unadmitted-v2-agent"));
}

#[test]
fn erased_overlay_contents_are_never_rendered_as_semantic_details() {
    let report = affected_overlay(true);
    assert!(!report
        .json()
        .contains("private-binding-no-render-if-erased"));
}

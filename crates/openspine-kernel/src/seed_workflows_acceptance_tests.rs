use super::*;
use crate::store::learned_artifacts::{CompatibilityStatus, Provenance};
use openspine_schemas::artifact::Lifecycle;

struct Fixture {
    dir: tempfile::TempDir,
    store: Store,
    artifacts: ArtifactStore,
    base: ArtifactRegistry,
    owner: PrincipalId,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let artifacts = ArtifactStore::open(dir.path().join("artifacts"), [3; 32]).unwrap();
        let base = artifact_loader::load_base_registry(&Self::lyra()).unwrap();
        let owner = crate::identity::bootstrap_owner_principal(&store, 123456789, "Test")
            .unwrap()
            .principal_id;
        Self {
            dir,
            store,
            artifacts,
            base,
            owner,
        }
    }
    fn lyra() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra")
    }
    fn overlay(&self) -> std::path::PathBuf {
        self.dir.path().join("artifacts.d")
    }
    fn accept(&self) -> usize {
        accept_embedded(
            &self.store,
            &self.artifacts,
            &self.overlay(),
            &self.base,
            self.owner,
        )
        .unwrap()
    }
    fn boot(&self) -> crate::overlay_startup::OverlayStartup {
        crate::overlay_startup::load(&Self::lyra(), self.dir.path(), &self.store, &self.artifacts)
            .unwrap()
    }
    fn path(&self, id: &str) -> std::path::PathBuf {
        self.overlay()
            .join("workflows")
            .join(artifact_loader::overlay_filename(id, 1))
    }
}

#[test]
fn init_acceptance_first_boot_has_real_owner_bound_provenance_and_no_reconfirms() {
    let f = Fixture::new();
    assert_eq!(f.accept(), 4);
    let startup = f.boot();
    assert_eq!(f.store.count_action_requests().unwrap(), 0);
    for row in f.store.list_learned_artifacts().unwrap() {
        assert_eq!(row.compatibility, CompatibilityStatus::OwnerAccepted);
        assert!(row.pending_reconfirmation_id.is_none());
        assert_eq!(
            row.accepted_base_epoch.as_deref(),
            Some(startup.base_compatibility_epoch.as_str())
        );
        assert_eq!(
            f.store
                .highest_active_version("workflow", &row.artifact_id)
                .unwrap(),
            Some(1)
        );
        let Provenance::ProducedBy {
            source_event_id,
            source_exchange,
            source_scope,
        } = &row.provenance
        else {
            panic!("real provenance required")
        };
        let event = f
            .store
            .validated_audit_event_by_id(*source_event_id)
            .unwrap()
            .unwrap();
        assert_eq!(event.kind.as_str(), "seed_workflows.owner_accepted");
        assert_eq!(event.actor, Some(f.owner));
        assert!(event.payload_refs.contains(source_exchange));
        let exchange = f
            .artifacts
            .get_scoped(source_scope.producing_scope(), source_exchange)
            .unwrap();
        let receipt: serde_json::Value = serde_json::from_slice(&exchange).unwrap();
        assert_eq!(receipt["owner_principal"], f.owner.to_string());
        assert_eq!(receipt["id"], row.artifact_id);
        assert_eq!(receipt["version"], row.version);
        assert_eq!(
            receipt["embedded_digest"],
            row.pending_yaml_digest.clone().unwrap()
        );
        if row.artifact_id == "customer_service_intake_seed" {
            let refs: Vec<String> =
                serde_json::from_str(row.accepted_dependency_fingerprint.as_deref().unwrap())
                    .unwrap();
            assert_eq!(
                refs,
                [
                    "agent:customer_service_agent",
                    "pack:customer_service_intake_pack"
                ]
            );
            assert!(!startup
                .registry
                .agents
                .contains_key("customer_service_agent"));
            assert!(!startup
                .registry
                .packs
                .contains_key("customer_service_intake_pack"));
        } else {
            assert_eq!(row.accepted_dependency_fingerprint.as_deref(), Some("[]"));
            assert_eq!(
                startup.registry.workflows[&row.artifact_id].lifecycle_state,
                Lifecycle::Active
            );
        }
    }
    assert_eq!(
        f.store
            .count_audit_events_of_kind("artifact.reconfirm_owner_accepted_dangling")
            .unwrap(),
        1
    );
    assert!(f.store.verify_audit_chain().unwrap());
}

#[test]
fn parked_then_init_consumes_exact_requests_atomically_and_is_idempotent() {
    let f = Fixture::new();
    crate::seed_workflows::materialize_missing(&f.store, &f.overlay()).unwrap();
    f.boot();
    let parked = f.store.list_learned_artifacts().unwrap();
    assert_eq!(parked.len(), 4);
    assert_eq!(f.store.count_action_requests().unwrap(), 4);
    assert_eq!(f.accept(), 4);
    for row in &parked {
        assert!(matches!(row.provenance, Provenance::LegacyMigration { .. }));
        assert!(f
            .store
            .is_action_request_used(row.pending_reconfirmation_id.unwrap())
            .unwrap());
    }
    let learned = f.store.list_learned_artifacts().unwrap();
    let audit = f
        .store
        .count_audit_events_of_kind("seed_workflows.owner_accepted")
        .unwrap();
    assert_eq!(f.accept(), 0);
    assert_eq!(
        f.store
            .count_audit_events_of_kind("seed_workflows.owner_accepted")
            .unwrap(),
        audit
    );
    assert_eq!(f.store.list_learned_artifacts().unwrap(), learned);
    f.boot();
    assert_eq!(f.store.count_action_requests().unwrap(), 4);
}

#[test]
fn modified_or_foreign_digest_parked_seed_is_not_accepted() {
    let f = Fixture::new();
    crate::seed_workflows::materialize_missing(&f.store, &f.overlay()).unwrap();
    let path = f.path("owner_control_conversation_seed");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(b"\n# owner edit\n");
    std::fs::write(&path, &bytes).unwrap();
    f.boot();
    let before = f
        .store
        .list_learned_artifacts()
        .unwrap()
        .into_iter()
        .find(|row| row.artifact_id == "owner_control_conversation_seed")
        .unwrap();
    assert_eq!(f.accept(), 3);
    let after = f
        .store
        .list_learned_artifacts()
        .unwrap()
        .into_iter()
        .find(|row| row.artifact_id == before.artifact_id)
        .unwrap();
    assert_eq!(after, before);
    assert!(!f
        .store
        .is_action_request_used(after.pending_reconfirmation_id.unwrap())
        .unwrap());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn later_seed_edit_uses_normal_reconfirmation_and_init_cannot_bypass_it() {
    let f = Fixture::new();
    f.accept();
    let path = f.path("research_and_brief_seed");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(b"\n# later owner edit\n");
    std::fs::write(path, bytes).unwrap();
    let startup = f.boot();
    assert!(!startup
        .registry
        .workflows
        .contains_key("research_and_brief_seed"));
    assert_eq!(f.accept(), 0);
    assert_eq!(
        f.store.count_action_requests().unwrap(),
        0,
        "changed reviewed bytes require re-proposal, not digest substitution"
    );
    assert!(startup
        .pending_reconfirm_notices
        .iter()
        .any(|notice| notice.contains("research_and_brief_seed") && notice.contains("re-propose")));
}

#[test]
fn owner_deleted_seeds_are_not_resurrected_by_init_or_boot() {
    for accepted in [false, true] {
        let f = Fixture::new();
        if accepted {
            f.accept();
        } else {
            crate::seed_workflows::materialize_missing(&f.store, &f.overlay()).unwrap();
            f.boot();
        }
        let path = f.path("research_and_brief_seed");
        std::fs::remove_file(&path).unwrap();
        assert_eq!(f.accept(), if accepted { 0 } else { 3 });
        f.boot();
        assert!(!path.exists());
    }
}

#[test]
fn base_dependency_change_reopens_normal_owner_review() {
    let f = Fixture::new();
    f.accept();
    let empty_base = tempfile::tempdir().unwrap();
    let startup =
        crate::overlay_startup::load(empty_base.path(), f.dir.path(), &f.store, &f.artifacts)
            .unwrap();
    assert!(!startup
        .registry
        .workflows
        .contains_key("research_and_brief_seed"));
    let row = f
        .store
        .list_learned_artifacts()
        .unwrap()
        .into_iter()
        .find(|row| row.artifact_id == "research_and_brief_seed")
        .unwrap();
    assert_eq!(
        row.compatibility,
        CompatibilityStatus::ReconfirmationRequired
    );
    assert_eq!(f.accept(), 0, "init is not a later-reconfirmation bypass");
}

#[test]
fn proposal_failure_rolls_back_parked_request_consumption_and_acceptance_audit() {
    let f = Fixture::new();
    crate::seed_workflows::materialize_missing(&f.store, &f.overlay()).unwrap();
    f.boot();
    let before = f.store.list_learned_artifacts().unwrap();
    f.store.with_conn_for_test(|conn| {
        conn.execute_batch(
            "CREATE TRIGGER reject_seed_proposal BEFORE INSERT ON proposed_artifacts
         BEGIN SELECT RAISE(ABORT, 'injected seed proposal failure'); END;",
        )
        .unwrap()
    });
    assert!(accept_embedded(&f.store, &f.artifacts, &f.overlay(), &f.base, f.owner).is_err());
    assert_eq!(f.store.list_learned_artifacts().unwrap(), before);
    for row in before {
        assert!(!f
            .store
            .is_action_request_used(row.pending_reconfirmation_id.unwrap())
            .unwrap());
    }
    assert_eq!(
        f.store
            .count_audit_events_of_kind("seed_workflows.owner_accepted")
            .unwrap(),
        0
    );
    assert!(f.store.verify_audit_chain().unwrap());
}

#[test]
fn init_refuses_a_parked_digest_that_does_not_match_restored_embedded_file() {
    let f = Fixture::new();
    crate::seed_workflows::materialize_missing(&f.store, &f.overlay()).unwrap();
    let path = f.path("research_and_brief_seed");
    let original = std::fs::read(&path).unwrap();
    std::fs::write(&path, [original.as_slice(), b"\n# changed\n"].concat()).unwrap();
    f.boot();
    std::fs::write(&path, original).unwrap();
    let before = f
        .store
        .list_learned_artifacts()
        .unwrap()
        .into_iter()
        .find(|row| row.artifact_id == "research_and_brief_seed")
        .unwrap();
    assert_eq!(f.accept(), 3);
    let after = f
        .store
        .list_learned_artifacts()
        .unwrap()
        .into_iter()
        .find(|row| row.artifact_id == before.artifact_id)
        .unwrap();
    assert_eq!(after, before);
    assert!(!f
        .store
        .is_action_request_used(after.pending_reconfirmation_id.unwrap())
        .unwrap());
}

#[tokio::test]
async fn accepted_customer_service_seed_cannot_compose_without_its_agent_and_pack() {
    let state = crate::test_support::fixtures::test_state();
    let base = state.registry.read().clone();
    assert_eq!(
        accept_embedded(
            &state.store,
            &state.artifacts,
            &state.overlay_dir,
            &base,
            state.owner.principal_id,
        )
        .unwrap(),
        4
    );
    let startup = crate::overlay_startup::load(
        &Fixture::lyra(),
        state.overlay_dir.parent().unwrap(),
        &state.store,
        &state.artifacts,
    )
    .unwrap();
    let mut registry = startup.registry;
    assert!(registry
        .workflows
        .contains_key("customer_service_intake_seed"));
    let mut route = registry
        .routes
        .iter()
        .find(|route| route.id == "owner_telegram_main_assistant")
        .unwrap()
        .clone();
    route.id = "customer-service-test-route".into();
    route.priority = Some(u32::MAX);
    route.agent = Some("customer_service_agent".into());
    route.workflow = Some("customer_service_intake_seed".into());
    route.capability_pack = Some("customer_service_intake_pack".into());
    registry.routes.push(route);
    *state.registry.write() = registry;
    let update = crate::test_support::fixtures::owner_update("help");
    let error = crate::pipeline::handle_owner_update(&state, &update)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("agent customer_service_agent not in registry"),
        "{error:#}"
    );
    let mut agent = state.registry.read().agents["main_assistant_agent"].clone();
    agent.id = "customer_service_agent".into();
    state
        .registry
        .write()
        .agents
        .insert(agent.id.clone(), agent);
    let error = crate::pipeline::handle_owner_update(&state, &update)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("capability_pack customer_service_intake_pack not in registry"),
        "{error:#}"
    );
    assert_eq!(
        state.store.count_task_grants().unwrap(),
        0,
        "no executable authority was minted"
    );
    assert_eq!(
        state
            .store
            .highest_active_version("workflow", "customer_service_intake_seed")
            .unwrap(),
        Some(1)
    );
}

#[path = "seed_workflows_deletion_tests.rs"]
mod deletion_tests;

#[test]
fn init_under_selection_accepts_seeds_against_the_managed_epoch() {
    use crate::package::managed_source::tests::{variant, Fixture as ManagedFixture};
    let fixture = ManagedFixture::new();
    let source = variant(fixture.root.path(), 3);
    let receipt = fixture.install(&source);
    fixture.select(&receipt);
    std::fs::remove_dir_all(&source).unwrap();
    let owner = crate::identity::bootstrap_owner_principal(&fixture.store, 123, "Owner").unwrap();
    let artifacts = ArtifactStore::open(fixture.data.join("artifacts"), [3; 32]).unwrap();
    assert_eq!(
        accept_for_instance(
            &fixture.store,
            &artifacts,
            &fixture.data,
            &source,
            owner.principal_id
        )
        .unwrap(),
        4
    );
    let selected = crate::package::managed_source::resolve(&fixture.store, &fixture.data)
        .unwrap()
        .unwrap();
    let registry = selected.snapshot.registry();
    let epoch = crate::overlay_compat::compatibility_epoch(
        registry,
        &artifact_loader::artifact_identity_pairs(registry),
    );
    for row in fixture.store.list_learned_artifacts().unwrap() {
        assert_eq!(row.accepted_base_epoch.as_deref(), Some(epoch.as_str()));
    }
    let base =
        crate::managed_startup::startup_base(&fixture.store, &fixture.data, &source).unwrap();
    let started =
        crate::overlay_startup::load_with_base(base, &fixture.data, &fixture.store, &artifacts)
            .unwrap();
    assert!(started.pending_reconfirm_buttons.is_empty());
    assert_eq!(fixture.store.count_action_requests().unwrap(), 0);
}

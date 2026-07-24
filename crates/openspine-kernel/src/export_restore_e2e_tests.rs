//! Focused end-to-end production-path regression for overlay export, restore,
//! terminal erasure carry-forward, newer-base compatibility, permissions,
//! and finalization audit (OpenSpec Task 5.3 / AD-150 / AD-140 / AD-070 / AD-071).
//!
//! Joints / Non-duplicated late-failure and rollback boundaries:
//! - `overlay_startup_tests::provider_failure_retains_pending_finalization`
//! - `overlay_startup_tests::bind_failure_retains_pending_finalization`
//! - `overlay_startup_tests::post_bind_clock_failure_retains_pending_finalization`
//! - `overlay_startup_tests::rollback_pending_restore_flag_is_pathless`
//! - `operation_tests::pathless_rollback_restores_previous_tree`
//! - `operation_tests::rollback_recover_from_each_rename_state`

use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::api::dispatch_tests::OWNER_CHAT_ID;
use crate::api::overlay_export_restore::{handle_overlay_export, handle_overlay_restore};
use crate::artifact_loader;
use crate::artifact_store::ArtifactStore;
use crate::counterparty_erasure::{erase_counterparty, reconcile_overlay_terminal_erasures};
use crate::overlay_export_restore::{acquire as acquire_operations, CompletionMetadata};
use crate::overlay_startup;
use crate::pipeline::AppState;
use crate::secret_store::SecretStore;
use crate::store::learned_artifacts::{
    CompatibilityStatus, LearnedArtifact, NominationStatus, Provenance,
};
use crate::store::Store;
use crate::test_support::fixtures::{repo_lyra_dir, test_state};
use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::{ArtifactNamespace, ArtifactRef};
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::grant::TaskGrant;
use serde_json::json;
use ulid::Ulid;

const MASTER_KEY: [u8; 32] = [17u8; 32];
const EXPORT_ACTION: &str = "openspine.overlay.export";
const RESTORE_ACTION: &str = "openspine.overlay.restore";

async fn mint_composed_owner_grant(state: &AppState, action: &str) -> TaskGrant {
    let grant = crate::pipeline::handle_owner_update(
        state,
        &crate::test_support::fixtures::owner_update(&format!("request {}", action)),
    )
    .await
    .expect("pipeline succeeds")
    .expect("pipeline composes an owner shell grant");

    assert!(grant.verify_mac(b"openspine-test-grant-hmac-key-v1"));
    assert!(grant.effectively_allows(&ActionId::new(EXPORT_ACTION)));
    assert!(grant.effectively_allows(&ActionId::new(RESTORE_ACTION)));
    assert!(grant.effectively_allows(&ActionId::new("openspine.status.read")));
    assert!(state
        .action_catalog
        .is_non_delegable(&ActionId::new(action)));
    grant
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path)
        .unwrap_or_else(|err| panic!("metadata for {} failed: {err}", path.display()))
        .permissions()
        .mode()
        & 0o777
}

fn build_state_at(data_root: &Path, master_key: &[u8; 32]) -> AppState {
    let ops = Arc::new(acquire_operations(data_root, master_key).unwrap());
    let canonical = ops.canonical_data_root().to_path_buf();
    let store_path = canonical.join("kernel.db");
    let store = Store::open(&store_path).unwrap();
    let artifacts_dir = canonical.join("artifacts");
    let credentials_dir = canonical.join("credentials");
    let overlay_dir = canonical.join("artifacts.d");
    fs::create_dir_all(&artifacts_dir).unwrap();
    fs::create_dir_all(&credentials_dir).unwrap();
    fs::create_dir_all(&overlay_dir).unwrap();
    let artifacts = ArtifactStore::open(artifacts_dir, *master_key).unwrap();
    let secrets = Arc::new(SecretStore::open(credentials_dir, *master_key).unwrap());
    let registry = artifact_loader::load_registry(&repo_lyra_dir()).unwrap();
    let owner_principal = store.bootstrap_owner_principal(42, "George").unwrap();
    let base = test_state();

    AppState {
        store,
        artifacts,
        registry: parking_lot::RwLock::new(registry),
        secrets,
        action_catalog: crate::action_catalog::canonical_catalog(),
        sandbox: base.sandbox,
        connectors: base.connectors,
        webhook_verifier: base.webhook_verifier,
        action_handlers: crate::api::handler_registry::ActionHandlerRegistry::default_registrations(
        ),
        owner_user_id: 42,
        owner_principal_id: owner_principal.id,
        owner_identity_id: owner_principal.identity_id,
        kernel_endpoint: "http://127.0.0.1:0".to_string(),
        unsafe_allow_uncontained_private_data: false,
        provider_pool: base.provider_pool,
        gateway_tier_map: base.gateway_tier_map,
        active_model_providers: base.active_model_providers,
        provider_config_digests: base.provider_config_digests,
        started_at: std::time::Instant::now(),
        connector_call_timeout: Duration::from_secs(30),
        overlay_dir,
        base_artifact_ids: HashSet::new(),
        base_compatibility_epoch: String::new(),
        spend_cap: crate::config::SpendCapConfig {
            model_calls_per_day: u64::MAX,
            connector_calls_per_day: u64::MAX,
        },
        conversation_locks: parking_lot::Mutex::new(HashMap::new()),
        overlay_operations: ops,
    }
}

fn write_learned_route(
    overlay_dir: &Path,
    id: &str,
    version: u32,
    agent: &str,
) -> (PathBuf, Vec<u8>) {
    let yaml = format!(
        "id: {id}\nschema_version: 1\nversion: {version}\nlifecycle_state: active\n\
         effect: allow\nagent: {agent}\n"
    );
    let dir = overlay_dir.join("routes");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{id}.yaml"));
    fs::write(&file, yaml.as_bytes()).unwrap();
    (file, yaml.into_bytes())
}

#[tokio::test]
async fn full_export_restore_e2e_regression_under_newer_base() {
    let root = tempfile::tempdir().unwrap();
    let data_root = root.path().join("data");
    fs::create_dir_all(&data_root).unwrap();

    let bundle_name = "e2e-bundle-v1";
    let counterparty_live = Ulid::new();
    let counterparty_later_erased = Ulid::new();

    let (live_ref, later_ref) = {
        let state = build_state_at(&data_root, &MASTER_KEY);
        let live_ref = state
            .artifacts
            .put_scoped(counterparty_live, b"live DM payload")
            .unwrap();
        let later_ref = state
            .artifacts
            .put_scoped(counterparty_later_erased, b"later erased payload")
            .unwrap();

        let learned = LearnedArtifact {
            kind: "route".to_string(),
            artifact_id: "route_learned_live".to_string(),
            version: 1,
            namespace: ArtifactNamespace::Overlay,
            provenance: Provenance::ProducedBy {
                source_event_id: Ulid::new(),
                source_exchange: ArtifactRef {
                    digest: digest_of_bytes(b"exchange-live"),
                    schema_version: 1,
                },
                source_scope: counterparty_live,
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
        };
        state.store.record_learned_artifact(&learned).unwrap();
        let grant = mint_composed_owner_grant(&state, EXPORT_ACTION).await;
        let payload = json!({"bundle_name": bundle_name});
        let result = handle_overlay_export(
            &state,
            &grant,
            &ActionId::new(EXPORT_ACTION),
            OWNER_CHAT_ID,
            Some(&payload),
        )
        .await
        .expect("export handler stages restart");
        assert_eq!(result["restart_required"], true);
        (live_ref, later_ref)
    };

    let (canonical_data_dir, control_dir) = {
        let ops = acquire_operations(&data_root, &MASTER_KEY).unwrap();
        let canonical = ops.canonical_data_root().to_path_buf();
        let control = ops.control_root().to_path_buf();
        let pending = ops
            .process_pre_open(false, Timestamp::now())
            .expect("process export pre-open")
            .expect("export pending finalization");
        let meta = ops.begin_finalization(&pending, Timestamp::now()).unwrap();
        let store = Store::open(&canonical.join("kernel.db")).unwrap();
        crate::append_overlay_finalization_audits(&store, &meta).unwrap();
        ops.complete_finalization(&meta).unwrap();
        (canonical, control)
    };

    let bundle_dir = control_dir.join("snapshots").join(bundle_name);
    assert_eq!(mode(&control_dir), 0o700);
    assert_eq!(mode(&bundle_dir), 0o700);
    assert_eq!(mode(&bundle_dir.join("manifest.json")), 0o600);

    {
        let state = build_state_at(&data_root, &MASTER_KEY);
        let _erase_report = erase_counterparty(
            &state.store,
            &state.artifacts,
            &state.overlay_operations,
            counterparty_later_erased,
        )
        .expect("later counterparty erasure");
        let grant = mint_composed_owner_grant(&state, RESTORE_ACTION).await;
        let payload = json!({"bundle_name": bundle_name});
        let result = handle_overlay_restore(
            &state,
            &grant,
            &ActionId::new(RESTORE_ACTION),
            OWNER_CHAT_ID,
            Some(&payload),
        )
        .await
        .expect("restore handler stages restart");
        assert_eq!(result["restart_required"], true);
    }

    let pending_finalization = {
        let ops = acquire_operations(&data_root, &MASTER_KEY).unwrap();
        ops.process_pre_open(false, Timestamp::now())
            .expect("process restore pre-open")
            .expect("restore pending finalization")
    };

    let lyra_dir = root.path().join("lyra_base");
    fs::create_dir_all(lyra_dir.join("agents")).unwrap();

    let mut base_agent_yaml = fs::read_to_string(
        repo_lyra_dir()
            .join("agents")
            .join("main_assistant_agent.yaml"),
    )
    .unwrap();
    base_agent_yaml = base_agent_yaml.replace("version: 1", "version: 2");
    fs::write(
        lyra_dir.join("agents").join("main_assistant_agent.yaml"),
        base_agent_yaml.as_bytes(),
    )
    .unwrap();

    let (incompat_yaml_path, incompat_bytes) = write_learned_route(
        &canonical_data_dir.join("artifacts.d"),
        "r_incompat",
        1,
        "removed_base_agent",
    );
    let incompat_learned = LearnedArtifact {
        kind: "route".to_string(),
        artifact_id: "r_incompat".to_string(),
        version: 1,
        namespace: ArtifactNamespace::Overlay,
        provenance: Provenance::ProducedBy {
            source_event_id: Ulid::new(),
            source_exchange: ArtifactRef {
                digest: digest_of_bytes(b"exchange-incompat"),
                schema_version: 1,
            },
            source_scope: counterparty_live,
        },
        accepted_via: None,
        learned_at: Timestamp::now(),
        compatibility: CompatibilityStatus::OwnerAccepted,
        nomination: NominationStatus::None,
        pending_reconfirmation_id: None,
        pending_yaml_digest: Some(digest_of_bytes(&incompat_bytes).to_string()),
        accepted_dependency_fingerprint: Some("fingerprint-old".to_string()),
        source_path: Some(incompat_yaml_path.to_string_lossy().into_owned()),
        accepted_base_epoch: Some("epoch-old".to_string()),
    };

    let opened_store = Store::open(&canonical_data_dir.join("kernel.db")).unwrap();
    opened_store
        .record_learned_artifact(&incompat_learned)
        .unwrap();
    let opened_artifacts =
        ArtifactStore::open(canonical_data_dir.join("artifacts"), MASTER_KEY).unwrap();

    reconcile_overlay_terminal_erasures(
        &opened_store,
        &opened_artifacts,
        &acquire_operations(&data_root, &MASTER_KEY).unwrap(),
    )
    .expect("startup erasure reconciliation");

    let startup = overlay_startup::load(
        &lyra_dir,
        &canonical_data_dir,
        &opened_store,
        &opened_artifacts,
    )
    .expect("startup overlay compatibility pass");

    assert_eq!(startup.pending_reconfirm_buttons.len(), 1);
    assert_eq!(mode(&canonical_data_dir), 0o700);
    assert_eq!(mode(&canonical_data_dir.join("kernel.db")), 0o600);

    assert_eq!(
        opened_artifacts
            .get_scoped(counterparty_live, &live_ref)
            .unwrap(),
        b"live DM payload"
    );
    assert!(opened_artifacts
        .get_scoped(counterparty_later_erased, &later_ref)
        .is_err());
    assert!(canonical_data_dir
        .join("keys")
        .join(format!("{counterparty_later_erased}.erased"))
        .is_file());

    let learned_rows = opened_store.list_learned_artifacts().unwrap();
    assert!(
        learned_rows
            .iter()
            .any(|item| item.artifact_id == "route_learned_live"),
        "restored snapshot learned DB row must be present in point-in-time DB"
    );

    let incompat_row = learned_rows
        .iter()
        .find(|item| item.artifact_id == "r_incompat")
        .expect("incompatible learned artifact present");
    assert_eq!(
        incompat_row.compatibility,
        CompatibilityStatus::ReconfirmationRequired
    );
    assert!(incompat_row.pending_reconfirmation_id.is_some());
    assert!(!startup.registry.routes.iter().any(|r| r.id == "r_incompat"));

    let ops = acquire_operations(&data_root, &MASTER_KEY).unwrap();
    let meta: CompletionMetadata = ops
        .begin_finalization(&pending_finalization, Timestamp::now())
        .expect("begin finalization");

    crate::append_overlay_finalization_audits(&opened_store, &meta).unwrap();
    ops.complete_finalization(&meta).unwrap();

    assert_eq!(
        opened_store
            .count_audit_events_of_kind("overlay.restore_requested")
            .unwrap(),
        1
    );
    assert_eq!(
        opened_store
            .count_audit_events_of_kind("overlay.restore_completed")
            .unwrap(),
        1
    );
    assert!(!ops.control_root().join(".pending-operation").exists());
}

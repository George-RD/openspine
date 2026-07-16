//! Shared test fixtures for the `openspine-kernel` crate.
//!
//! This module is declared test-only in `main.rs`, so it is compiled only
//! when running `cargo test --package openspine-kernel`. It exists because
//! both the pipeline tests and the API tests need the same real Lyra
//! artifact registry and a fully wired [`AppState`] — duplication would
//! drift as the fixtures evolve.

#[cfg(test)]
pub(crate) mod fixtures {
    use std::path::Path;

    use crate::api::handler_registry::ActionHandlerRegistry;
    use crate::artifact_store::ArtifactStore;
    use crate::config::{ProviderAuth, ProviderConfig, ProviderKind};
    use crate::connectors::ConnectorRegistry;
    use crate::gmail::GmailConnector;
    use crate::model_gateway::ProviderClient;
    use crate::pipeline::AppState;
    use crate::sandbox::{ProcessDriver, Sandbox};
    use crate::store::Store;
    use crate::telegram::{TelegramConnector, TelegramUpdate};
    use openspine_schemas::digest::Digest;

    pub(crate) fn repo_lyra_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra")
    }
    fn build_state(telegram: TelegramConnector, gmail: Option<GmailConnector>) -> AppState {
        let registry = crate::artifact_loader::load_registry(&repo_lyra_dir()).unwrap();
        let key = [7u8; 32];
        let artifacts_dir = tempfile::tempdir().unwrap().keep();
        // 5a/5d: a per-test overlay dir so activation tests can assert the
        // on-disk overlay file without touching the real fixture tree.
        let overlay_dir = tempfile::tempdir().unwrap().keep();
        let store = Store::open_in_memory().unwrap();
        let owner_principal = store.bootstrap_owner_principal(42, "George").unwrap();
        let test_provider_config = ProviderConfig {
            id: "test-provider".to_string(),
            kind: ProviderKind::Anthropic,
            base_url: None,
            model: "test-model".to_string(),
            auth: ProviderAuth::ApiKey {
                env: "UNUSED".to_string(),
            },
        };
        let test_provider =
            ProviderClient::from_config(&test_provider_config, "unused-test-key".to_string());

        AppState {
            store,
            artifacts: ArtifactStore::open(artifacts_dir, key).unwrap(),
            registry: parking_lot::RwLock::new(registry),
            action_catalog: crate::action_catalog::canonical_catalog(),
            sandbox: Sandbox::Process(ProcessDriver::default()),
            connectors: ConnectorRegistry::new(telegram, gmail)
                .expect("built-in egress ratings are conflict-free"),
            action_handlers: ActionHandlerRegistry::default_registrations(),
            owner_user_id: 42,
            owner_principal_id: owner_principal.id,
            owner_identity_id: owner_principal.identity_id,
            kernel_endpoint: "http://127.0.0.1:0".to_string(),
            unsafe_allow_uncontained_private_data: false,
            provider_pool: std::collections::HashMap::from([(
                "test-provider".to_string(),
                test_provider,
            )]),
            active_model_providers: parking_lot::RwLock::new(std::collections::HashMap::from([
                (
                    openspine_schemas::model_swap::ModelRole::Base,
                    "test-provider".to_string(),
                ),
                (
                    openspine_schemas::model_swap::ModelRole::Matcher,
                    "test-provider".to_string(),
                ),
                (
                    openspine_schemas::model_swap::ModelRole::Miner,
                    "test-provider".to_string(),
                ),
            ])),
            provider_config_digests: std::collections::HashMap::from([(
                "test-provider".to_string(),
                crate::config::provider_config_digest(&test_provider_config),
            )]),
            started_at: std::time::Instant::now(),
            overlay_dir,
        }
    }

    pub(crate) fn test_state() -> AppState {
        build_state(TelegramConnector::new("test-token".to_string()), None)
    }

    pub(crate) fn test_state_with_telegram(telegram: TelegramConnector) -> AppState {
        build_state(telegram, None)
    }

    pub(crate) fn test_state_with_gmail(gmail: GmailConnector) -> AppState {
        build_state(
            TelegramConnector::new("test-token".to_string()),
            Some(gmail),
        )
    }

    pub(crate) fn owner_update(text: &str) -> TelegramUpdate {
        TelegramUpdate {
            update_id: 1,
            chat_id: 555,
            is_private_chat: true,
            sender_user_id: Some(42),
            text: Some(text.to_string()),
            ..Default::default()
        }
    }

    pub(crate) fn seed_owner_history(
        state: &AppState,
        grant: &openspine_schemas::grant::TaskGrant,
    ) {
        let digest = Digest::parse(format!("sha256:{}", "0".repeat(64))).unwrap();
        state
            .store
            .append_conversation_message(grant.id, "user", &digest)
            .unwrap();
    }
}

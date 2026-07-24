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
    use std::time::Duration;

    use crate::api::handler_registry::ActionHandlerRegistry;
    use crate::artifact_store::ArtifactStore;
    use crate::config::{ProviderAuth, ProviderConfig, ProviderKind};
    use crate::connector_reality::WebhookVerifier;
    use crate::connectors::ConnectorRegistry;
    use crate::gmail::GmailConnector;
    use crate::model_gateway::ProviderClient;
    use crate::pipeline::AppState;
    use crate::sandbox::{ProcessDriver, Sandbox};
    use crate::secret_store::SecretStore;
    use crate::store::Store;
    use crate::telegram::{TelegramConnector, TelegramUpdate};
    use openspine_schemas::digest::Digest;

    pub(crate) fn repo_lyra_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra")
    }
    fn build_state(telegram: TelegramConnector, gmail: Option<GmailConnector>) -> AppState {
        build_state_with_store(Store::open_in_memory().unwrap(), telegram, gmail)
    }
    pub(crate) fn build_state_with_store(
        store: Store,
        telegram: TelegramConnector,
        gmail: Option<GmailConnector>,
    ) -> AppState {
        let registry = crate::artifact_loader::load_registry(&repo_lyra_dir()).unwrap();
        let key = [7u8; 32];
        let data_root = tempfile::tempdir().unwrap().keep();
        let overlay_operations = std::sync::Arc::new(
            crate::overlay_export_restore::acquire(&data_root, &key)
                .expect("test overlay operations acquire"),
        );
        let canonical = overlay_operations.canonical_data_root().to_path_buf();
        let artifacts_dir = canonical.join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        let credentials_dir = canonical.join("credentials");
        std::fs::create_dir_all(&credentials_dir).unwrap();
        // Overlay dir under the same canonical data root as production.
        let overlay_dir = canonical.join("artifacts.d");
        std::fs::create_dir_all(&overlay_dir).unwrap();
        let secrets = std::sync::Arc::new(SecretStore::open(credentials_dir, key).unwrap());
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
            secrets,
            action_catalog: crate::action_catalog::canonical_catalog(),
            sandbox: Sandbox::Process(ProcessDriver::default()),
            connectors: ConnectorRegistry::new(telegram, gmail)
                .expect("built-in egress ratings are conflict-free"),
            webhook_verifier: WebhookVerifier::new(
                b"openspine-test-webhook-hmac-key-v1".to_vec(),
                Duration::from_secs(300),
            ),
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
            gateway_tier_map: crate::model_gateway::GatewayTierMap::new(),
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
            connector_call_timeout: std::time::Duration::from_secs(30),
            overlay_dir,
            base_artifact_ids: std::collections::HashSet::new(),
            base_compatibility_epoch: String::new(),
            // Effectively unlimited for unit tests; real configs set a finite cap.
            spend_cap: crate::config::SpendCapConfig {
                model_calls_per_day: i64::MAX as u64,
                connector_calls_per_day: i64::MAX as u64,
            },
            conversation_locks: parking_lot::Mutex::new(std::collections::HashMap::new()),
            overlay_operations,
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

    /// Build a state wired to both a Gmail connector and a caller-supplied
    /// Telegram connector (typically one backed by a `wiremock` `MockServer`
    /// so `answer_callback_query` / `send_reply` can be intercepted). Needed
    /// now that `answer_callback_query` is a typed real connector boundary
    /// whose acknowledgement must be exercised against a mock endpoint.
    pub(crate) fn test_state_with_gmail_and_telegram(
        gmail: GmailConnector,
        telegram: TelegramConnector,
    ) -> AppState {
        build_state(telegram, Some(gmail))
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

    /// Build a real `AppState` over an explicit `Store`. Used by durable
    /// workflow replay tests that must persist a ledger state, then reopen
    /// the same store after a simulated crash/restart and re-run the
    /// production path against it.
    pub(crate) fn test_state_with_store(store: Store) -> AppState {
        build_state_with_store(
            store,
            TelegramConnector::new("test-token".to_string()),
            None,
        )
    }
}

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

    use crate::artifact_store::ArtifactStore;
    use crate::config::{ProviderAuth, ProviderConfig, ProviderKind};
    use crate::gmail::GmailConnector;
    use crate::model_gateway::ProviderClient;
    use crate::pipeline::AppState;
    use crate::sandbox::{ProcessDriver, Sandbox};
    use crate::store::Store;
    use crate::telegram::{TelegramConnector, TelegramUpdate};

    pub(crate) fn repo_lyra_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra")
    }

    pub(crate) fn test_state() -> AppState {
        let registry = crate::artifact_loader::load_registry(&repo_lyra_dir()).unwrap();
        let key = [7u8; 32];
        let artifacts_dir = tempfile::tempdir().unwrap().keep();
        AppState {
            store: Store::open_in_memory().unwrap(),
            artifacts: ArtifactStore::open(artifacts_dir, key).unwrap(),
            registry,
            sandbox: Sandbox::Process(ProcessDriver::default()),
            telegram: TelegramConnector::new("test-token".to_string()),
            owner_user_id: 42,
            kernel_endpoint: "http://127.0.0.1:0".to_string(),
            unsafe_allow_uncontained_private_data: false,
            gmail: None,
            provider: ProviderClient::from_config(
                &ProviderConfig {
                    id: "test-provider".to_string(),
                    kind: ProviderKind::Anthropic,
                    base_url: None,
                    model: "test-model".to_string(),
                    auth: ProviderAuth::ApiKey {
                        env: "UNUSED".to_string(),
                    },
                },
                "unused-test-key".to_string(),
            ),
            started_at: std::time::Instant::now(),
        }
    }

    pub(crate) fn test_state_with_telegram(telegram: TelegramConnector) -> AppState {
        let mut state = test_state();
        state.telegram = telegram;
        state
    }

    pub(crate) fn test_state_with_gmail(gmail: GmailConnector) -> AppState {
        let mut state = test_state();
        state.gmail = Some(gmail);
        state
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
}

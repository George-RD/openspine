//! `openspine` — the OpenSpine kernel binary.
//!
//! Implements `openspec/changes/implement-telegram-owner-control-slice/`
//! (4a-4d): config, storage, artifact store/loader, connectors, minimal
//! model gateway, and the axum kernel API, wired into two concurrent
//! long-running tasks — the HTTP API the shell talks to, and the Telegram
//! long-poll loop that turns owner messages into task grants.

mod action_catalog;
mod api;
mod artifact_loader;
mod artifact_store;
mod benchmark;
mod config;
mod connectors;
mod escalation;
mod gmail;
mod identity;
mod model_gateway;
mod model_swap;
mod model_swap_recovery;
mod overlay_eval_gate;
mod pipeline;
mod sandbox;
mod secret_intake;
mod secret_store;
mod store;
mod telegram;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod kernel_tests;
#[cfg(test)]
mod model_swap_recovery_tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crate::api::handler_registry::ActionHandlerRegistry;
use crate::connectors::ConnectorRegistry;
use anyhow::Context as _;
use clap::Parser;

/// Kernel-owned grant-chain verification key. Production requires explicit
/// secret intake; tests use a deterministic fixture key only under cfg(test).
pub(crate) fn grant_hmac_key() -> Option<Vec<u8>> {
    #[cfg(test)]
    {
        Some(b"openspine-test-grant-hmac-key-v1".to_vec())
    }
    #[cfg(not(test))]
    {
        std::env::var("OPENSPINE_GRANT_HMAC_KEY")
            .ok()
            .filter(|key| !key.is_empty())
            .map(|key| key.into_bytes())
    }
}

/// The kernel binary's own CLI — distinct from `openspine-shell`'s (which
/// takes `--kernel`/`--task`, never a config path: the shell never reads
/// `openspine.yaml`, only `KERNEL_ENDPOINT`/`TASK_TOKEN`, see
/// `docs/kernel-http-contract.md`).
#[derive(Debug, Parser)]
#[command(name = "openspine")]
struct Cli {
    /// Path to `openspine.yaml`.
    #[arg(long, default_value = "openspine.yaml")]
    config: PathBuf,

    /// Run benchmarks instead of starting the daemon.
    #[arg(long)]
    benchmark: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    if cli.benchmark {
        benchmark::run_benchmarks()?;
        return Ok(());
    }
    let cfg = config::Config::load(&cli.config)
        .with_context(|| format!("loading {}", cli.config.display()))?;

    let artifact_key = config::artifact_key_bytes()?;

    let artifacts =
        artifact_store::ArtifactStore::open(cfg.data_dir.join("artifacts"), artifact_key)
            .context("opening artifact store")?;
    let secrets = Arc::new(
        secret_store::SecretStore::open(cfg.data_dir.join("credentials"), artifact_key)
            .context("opening secret store")?,
    );
    let bot_token = if let Some(value) = secrets
        .get_string("telegram.bot_token")
        .context("reading Telegram bot token from vault")?
    {
        value
    } else {
        let value = config::telegram_bot_token()?;
        secrets
            .seed_if_absent("telegram.bot_token", value.as_bytes())
            .context("seeding Telegram bot token")?;
        value
    };
    let store =
        store::Store::open(&cfg.data_dir.join("kernel.db")).context("opening kernel store")?;
    // Bootstrap the owner principal at startup (idempotent, transactional, fail-closed)
    let owner_principal = store
        .bootstrap_owner_principal(cfg.owner.telegram_user_id, &cfg.owner.display_name)
        .context("bootstrapping owner principal failed")?;
    // PRD §18: the audit log is append-only and hash-chained specifically
    // so tampering/corruption is detectable — detect it now, at boot,
    // rather than never. A broken chain means someone edited the SQLite
    // file directly or the process crashed mid-write in a way that left a
    // torn row; either way, refuse to start rather than serve on top of an
    // audit trail that can no longer be trusted.
    if !store
        .verify_audit_chain()
        .context("verifying audit chain")?
    {
        anyhow::bail!(
            "audit_log hash chain is broken in {} — refusing to start on an untrustworthy audit trail",
            cfg.data_dir.join("kernel.db").display()
        );
    }
    let mut registry = artifact_loader::load_registry(&cfg.lyra_dir)
        .with_context(|| format!("loading artifact registry from {}", cfg.lyra_dir.display()))?;
    // Overlay files are committed after their proposal row becomes Active;
    // reconcile the crash window before merging them into the registry.
    let overlay_dir = cfg.data_dir.join("artifacts.d");
    model_swap_recovery::reconcile_model_swap_overlay(&store, &artifacts, &overlay_dir)?;
    artifact_loader::load_registry_into(&mut registry, &overlay_dir)
        .with_context(|| format!("loading artifact overlay from {}", overlay_dir.display()))?;

    let sandbox = match cfg.sandbox.driver {
        config::SandboxDriverKind::Process => {
            sandbox::Sandbox::Process(sandbox::ProcessDriver::default())
        }
        config::SandboxDriverKind::Docker => sandbox::Sandbox::Docker(sandbox::DockerDriver {
            image_tag: cfg
                .sandbox
                .docker_image
                .clone()
                .unwrap_or_else(|| "openspine-shell:latest".to_string()),
            network: cfg
                .sandbox
                .docker_network
                .clone()
                .unwrap_or_else(|| "openspine-internal".to_string()),
            run_as_uid: 10001,
        }),
    };
    let provider_config_digests: HashMap<String, openspine_schemas::digest::Digest> = cfg
        .providers
        .iter()
        .map(|provider| {
            (
                provider.id.clone(),
                config::provider_config_digest(provider),
            )
        })
        .collect();
    let mut provider_pool = HashMap::new();
    for provider_config in &cfg.providers {
        let provider_key = config::provider_api_key(provider_config)?;
        let provider = model_gateway::ProviderClient::from_config(provider_config, provider_key);
        if provider_pool
            .insert(provider_config.id.clone(), provider)
            .is_some()
        {
            anyhow::bail!("duplicate provider id {}", provider_config.id);
        }
    }
    let default_provider_id = cfg
        .providers
        .first()
        .map(|provider| provider.id.clone())
        .ok_or_else(|| anyhow::anyhow!("openspine.yaml must configure at least one provider"))?;
    let mut active_model_providers = HashMap::from([
        (
            openspine_schemas::model_swap::ModelRole::Base,
            default_provider_id.clone(),
        ),
        (
            openspine_schemas::model_swap::ModelRole::Matcher,
            default_provider_id.clone(),
        ),
        (
            openspine_schemas::model_swap::ModelRole::Miner,
            default_provider_id,
        ),
    ]);
    for (id, version) in store.active_model_swap_ids()? {
        let Some(swap) = registry.model_swaps.get(&id) else {
            anyhow::bail!(
                "active model swap {id} v{version} has no matching active overlay; refusing startup"
            );
        };
        if swap.version != version
            || swap.lifecycle_state != openspine_schemas::artifact::Lifecycle::Active
        {
            anyhow::bail!("active model swap {id} v{version} is not active in the loaded overlay");
        }
    }
    for swap in registry.model_swaps.values() {
        if swap.lifecycle_state == openspine_schemas::artifact::Lifecycle::Active {
            let (provenance_state, provenance_digest) = store
                .find_proposed_artifact("model_swap", &swap.id, swap.version)
                .with_context(|| {
                    format!("checking ceremony provenance for active swap {}", swap.id)
                })?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "active model swap {} has no persisted ceremony provenance",
                        swap.id
                    )
                })?;
            if provenance_state != openspine_schemas::artifact::Lifecycle::Active {
                anyhow::bail!(
                    "active model swap {} lacks an Active proposed-artifact provenance row",
                    swap.id
                );
            }
            let verdicts =
                store.eval_verdicts_for_artifact("model_swap", &swap.id, swap.version)?;
            let has_replay = verdicts.iter().any(|v| {
                v.evaluator
                    .as_deref()
                    .is_some_and(|e| e.starts_with("overlay-eval-gate/replay@"))
                    && v.artifact_digest == provenance_digest
                    && v.verdict == "pass"
            });
            let has_judge = verdicts.iter().any(|v| {
                v.evaluator
                    .as_deref()
                    .is_some_and(|e| e.starts_with("overlay-eval-gate/risk-judge@"))
                    && v.artifact_digest == provenance_digest
                    && v.verdict == "pass"
            });
            if !has_replay || !has_judge {
                anyhow::bail!(
                    "active model swap {} has incomplete digest-bound AD-142 provenance",
                    swap.id
                );
            }
            let reviewed_bytes = artifacts.get(&openspine_schemas::artifact::ArtifactRef {
                digest: openspine_schemas::digest::Digest::parse(&provenance_digest)?,
                schema_version: 1,
            })?;
            let reviewed = match artifact_loader::parse_proposal(
                "model_swap",
                std::str::from_utf8(&reviewed_bytes)?,
            )? {
                artifact_loader::ParsedProposal::ModelSwap(manifest) => manifest,
                _ => anyhow::bail!("provenance row for {} is not a model_swap", swap.id),
            };
            let mut loaded_normalized = swap.clone();
            loaded_normalized.lifecycle_state = openspine_schemas::artifact::Lifecycle::Proposed;
            let mut reviewed_normalized = reviewed;
            reviewed_normalized.lifecycle_state = openspine_schemas::artifact::Lifecycle::Proposed;
            if loaded_normalized != reviewed_normalized {
                anyhow::bail!(
                    "active model swap {} differs from its reviewed ceremony manifest",
                    swap.id
                );
            }
            let golden_set = registry
                .golden_sets
                .get(&swap.golden_set_id)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "active model swap {} references missing golden set {}",
                        swap.id,
                        swap.golden_set_id
                    )
                })?;
            if !golden_set.roles.contains(&swap.role) {
                anyhow::bail!(
                    "active model swap {} golden set is not authorized for role {:?}",
                    swap.id,
                    swap.role
                );
            }
            let provider_digest = provider_config_digests
                .get(&swap.target_provider_id)
                .ok_or_else(|| anyhow::anyhow!("missing provider digest"))?;
            model_swap::verify_activation_binding(swap, golden_set, provider_digest)
                .with_context(|| format!("validating active model swap {}", swap.id))?;
            if !provider_pool.contains_key(&swap.target_provider_id) {
                anyhow::bail!(
                    "active model swap {} v{} for role {:?} references missing provider {}; restore it or activate another approved swap",
                    swap.id,
                    swap.version,
                    swap.role,
                    swap.target_provider_id
                );
            }
            active_model_providers.insert(swap.role, swap.target_provider_id.clone());
        }
    }

    let telegram = telegram::TelegramConnector::new_with_store(
        bot_token,
        secrets.clone(),
        "telegram.bot_token".to_string(),
    );

    let gmail = match &cfg.gmail {
        Some(gmail_cfg) => {
            let client_secret_slot = "gmail.client_secret";
            let refresh_token_slot = "gmail.refresh_token";
            if !secrets.contains(client_secret_slot)? {
                if let Ok(value) = config::gmail_client_secret(gmail_cfg) {
                    secrets.seed_if_absent(client_secret_slot, value.as_bytes())?;
                }
            }
            if !secrets.contains(refresh_token_slot)? {
                if let Ok(value) = config::gmail_refresh_token(gmail_cfg) {
                    secrets.seed_if_absent(refresh_token_slot, value.as_bytes())?;
                }
            }
            Some(gmail::GmailConnector::new_with_store(
                gmail_cfg.client_id.clone(),
                secrets.clone(),
                client_secret_slot.to_string(),
                refresh_token_slot.to_string(),
                gmail_cfg.mailbox_address.clone(),
            ))
        }
        None => None,
    };

    let state = Arc::new(pipeline::AppState {
        store,
        artifacts,
        registry: parking_lot::RwLock::new(registry),
        secrets: secrets.clone(),
        action_catalog: crate::action_catalog::canonical_catalog(),
        sandbox,
        connectors: ConnectorRegistry::new(telegram, gmail)?,
        owner_user_id: cfg.owner.telegram_user_id,
        provider_config_digests,
        owner_principal_id: owner_principal.id,
        owner_identity_id: owner_principal.identity_id,
        kernel_endpoint: cfg
            .kernel
            .advertise_endpoint
            .clone()
            .unwrap_or_else(|| format!("http://{}", cfg.kernel.bind_addr)),
        unsafe_allow_uncontained_private_data: cfg.unsafe_allow_uncontained_private_data,
        action_handlers: ActionHandlerRegistry::default_registrations(),
        provider_pool,
        active_model_providers: parking_lot::RwLock::new(active_model_providers),
        started_at: Instant::now(),
        overlay_dir,
    });

    let listener = tokio::net::TcpListener::bind(&cfg.kernel.bind_addr)
        .await
        .with_context(|| format!("binding {}", cfg.kernel.bind_addr))?;
    tracing::info!(addr = %cfg.kernel.bind_addr, owner = cfg.owner.telegram_user_id, "openspine kernel starting");

    let http_server =
        axum::serve(listener, api::router(state.clone())).with_graceful_shutdown(shutdown_signal());
    let telegram_poll = pipeline::run_telegram_poll_loop(&state);

    tokio::select! {
        res = http_server => res.context("http server failed")?,
        res = telegram_poll => res.context("telegram poll loop failed")?,
    }

    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received, draining in-flight requests");
}
